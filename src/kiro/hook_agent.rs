//! Hooks des outils sous-agents : tir depuis les tâches détachées.
//!
//! Les tâches agents n'ont pas accès à `App` : elles sélectionnent
//! (mêmes fichiers + matchers que les chats) et lancent les commandes
//! en fond (même tombée `HookDone` sur le chat propriétaire, même
//! historique `/hooks`). Les prompts `agent` sont retournés pour
//! injection au tour suivant (équivalent du « prochain envoi » des
//! chats). Vide ou type opaque = ignoré sans trace (les chats, eux,
//! notent `skipped`). Pas de récursion : la tombée ne re-tire jamais.

use std::path::PathBuf;

use tokio::sync::mpsc::Sender;

use super::hook_run::select_hooks;
use super::hooks::Trigger;
use crate::app::Incoming;

/// Chemins Kiro injectés (tests : jouets, jamais les vrais hooks user).
#[derive(Debug, Clone)]
pub struct HookPaths {
    pub project: PathBuf,
    pub global: Option<PathBuf>,
}

/// Tir : commandes lancées (tombée async) + prompts à injecter.
#[derive(Debug, Clone, Default)]
pub struct HookFired {
    pub commands: usize,
    pub prompts: Vec<String>,
}

/// Tire les hooks du trigger pour l'outil (commandes en fond, prompts
/// retournés) ; `Unknown` = rien, comme les chats. Seuls les outils
/// exécutés tirent (un refus ne tire pas, voir `tools::agent`).
pub fn fire_tool_hooks(
    paths: &HookPaths,
    tx: &Sender<Incoming>,
    trigger: Trigger,
    tool: &str,
    chat: usize,
) -> HookFired {
    if trigger == Trigger::Unknown {
        return HookFired::default();
    }
    let mut fired = HookFired::default();
    let selected = select_hooks(trigger, tool, &paths.project, paths.global.as_deref());
    for hook in &selected {
        match hook.action.kind.as_str() {
            "command" => {
                let cmd = hook
                    .action
                    .command
                    .as_deref()
                    .filter(|cmd| !cmd.trim().is_empty());
                if let Some(cmd) = cmd {
                    super::hook_exec::spawn_command(
                        tx,
                        chat,
                        &hook.name,
                        cmd,
                        trigger,
                        &paths.project,
                    );
                    fired.commands += 1;
                }
            }
            "agent" => {
                let prompt = hook
                    .action
                    .prompt
                    .as_deref()
                    .filter(|prompt| !prompt.trim().is_empty());
                if let Some(prompt) = prompt {
                    fired.prompts.push(prompt.to_string());
                }
            }
            _ => {}
        }
    }
    fired
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("kh-hook-agent-{name}-{}", std::process::id()))
    }

    /// Projet jouet : hook commande (Pre, matcher shell) + hook agent
    /// (Post) + hook vide + hook opaque (tous deux ignorés).
    fn seed(tag: &str) -> std::path::PathBuf {
        let dir = tmp_dir(tag);
        let hooks = dir.join(".kiro/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        std::fs::write(
            hooks.join("mix.json"),
            r#"{"hooks":[
              {"name":"log","trigger":"PreToolUse","matcher":"shell",
               "action":{"type":"command","command":"echo coucou"}},
              {"name":"vide","trigger":"PreToolUse",
               "action":{"type":"command","command":"  "}},
              {"name":"etrange","trigger":"PreToolUse",
               "action":{"type":"ovni","command":"echo non"}},
              {"name":"suite","trigger":"PostToolUse",
               "action":{"type":"agent","prompt":"Résume."}}]}"#,
        )
        .unwrap();
        dir
    }

    /// Chemins du projet jouet (pas de global).
    fn paths(dir: &std::path::Path) -> HookPaths {
        HookPaths {
            project: dir.to_path_buf(),
            global: None,
        }
    }

    #[tokio::test]
    async fn tir_commande_tombee_et_prompt_post() {
        let dir = seed("tir");
        let paths = paths(&dir);
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let fired = fire_tool_hooks(&paths, &tx, Trigger::PreToolUse, "shell_run", 3);
        assert_eq!(fired.commands, 1);
        assert!(fired.prompts.is_empty());
        let got = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        let Incoming::HookDone(out) = got else {
            panic!("attendu HookDone, reçu autre");
        };
        assert_eq!(out.chat, 3);
        assert_eq!(out.name, "log");
        assert_eq!(out.trigger, Trigger::PreToolUse);
        assert!(out.ok);
        assert!(out.text.contains("coucou"), "{}", out.text);
        let fired = fire_tool_hooks(&paths, &tx, Trigger::PostToolUse, "shell_run", 3);
        assert_eq!(fired.commands, 0);
        assert_eq!(fired.prompts, vec!["Résume."]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tir_matcher_et_trigger_filtres() {
        let dir = seed("filtres");
        let paths = paths(&dir);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let fired = fire_tool_hooks(&paths, &tx, Trigger::PreToolUse, "browser_open", 0);
        assert_eq!(fired.commands, 0);
        assert!(fired.prompts.is_empty());
        let fired = fire_tool_hooks(&paths, &tx, Trigger::Unknown, "shell_run", 0);
        assert_eq!(fired.commands, 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
