//! Tests live des outils (`--ignored` : réseau Muse + binaires réels).
//!
//! Skippés sans clé (`~/.config/muse/auth.json`) ; le test navigateur
//! lance un vrai Chromium (~1,5 Go, session `crypte-<pid>`, daemon
//! auto-extingué). Jamais joués par la CI.

use super::exec;
use super::flow::tooled_wire;
use super::{defs, WireMsg};
use crate::app::{App, Incoming};
use crate::history::send_window;
use crate::permissions::Mode;
use crate::provider::{Provider, ProviderConfig};

/// Config Muse du store (`None` si absente : le test live est skippé).
fn muse_config() -> Option<ProviderConfig> {
    let path = ProviderConfig::muse_auth_path().ok()?;
    ProviderConfig::from_muse_auth(path).ok()
}

/// App outillée sur le provider donné (yolo : exécute sans demander).
fn live_app(cfg: ProviderConfig) -> App {
    let mut app = App::from_config(cfg).unwrap();
    app.permissions = Mode::Yolo;
    app
}

/// Prochain message async (scrute 120 s max, `None` si timeout).
async fn recv_next(app: &mut App) -> Option<Incoming> {
    for _ in 0..1200 {
        if let Ok(msg) = app.rx.try_recv() {
            return Some(msg);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    None
}

/// Pompe la boucle jusqu'à la fin (40 tours max), rend le nombre de tours.
async fn pump_until_idle(app: &mut App) -> u32 {
    let mut rounds = 0;
    while app.sessions.get(0).is_some_and(|sess| sess.sending) && rounds < 40 {
        rounds += 1;
        match recv_next(app).await {
            Some(Incoming::ToolDone(id, deliveries)) => app.drain_tool_done(id, deliveries),
            Some(Incoming::Chat(id, reply)) => app.drain_chat(id, reply),
            _ => break,
        }
    }
    rounds
}

#[tokio::test]
#[ignore = "live Muse: real tools round-trip (store key)"]
async fn live_muse_emet_des_appels() {
    let Some(cfg) = muse_config() else {
        return;
    };
    let model = cfg.model.clone();
    let provider = Provider::new(cfg).unwrap();
    let wire = vec![WireMsg::text(
        String::from("user"),
        String::from("Call shell_run with cmd set to 'echo hello'. Nothing else."),
    )];
    let done = provider
        .complete_with_tools(&wire, &defs(), &model, None)
        .await
        .expect("live tools call");
    assert!(!done.tool_calls.is_empty(), "no tool_calls: {}", done.text);
    assert_eq!(done.tool_calls[0].name, "shell_run");
}

#[tokio::test]
#[ignore = "live Muse: full tool loop with shell (store key)"]
async fn live_boucle_shell_avec_muse() {
    let Some(cfg) = muse_config() else {
        return;
    };
    let mut app = live_app(cfg);
    let sess = app.sessions.current_mut();
    sess.history
        .push((String::from("user"), String::from("Hi")));
    sess.sending = true;
    let wire = {
        let sess = app.sessions.current();
        let mut wire = tooled_wire(send_window(&sess.history, sess.pinned_system));
        wire.push(WireMsg::text(
            String::from("user"),
            String::from("Run shell_run with cmd 'echo hello', then reply DONE."),
        ));
        wire
    };
    let provider = app.provider.clone().unwrap();
    let model = app.effective_model();
    let reply = provider
        .complete_with_tools(&wire, &defs(), &model, None)
        .await
        .expect("live first call");
    app.drain_chat(0, Ok(reply));
    let rounds = pump_until_idle(&mut app).await;
    assert!(rounds > 0, "loop stuck");
    assert!(
        !app.sessions.get(0).unwrap().sending,
        "still sending after {rounds} rounds"
    );
    assert!(app.tool_run.is_none(), "run not released");
    let texts: Vec<String> = app
        .sessions
        .get(0)
        .unwrap()
        .chat
        .messages()
        .iter()
        .map(|m| m.text.clone())
        .collect();
    assert!(
        texts.iter().any(|t| t.contains("[tool shell_run → ok")),
        "no tool line: {texts:?}"
    );
}

#[tokio::test]
#[ignore = "live browser: launches agent-browser Chromium (~1.5GB)"]
async fn live_navigateur_ouvre_et_lit() {
    let out = exec::run_tool(0, "browser_open", r#"{"url":"https://example.com"}"#).await;
    assert!(out.ok, "open: {}", out.text);
    let out = exec::run_tool(0, "browser_snapshot", "{}").await;
    assert!(out.ok, "snapshot: {}", out.text);
    assert!(out.text.contains("ref=e"), "snapshot: {}", out.text);
    let _ = std::process::Command::new("agent-browser")
        .env(super::browser::SESSION_ENV, super::browser::session())
        .args(["close"])
        .output();
}
