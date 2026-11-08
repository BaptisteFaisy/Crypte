use super::*;
use crate::provider::ProviderConfig;

/// App de test (clé factice, réseau `localhost:9` : refusé ; pas de
/// lecture AGENTS.md : déterministe).
fn test_app() -> App {
    let cfg = ProviderConfig {
        base_url: String::from("http://localhost:9/"),
        api_key: String::from("factice"),
        model: String::from("factice"),
    };
    let mut app = App::from_config(cfg).unwrap();
    app.agents_tried = true;
    app.agents_md = None;
    app.t5.checkpoint_dir = None;
    app
}

/// Texte du dernier message affiché du chat `idx`.
fn last_text(app: &App, idx: usize) -> String {
    app.sessions
        .get(idx)
        .unwrap()
        .chat
        .messages()
        .last()
        .map(|m| m.text.clone())
        .unwrap_or_default()
}

/// Projet `.kiro/` temporaire + `KH_KIRO_PROJECT` pointé dessus. SEUL
/// site de tests qui touche l'env (ne pas imiter : chemins injectés
/// partout ailleurs) : sûr car la fixture ne contient NI steering NI
/// MCP (lecteurs parallèles = vide, comme le défaut hermétique).
fn with_project(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kh-spec-cmds-{}-{name}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kiro/specs")).unwrap();
    std::env::set_var("KH_KIRO_PROJECT", &dir);
    dir
}

/// Bout en bout : gardes, statut, génération, run auto/yolo, échec.
#[tokio::test]
async fn spec_bout_en_bout() {
    let dir = with_project("e2e");
    h_new_gardes(&dir);
    h_show(&dir);
    h_run_gardes(&dir);
    h_drain_gen(&dir);
    h_auto_n(&dir);
    h_yolo_failed(&dir);
    std::env::remove_var("KH_KIRO_PROJECT");
    std::fs::remove_dir_all(&dir).ok();
}

/// `/spec new` : nom invalide, spec existante, déconnecté.
fn h_new_gardes(dir: &Path) {
    let mut app = test_app();
    app.cmd_spec_new(String::from("a/b"), String::from("idea"));
    assert!(last_text(&app, 0).starts_with("invalid spec name"));
    let spec = dir.join(".kiro/specs/auth");
    std::fs::create_dir_all(&spec).unwrap();
    std::fs::write(spec.join("requirements.md"), "req").unwrap();
    app.cmd_spec_new(String::from("auth"), String::from("idea"));
    assert_eq!(last_text(&app, 0), "spec auth already exists — kept as is");
    app.provider = None;
    app.cmd_spec_new(String::from("fresh"), String::from("idea"));
    assert_eq!(last_text(&app, 0), crate::connect::NOT_CONNECTED);
}

/// `/spec <nom>` : inconnu, invalide, puis statut (docs, x/y, prochaine).
fn h_show(dir: &Path) {
    let mut app = test_app();
    app.cmd_spec_show(String::from("nope"));
    assert!(last_text(&app, 0).starts_with("unknown spec: nope"));
    app.cmd_spec_show(String::from("../x"));
    assert!(last_text(&app, 0).starts_with("invalid spec name"));
    let spec = dir.join(".kiro/specs/shop");
    std::fs::create_dir_all(&spec).unwrap();
    std::fs::write(spec.join("requirements.md"), "req").unwrap();
    std::fs::write(spec.join("design.md"), "des").unwrap();
    std::fs::write(spec.join("tasks.md"), "- [x] done\n- [ ] code\n").unwrap();
    app.cmd_spec_show(String::from("shop"));
    assert_eq!(
        last_text(&app, 0),
        "spec shop (req, design, tasks 1/2, next: code)"
    );
}

/// `/spec run` : inconnu, sans tâches, tout fait, déjà en cours, attente.
fn h_run_gardes(dir: &Path) {
    let mut app = test_app();
    app.cmd_spec_run(String::from("nope"));
    assert!(last_text(&app, 0).starts_with("unknown spec: nope"));
    let empty = dir.join(".kiro/specs/empty");
    std::fs::create_dir_all(&empty).unwrap();
    std::fs::write(empty.join("requirements.md"), "req").unwrap();
    app.cmd_spec_run(String::from("empty"));
    assert_eq!(last_text(&app, 0), "spec empty: no tasks yet");
    let full = dir.join(".kiro/specs/full");
    std::fs::create_dir_all(&full).unwrap();
    std::fs::write(full.join("tasks.md"), "- [x] a\n").unwrap();
    app.cmd_spec_run(String::from("full"));
    assert_eq!(last_text(&app, 0), "spec full complete (1/1)");
    // Déjà en cours : run enregistré pour l'agent 7.
    app.spec_runs
        .insert(7, SpecRun::new("busy", 0, 0, 2).with_agent(7));
    app.cmd_spec_run(String::from("busy"));
    assert_eq!(last_text(&app, 0), "spec busy is already running");
    // Une attente `y/n` bloque les autres runs.
    app.spec_runs.clear();
    app.spec_waiting = Some(SpecRun::new("wait", 0, 1, 2));
    app.cmd_spec_run(String::from("other"));
    assert_eq!(last_text(&app, 0), "spec wait waits for y/n — answer first");
}

/// `drain_spec_new` : étape finale écrite, résumé, ledger ; erreur gardée.
fn h_drain_gen(dir: &Path) {
    let mut app = test_app();
    let spec = dir.join(".kiro/specs/gen");
    std::fs::create_dir_all(&spec).unwrap();
    std::fs::write(spec.join("requirements.md"), "R1").unwrap();
    std::fs::write(spec.join("design.md"), "D1").unwrap();
    let mut gen = SpecGen::new("gen", "idea");
    gen.requirements = String::from("R1");
    gen.design = String::from("D1");
    gen.step = 2;
    let done = Completion {
        text: String::from("- [ ] a\n- [ ] b\n"),
        tool_calls: Vec::new(),
        prompt_tokens: 10,
        completion_tokens: 5,
    };
    app.drain_spec_new(0, gen, Ok(done));
    assert_eq!(
        std::fs::read_to_string(spec.join("tasks.md")).unwrap(),
        "- [ ] a\n- [ ] b\n"
    );
    assert_eq!(
        last_text(&app, 0),
        "spec gen ready: requirements + design + 2 tasks (3/3)"
    );
    assert_eq!(app.sessions.get(0).unwrap().ledger.total(), 15);
    // Erreur à mi-chaîne : fichiers gardés annoncés.
    let mut gen = SpecGen::new("gen", "idea");
    gen.requirements = String::from("R1");
    gen.step = 1;
    app.drain_spec_new(0, gen, Err(anyhow::anyhow!("boom")));
    assert!(last_text(&app, 0).contains("design failed: boom"));
    assert!(last_text(&app, 0).contains("kept: requirements.md"));
}

/// Tâche finie en `auto` : cochée sur disque, `y/n` demandé, `n` stoppe.
fn h_auto_n(dir: &Path) {
    let mut app = test_app();
    app.permissions = Mode::Auto;
    let spec = dir.join(".kiro/specs/auto");
    std::fs::create_dir_all(&spec).unwrap();
    std::fs::write(spec.join("requirements.md"), "R").unwrap();
    std::fs::write(spec.join("tasks.md"), "- [ ] a\n- [ ] b\n").unwrap();
    app.spec_runs
        .insert(1, SpecRun::new("auto", 0, 0, 2).with_agent(1));
    app.spec_agent_done(1);
    assert_eq!(
        std::fs::read_to_string(spec.join("tasks.md")).unwrap(),
        "- [x] a\n- [ ] b\n"
    );
    assert!(app.spec_waiting.is_some());
    assert!(last_text(&app, 0).contains("run task 2/2"));
    assert!(last_text(&app, 0).contains("(y/n)"));
    // `n` : run stoppé, prompt restauré.
    app.spec_resolve(false);
    assert!(app.spec_waiting.is_none());
    assert_eq!(last_text(&app, 0), "spec auto run stopped (1/2 done)");
    // Agent hors spec : ignoré silencieusement.
    let before = last_text(&app, 0);
    app.spec_agent_done(42);
    app.spec_agent_failed(42);
    assert_eq!(last_text(&app, 0), before);
    // `y` : tâche suivante lancée via un sous-agent (parent + enfant niché).
    app.spec_waiting = Some(SpecRun::new("auto", 0, 1, 2));
    app.spec_resolve(true);
    assert_eq!(app.spec_runs.len(), 1);
    let msgs = app.sessions.get(0).unwrap().chat.messages();
    let n = msgs.len();
    assert!(msgs[n - 2].text.contains("task 2/2"), "parent spec");
    assert!(msgs[n - 1].text.contains("agent#1"), "enfant niché");
    let (_, agent) = app.sessions.find_agent_mut(1).unwrap();
    assert_eq!(
        agent.spec.as_ref().map(|s| s.name.as_str()),
        Some("auto"),
        "tag workflow posé"
    );
}

/// Tâche finie en `yolo` : enchaînée direct ; échec agent = stop.
fn h_yolo_failed(dir: &Path) {
    let mut app = test_app();
    app.permissions = Mode::Yolo;
    let spec = dir.join(".kiro/specs/yolo");
    std::fs::create_dir_all(&spec).unwrap();
    std::fs::write(spec.join("tasks.md"), "- [ ] a\n- [ ] b\n").unwrap();
    app.spec_runs
        .insert(1, SpecRun::new("yolo", 0, 0, 2).with_agent(1));
    app.spec_agent_done(1);
    assert!(app.spec_waiting.is_none());
    assert_eq!(app.spec_runs.len(), 1);
    let msgs = app.sessions.get(0).unwrap().chat.messages();
    let n = msgs.len();
    assert!(msgs[n - 2].text.contains("task 2/2"), "parent spec");
    assert!(msgs[n - 1].text.contains("agent#"), "enfant niché");
    let run_agent = app.spec_runs.values().next().unwrap().agent.unwrap();
    let (_, agent) = app.sessions.find_agent_mut(run_agent).unwrap();
    assert_eq!(
        agent.spec.as_ref().map(|s| s.name.as_str()),
        Some("yolo"),
        "tag workflow posé"
    );
    // Dernière tâche finie : run terminé, message `complete`.
    let id = *app.spec_runs.keys().next().unwrap();
    app.spec_agent_done(id);
    assert!(app.spec_runs.is_empty());
    assert_eq!(last_text(&app, 0), "spec yolo complete (2/2)");
    // Échec agent : run stoppé, tâche NON cochée.
    std::fs::write(spec.join("tasks.md"), "- [ ] a\n- [ ] b\n").unwrap();
    app.spec_runs
        .insert(9, SpecRun::new("yolo", 0, 0, 2).with_agent(9));
    app.spec_agent_failed(9);
    assert!(app.spec_runs.is_empty());
    assert_eq!(
        last_text(&app, 0),
        "spec yolo: task 1/2 failed — run stopped"
    );
    assert!(std::fs::read_to_string(spec.join("tasks.md"))
        .unwrap()
        .contains("- [ ] a"));
}

/// Sans projet : écriture doc (`\n` final) + touches hors attente.
#[test]
fn divers_sans_projet() {
    let dir = std::env::temp_dir().join(format!("kh-spec-divers-{}", std::process::id()));
    write_doc(None, &dir, "requirements.md", "R1").unwrap();
    write_doc(None, &dir, "design.md", "D1\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("requirements.md")).unwrap(),
        "R1\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("design.md")).unwrap(),
        "D1\n"
    );
    // Snapshots via dossier injecté (2e écriture → 1 snapshot).
    let ckpt = dir.join("ckpt");
    write_doc(Some(&ckpt), &dir, "tracked.md", "v1").unwrap();
    write_doc(Some(&ckpt), &dir, "tracked.md", "v2").unwrap();
    assert_eq!(crate::kiro::checkpoint::list_in(&ckpt).len(), 1);
    std::fs::remove_dir_all(&dir).ok();
    let mut app = test_app();
    assert!(app.spec_waiting.is_none());
    let key = KeyEvent::new(KeyCode::Char('y'), crossterm::event::KeyModifiers::empty());
    assert!(app.on_spec_key(key));
    assert!(app.spec_runs.is_empty());
}
