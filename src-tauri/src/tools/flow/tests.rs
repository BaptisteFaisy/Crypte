//! Tests de la boucle outils (machine + porte + lignes).

use super::*;
use crate::kiro::mcp::Config;
use crate::permissions::Rule;

/// Contexte de porte sur dossier temporaire (hooks hermétiques).
fn gate_ctx() -> (Rules, McpState, HooksEngine, std::path::PathBuf) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("bt-flow-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let engine = HooksEngine::new(dir.clone(), None);
    (
        Rules::default(),
        McpState::for_config(Config::default()),
        engine,
        dir,
    )
}

fn completion(calls: Vec<ToolCall>) -> Completion {
    Completion {
        text: String::from("voilà"),
        tool_calls: calls
            .into_iter()
            .map(|c| crate::provider::ToolCall {
                id: c.id,
                name: c.name,
                args: c.args,
            })
            .collect(),
        prompt_tokens: 1,
        completion_tokens: 1,
    }
}

fn call(name: &str) -> ToolCall {
    ToolCall {
        id: String::from("c1"),
        name: name.to_string(),
        args: String::from("{}"),
    }
}

#[test]
fn yolo_execute_livre_puis_relance_le_llm() {
    let (rules, mcp, mut hooks, dir) = gate_ctx();
    let mut flow = ToolFlow::new();
    let mut gate = GateCtx {
        rules: &rules,
        mode: Mode::Yolo,
        mcp: &mcp,
        hooks: &mut hooks,
    };
    let outs = flow.route_calls(0, vec![], &completion(vec![call("shell_run")]), &mut gate);
    assert!(outs
        .iter()
        .any(|o| matches!(o, ToolOut::SetSending { sending: true, .. })));
    assert!(outs.iter().any(|o| matches!(o, ToolOut::SpawnExec { .. })));
    assert_eq!(flow.active_chat(), Some(0));
    let done = Delivery {
        call: call("shell_run"),
        ok: true,
        text: String::from("hi\n"),
        ms: 4,
    };
    let outs = flow.exec_done(0, vec![done], &mut gate);
    assert!(outs.iter().any(|o| matches!(o, ToolOut::Delivered { .. })));
    assert!(outs.iter().any(|o| matches!(o, ToolOut::SpawnLlm { .. })));
    assert!(!outs.iter().any(|o| matches!(o, ToolOut::OmenError)));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn auto_demande_puis_decide_et_perime_l_autre_chat() {
    let (rules, mcp, mut hooks, dir) = gate_ctx();
    let mut flow = ToolFlow::new();
    let mut gate = GateCtx {
        rules: &rules,
        mode: Mode::Auto,
        mcp: &mcp,
        hooks: &mut hooks,
    };
    let outs = flow.route_calls(0, vec![], &completion(vec![call("shell_run")]), &mut gate);
    assert!(outs.iter().any(|o| matches!(o, ToolOut::Confirm { .. })));
    assert!(flow.is_confirming());
    assert_eq!(flow.pending_call().unwrap().name, "shell_run");
    // Livraison d'un autre chat : périmée, ignorée.
    let stale = flow.exec_done(3, vec![], &mut gate);
    assert!(stale.is_empty());
    // Refus : livraison `denied`, présage, relance LLM.
    let outs = flow.decide(false, &mut gate);
    assert!(!flow.is_confirming());
    assert!(outs.iter().any(|o| matches!(o, ToolOut::OmenError)));
    assert!(outs.iter().any(|o| matches!(o, ToolOut::SpawnLlm { .. })));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn regle_deny_refuse_sans_modale() {
    let (mut rules, mcp, mut hooks, dir) = gate_ctx();
    rules.set("shell_run", Rule::Deny);
    let mut flow = ToolFlow::new();
    let mut gate = GateCtx {
        rules: &rules,
        mode: Mode::Yolo,
        mcp: &mcp,
        hooks: &mut hooks,
    };
    let outs = flow.route_calls(0, vec![], &completion(vec![call("shell_run")]), &mut gate);
    assert!(!outs.iter().any(|o| matches!(o, ToolOut::Confirm { .. })));
    assert!(outs.iter().any(|o| matches!(
        o,
        ToolOut::ChatLine { text, .. } if text.contains("denied shell_run (rule)")
    )));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn lignes_appel_resultat_etat() {
    let text: String = "x".repeat(200);
    let c = ToolCall {
        id: String::from("c1"),
        name: String::from("shell_run"),
        args: text,
    };
    assert!(call_line(&c).len() < 140);
    let done = Delivery {
        call: c,
        ok: false,
        text: String::from("boom"),
        ms: 12,
    };
    let line = done_line(&done);
    assert!(line.contains("FAILED") && line.contains("boom"));
    assert!(tools_line(true).contains("tools: on (14 tools"));
    assert_eq!(tools_line(false), "tools: off");
}
