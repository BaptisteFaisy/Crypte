//! Commandes specs (§6) : `spec_new|show|run|respond`.
//!
//! `spec_new`/`spec_run` produisent des sorties outbox (LLM/agents
//! exécutés par l'intégrateur) ; `spec_respond` résout l'attente y/n
//! (`y…` lance, `n…` stoppe, le reste garde l'attente).

use std::sync::Mutex;

use tauri::State;

use crate::state::{lock, ToolsBackend};
use crate::views::{SpecRunAck, SpecTaskView, SpecView};

/// `spec_new` : génère requirements + design + tasks (3 appels chaînés
/// via outbox `SpawnGen`, fichiers écrits + snapshotés). Les lignes
/// vont au chat courant (comme le TUI).
#[tauri::command]
pub fn spec_new(
    backend: State<Mutex<ToolsBackend>>,
    name: String,
    idea: String,
) -> Result<(), String> {
    let mut backend = lock(&backend)?;
    let chat = backend.current_chat;
    backend.spec_gen_start_cmd(&name, &idea, chat)
}

/// `spec_show` : docs présentes, tâches x/y (erreur si inconnue).
#[tauri::command]
pub fn spec_show(backend: State<Mutex<ToolsBackend>>, name: String) -> Result<SpecView, String> {
    let backend = lock(&backend)?;
    if !crate::kiro::spec::safe_name(&name) {
        return Err(format!("invalid spec name: {name}"));
    }
    let dir = crate::kiro::spec::spec_dir(&backend.kiro_project, &name);
    match crate::kiro::spec::load(&dir) {
        None => Err(format!(
            "unknown spec: {name} — /spec new {name} <idea> to create it"
        )),
        Some(spec) => {
            let (done, total) = crate::kiro::spec::progress(&spec.tasks);
            Ok(SpecView {
                name: spec.name,
                tasks: spec
                    .tasks
                    .iter()
                    .map(|task| SpecTaskView {
                        label: task.text.clone(),
                        done: task.done,
                    })
                    .collect(),
                done,
                total,
            })
        }
    }
}

/// `spec_run` : exécute les tâches via sous-agents (outbox
/// `SpawnAgent`, progression par event `spec_progress`).
#[tauri::command]
pub fn spec_run(
    backend: State<Mutex<ToolsBackend>>,
    name: String,
    chat: usize,
) -> Result<SpecRunAck, String> {
    let mut backend = lock(&backend)?;
    let (started, label) = backend.spec_run_start_cmd(&name, chat);
    Ok(SpecRunAck { started, label })
}

/// `spec_respond` : répond à l'attente y/n du run en cours.
#[tauri::command]
pub fn spec_respond(backend: State<Mutex<ToolsBackend>>, text: String) -> Result<(), String> {
    let mut backend = lock(&backend)?;
    backend.spec_respond_cmd(&text);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend() -> (ToolsBackend, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("bt-cmd-spec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        (ToolsBackend::new_in(&dir), dir)
    }

    #[test]
    fn show_inconnue_et_run_sans_taches() {
        let (mut backend, dir) = backend();
        backend.note_connected(true);
        assert!(backend
            .specs
            .run_start("zzz", 0)
            .iter()
            .any(|o| matches!(o, crate::spec_runner::SpecOut::ChatLine { text, .. } if text.contains("unknown spec"))));
        let spec = dir.join(".kiro/specs/vide");
        std::fs::create_dir_all(&spec).unwrap();
        std::fs::write(spec.join("requirements.md"), "req").unwrap();
        let (started, label) = backend.spec_run_start_cmd("vide", 0);
        assert!(!started && label.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
