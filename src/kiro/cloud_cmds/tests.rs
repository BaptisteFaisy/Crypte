use super::*;

/// App de test (clé factice, aucun appel réseau hors stub).
fn test_app() -> App {
    let cfg = crate::provider::ProviderConfig {
        base_url: String::from("http://localhost:9/"),
        api_key: String::from("factice"),
        model: String::from("factice"),
    };
    App::from_config(cfg).unwrap()
}

/// Dernière ligne affichée du chat courant.
fn last_text(app: &App) -> String {
    app.sessions
        .get(0)
        .unwrap()
        .chat
        .messages()
        .last()
        .map(|msg| msg.text.clone())
        .unwrap_or_default()
}

/// Tout le chat courant (asserts `contains` multi-lignes).
fn all_text(app: &App) -> String {
    app.sessions
        .get(0)
        .unwrap()
        .chat
        .messages()
        .iter()
        .map(|msg| msg.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// CLI stubbé absent : `/cloud` et `/cloud list` expliquent.
#[test]
fn status_et_list_sans_cli_expliquent() {
    let scope = crate::test_env::EnvScope::new(&["KH_KIRO_PATH", "KH_KIRO_CLI"]);
    scope.set("KH_KIRO_PATH", "/nonexistent");
    scope.unset("KH_KIRO_CLI");
    let mut app = test_app();
    app.run_command("/cloud");
    assert!(
        last_text(&app).contains("introuvable"),
        "{}",
        last_text(&app)
    );
    app.run_command("/cloud list");
    assert!(
        last_text(&app).contains("introuvable"),
        "{}",
        last_text(&app)
    );
}

/// `/cloud run` sans tâche : aide d'usage (CLI stubbé présent).
#[test]
fn run_sans_tache_aide() {
    let dir = std::env::temp_dir().join(format!("kh-cloud-u-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let stub = dir.join("kiro-cli");
    std::fs::write(&stub, "#!/bin/sh\necho 'kiro-cli 2.17.0'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let scope = crate::test_env::EnvScope::new(&["KH_KIRO_CLI", "KH_KIRO_PROJECT"]);
    scope.set("KH_KIRO_CLI", &stub);
    scope.set("KH_KIRO_PROJECT", &dir);
    let mut app = test_app();
    app.run_command("/cloud run");
    assert!(
        last_text(&app).contains("usage: /cloud run"),
        "{}",
        last_text(&app)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `/cloud attach` imprime reprise + web, sans id : aide.
#[test]
fn attach_imprime_reprise() {
    let mut app = test_app();
    app.run_command("/cloud attach abc123");
    let text = last_text(&app);
    assert!(text.contains("kiro-cli --resume-id abc123"), "{text}");
    assert!(text.contains("https://app.kiro.dev"), "{text}");
    app.run_command("/cloud attach");
    assert!(
        last_text(&app).contains("usage: /cloud attach"),
        "{}",
        last_text(&app)
    );
}

/// `/cloud stop` sans run : message clair (pas de spawn).
#[test]
fn stop_sans_run_clair() {
    let mut app = test_app();
    app.run_command("/cloud stop");
    assert!(
        last_text(&app).contains("usage: /cloud stop"),
        "{}",
        last_text(&app)
    );
    app.run_command("/cloud stop #3");
    assert_eq!(last_text(&app), "cloud: no run #3");
}

/// Ligne drainée : taggée `[cloud#n]` dans le bon chat.
#[test]
fn drain_line_taggee() {
    let mut app = test_app();
    app.drain_cloud(
        0,
        0,
        CloudMsg::Line(CloudLine {
            text: String::from("hi"),
            session: None,
        }),
    );
    assert_eq!(last_text(&app), "[cloud#0] hi");
}

/// Fin drainée : job inconnu sans session ignoré ; job 0 = brut.
#[test]
fn drain_done_inconnu_ignore_job0_brut() {
    let mut app = test_app();
    let before = all_text(&app);
    app.drain_cloud(
        0,
        7,
        CloudMsg::Done(CloudOutcome {
            session: None,
            ok: true,
            detail: String::new(),
        }),
    );
    assert_eq!(all_text(&app), before);
    app.drain_cloud(
        0,
        0,
        CloudMsg::Done(CloudOutcome {
            session: None,
            ok: true,
            detail: String::from("sessions:\n  abc (cloud, running)"),
        }),
    );
    assert!(
        last_text(&app).contains("abc (cloud, running)"),
        "{}",
        last_text(&app)
    );
}

/// Run complet via stub : ack, lignes streamées, résumé session.
#[tokio::test]
async fn run_stub_stream_et_resume() {
    let dir = std::env::temp_dir().join(format!("kh-cloud-stub-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let stub = dir.join("kiro-cli");
    std::fs::write(
        &stub,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'kiro-cli 2.17.0'; exit 0; fi\necho '{\"session_id\":\"s9\",\"text\":\"hello cloud\"}'\necho 'plain tail'\nexit 0\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let scope = crate::test_env::EnvScope::new(&["KH_KIRO_CLI", "KH_KIRO_PROJECT"]);
    scope.set("KH_KIRO_CLI", &stub);
    scope.set("KH_KIRO_PROJECT", &dir);
    let mut app = test_app();
    app.run_command("/cloud run --repo o/r hello");
    assert!(
        all_text(&app).contains("cloud#1 on o/r"),
        "{}",
        all_text(&app)
    );
    let mut done = false;
    for _ in 0..100 {
        match tokio::time::timeout(Duration::from_millis(50), app.rx.recv()).await {
            Ok(Some(Incoming::Cloud(chat, job, msg))) => {
                done = matches!(msg, CloudMsg::Done(_));
                app.drain_cloud(chat, job, msg);
                if done {
                    break;
                }
            }
            _ => break,
        }
    }
    assert!(done, "pas de Done reçu");
    let all = all_text(&app);
    assert!(all.contains("[cloud#1] hello cloud"), "{all}");
    assert!(all.contains("[cloud#1] plain tail"), "{all}");
    assert!(all.contains("kiro-cli --resume-id s9"), "{all}");
    assert!(all.contains("https://app.kiro.dev"), "{all}");
    drop(scope);
    let _ = std::fs::remove_dir_all(&dir);
}
