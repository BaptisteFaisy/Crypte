use super::*;
use crate::permissions::Rule;

/// Schéma jouet (nom seul significatif pour le filtre).
fn def(name: &str) -> ToolDef {
    ToolDef::new(name, "jouet", serde_json::Value::Null)
}

#[test]
fn filtre_vide_garde_tout() {
    let all = vec![def("shell_run"), def("mcp_gh_search")];
    let got = filter_tools(&all, &Filter::default());
    assert_eq!(got.len(), 2);
}

#[test]
fn filtre_allowlist_exact_et_court_mcp() {
    let all = vec![def("shell_run"), def("browser_open"), def("mcp_gh_search")];
    let filter = Filter {
        tools: vec![String::from("shell_run"), String::from("search")],
        ..Filter::default()
    };
    let got = filter_tools(&all, &filter);
    let got: Vec<&str> = got.iter().map(|d| d.function.name.as_str()).collect();
    assert_eq!(got, vec!["shell_run", "mcp_gh_search"]);
}

#[test]
fn filtre_exclus_puis_serveurs_mcp() {
    let all = vec![def("shell_run"), def("mcp_gh_search"), def("mcp_jira_get")];
    let filter = Filter {
        excluded: vec![String::from("shell_run")],
        mcp_servers: vec![String::from("gh")],
        ..Filter::default()
    };
    let got = filter_tools(&all, &filter);
    let got: Vec<&str> = got.iter().map(|d| d.function.name.as_str()).collect();
    assert_eq!(got, vec!["mcp_gh_search"]);
}

#[test]
fn decision_auto_lecture_ok_modif_refusee() {
    let rules = Rules::default();
    assert!(allowed(&rules, Mode::Auto, "browser_snapshot"));
    assert!(!allowed(&rules, Mode::Auto, "shell_run"));
    assert!(allowed(&rules, Mode::Yolo, "shell_run"));
}

#[test]
fn decision_regles_prient_sur_mode() {
    let mut rules = Rules::default();
    rules.set("shell_run", Rule::Allow);
    rules.set("browser_*", Rule::Deny);
    assert!(allowed(&rules, Mode::Auto, "shell_run"));
    assert!(!allowed(&rules, Mode::Yolo, "browser_snapshot"));
}

#[test]
fn resume_vide_sans_appels_compte_sinon() {
    assert!(summarize(&[]).is_empty());
    let calls = vec![
        CallRecord {
            name: String::from("a"),
            ok: true,
            refused: false,
        },
        CallRecord {
            name: String::from("b"),
            ok: false,
            refused: true,
        },
    ];
    assert_eq!(
        summarize(&calls),
        "\n\n(tools: 2 calls — 1 ok, 0 failed, 1 refused [refused: b])"
    );
}

#[tokio::test]
async fn execute_shell_echo_hermetique() {
    let mcp = McpState::for_config(crate::kiro::mcp::Config::default());
    let out = execute_one(&mcp, 0, "shell_run", r#"{"cmd":"echo coucou"}"#).await;
    assert!(out.ok);
    assert!(out.text.contains("coucou"));
}

#[tokio::test]
async fn execute_mcp_inconnu_erreur_rapide() {
    let mcp = McpState::for_config(crate::kiro::mcp::Config::default());
    let out = execute_one(&mcp, 0, "mcp_fantome_outil", "{}").await;
    assert!(!out.ok);
    assert!(!out.text.is_empty());
}

/// Stub HTTP 127.0.0.1 : rejoue les corps dans l'ordre (2 requêtes).
async fn stub_provider(bodies: Vec<String>) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        for body in bodies {
            let (mut sock, _) = listener.accept().await.unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let total = loop {
                let n = sock.read(&mut chunk).await.unwrap();
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf);
                if let Some(at) = text.find("\r\n\r\n") {
                    let len = text[..at]
                        .lines()
                        .filter_map(|line| line.split_once(':'))
                        .filter(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
                        .filter_map(|(_, num)| num.trim().parse::<usize>().ok())
                        .next()
                        .unwrap_or(0);
                    break at + 4 + len;
                }
            };
            while buf.len() < total {
                let n = sock.read(&mut chunk).await.unwrap();
                buf.extend_from_slice(&chunk[..n]);
            }
            let http = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            sock.write_all(http.as_bytes()).await.unwrap();
        }
    });
    (base, task)
}

#[tokio::test]
async fn boucle_shell_puis_texte_final() {
    let tool = r#"{"choices":[{"message":{"content":"je regarde","tool_calls":[{"id":"c1","type":"function","function":{"name":"shell_run","arguments":"{\"cmd\":\"echo salut\"}"}}]}}],"usage":{"prompt_tokens":7,"completion_tokens":3}}"#;
    let final_msg = r#"{"choices":[{"message":{"content":"fini","tool_calls":[]}}],"usage":{"prompt_tokens":5,"completion_tokens":2}}"#;
    let (base, stub) = stub_provider(vec![tool.to_string(), final_msg.to_string()]).await;
    let provider = Provider::new(crate::provider::ProviderConfig {
        base_url: base,
        api_key: String::from("test"),
        model: String::from("jouet"),
    })
    .unwrap();
    let mcp = McpState::for_config(crate::kiro::mcp::Config::default());
    let rules = Rules::default();
    let ctx = Ctx {
        provider: &provider,
        model: "jouet",
        effort: None,
        rules: &rules,
        mode: Mode::Yolo,
        mcp: &mcp,
        chat: 0,
    };
    let done = run_loop(
        ctx,
        vec![(String::from("user"), String::from("salut ?"))],
        vec![exec::shell_def()],
    )
    .await
    .unwrap();
    stub.await.unwrap();
    assert!(done.text.contains("fini"), "texte : {}", done.text);
    assert!(done
        .text
        .contains("(tools: 1 call — 1 ok, 0 failed, 0 refused)"));
    assert!(done.tool_calls.is_empty());
    assert_eq!(done.prompt_tokens, 12);
    assert_eq!(done.completion_tokens, 5);
}

#[tokio::test]
async fn boucle_refus_auto_rejoue_sans_executer() {
    let tool = r#"{"choices":[{"message":{"content":"","tool_calls":[{"id":"c1","type":"function","function":{"name":"shell_run","arguments":"{\"cmd\":\"echo non\"}"}}]}}],"usage":{"prompt_tokens":1,"completion_tokens":1}}"#;
    let final_msg = r#"{"choices":[{"message":{"content":"pigé","tool_calls":[]}}],"usage":{"prompt_tokens":1,"completion_tokens":1}}"#;
    let (base, stub) = stub_provider(vec![tool.to_string(), final_msg.to_string()]).await;
    let provider = Provider::new(crate::provider::ProviderConfig {
        base_url: base,
        api_key: String::from("test"),
        model: String::from("jouet"),
    })
    .unwrap();
    let mcp = McpState::for_config(crate::kiro::mcp::Config::default());
    let rules = Rules::default();
    let ctx = Ctx {
        provider: &provider,
        model: "jouet",
        effort: None,
        rules: &rules,
        mode: Mode::Auto,
        mcp: &mcp,
        chat: 0,
    };
    let done = run_loop(
        ctx,
        vec![(String::from("user"), String::from("go"))],
        vec![exec::shell_def()],
    )
    .await
    .unwrap();
    stub.await.unwrap();
    assert!(done.text.contains("pigé"), "texte : {}", done.text);
    assert!(done.text.contains("0 ok, 0 failed, 1 refused"));
}
