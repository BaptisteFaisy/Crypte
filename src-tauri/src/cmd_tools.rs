//! Commandes §5 : outils (`tools_*`, `tool_decide`) + permissions.
//!
//! `tools_set` n'interrompt pas un run (comme le TUI) ; `tool_decide`
//! résout l'unique confirmation en attente (pas d'id, comme le TUI).
//! Persistance best-effort silencieuse (pas de champ contrat).

use std::sync::Mutex;

use tauri::State;

use crate::permissions::{self, Mode, Rule, Rules};
use crate::state::{lock, ToolsBackend};
use crate::views::{EnabledView, PermRuleView, PermissionsView, ToolView};

/// `tools_status` : interrupteur d'envoi outillé.
#[tauri::command]
pub fn tools_status(backend: State<Mutex<ToolsBackend>>) -> Result<EnabledView, String> {
    let backend = lock(&backend)?;
    Ok(EnabledView {
        enabled: backend.tools_on,
    })
}

/// `tools_set` : change l'interrupteur (run en cours non interrompu).
#[tauri::command]
pub fn tools_set(
    backend: State<Mutex<ToolsBackend>>,
    enabled: bool,
) -> Result<EnabledView, String> {
    let mut backend = lock(&backend)?;
    backend.tools_on = enabled;
    Ok(EnabledView { enabled })
}

/// `tools_list` : les 14 schémas natifs + leur classe d'effet.
#[tauri::command]
pub fn tools_list(backend: State<Mutex<ToolsBackend>>) -> Result<Vec<ToolView>, String> {
    let _backend = lock(&backend)?;
    Ok(crate::tools::defs()
        .iter()
        .map(|def| ToolView {
            name: def.function.name.clone(),
            description: def.function.description.clone(),
            class: match crate::tools::class(&def.function.name) {
                crate::tools::Class::Read => "read",
                crate::tools::Class::Change => "change",
            },
        })
        .collect())
}

/// `tool_decide` : résout y/n (l'intégrateur draine l'outbox après).
#[tauri::command]
pub fn tool_decide(backend: State<Mutex<ToolsBackend>>, allow: bool) -> Result<(), String> {
    let mut backend = lock(&backend)?;
    backend.tool_decide_cmd(allow);
    Ok(())
}

/// `permissions_get` : mode + règles fines.
#[tauri::command]
pub fn permissions_get(backend: State<Mutex<ToolsBackend>>) -> Result<PermissionsView, String> {
    let backend = lock(&backend)?;
    Ok(permissions_view(&backend))
}

/// `permissions_set_mode` : `yolo` ou `auto` (persisté).
#[tauri::command]
pub fn permissions_set_mode(
    backend: State<Mutex<ToolsBackend>>,
    mode: String,
) -> Result<PermissionsView, String> {
    let mut backend = lock(&backend)?;
    let Some(mode) = Mode::parse(&mode) else {
        return Err(format!("invalid mode: {mode} — values: yolo, auto"));
    };
    backend.set_mode(mode);
    Ok(permissions_view(&backend))
}

/// `permissions_set_rule` : pose une règle (`null` = retire, persisté).
#[tauri::command]
pub fn permissions_set_rule(
    backend: State<Mutex<ToolsBackend>>,
    pattern: String,
    rule: Option<String>,
) -> Result<PermissionsView, String> {
    let mut backend = lock(&backend)?;
    match rule {
        None => backend.rules.remove(&pattern),
        Some(word) => {
            let Some(parsed) = Rule::parse(&word) else {
                return Err(format!("invalid rule: {word} — values: allow, ask, deny"));
            };
            backend.rules.set(&pattern, parsed);
        }
    }
    persist_rules(&backend);
    Ok(permissions_view(&backend))
}

/// `permissions_reset` : vide les règles (persisté).
#[tauri::command]
pub fn permissions_reset(backend: State<Mutex<ToolsBackend>>) -> Result<PermissionsView, String> {
    let mut backend = lock(&backend)?;
    backend.rules = Rules::default();
    persist_rules(&backend);
    Ok(permissions_view(&backend))
}

/// Vue contrat depuis l'état (réutilisée par la préview).
pub fn permissions_view(backend: &ToolsBackend) -> PermissionsView {
    PermissionsView {
        mode: backend.permissions.label().to_string(),
        rules: backend
            .rules
            .entries
            .iter()
            .map(|(pattern, rule)| PermRuleView {
                pattern: pattern.clone(),
                rule: rule.label().to_string(),
            })
            .collect(),
    }
}

/// Persiste les règles (best-effort, comme le TUI sans sa note).
fn persist_rules(backend: &ToolsBackend) {
    if let Some(path) = &backend.rules_path {
        let _ = permissions::save_rules_to(path, &backend.rules);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Backend hermétique (ni HOME ni checkpoint réels touchés).
    fn backend() -> (ToolsBackend, std::path::PathBuf) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("bt-cmd-tools-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        (ToolsBackend::new_in(&dir), dir)
    }

    #[test]
    fn liste_quatorze_classes() {
        let (backend, dir) = backend();
        assert!(backend.tools_on);
        let defs = crate::tools::defs();
        assert_eq!(defs.len(), 14);
        let reads = defs
            .iter()
            .filter(|d| crate::tools::class(&d.function.name) == crate::tools::Class::Read)
            .count();
        assert_eq!(reads, 5);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn regles_posent_retirent_reset() {
        let (mut backend, dir) = backend();
        backend.rules.set("shell_run", Rule::Deny);
        let view = permissions_view(&backend);
        assert_eq!(view.mode, "auto");
        assert_eq!(view.rules.len(), 1);
        backend.rules.remove("shell_run");
        assert!(permissions_view(&backend).rules.is_empty());
        backend.set_mode(Mode::Yolo);
        assert_eq!(permissions_view(&backend).mode, "yolo");
        assert!(Mode::parse("turbo").is_none());
        assert!(Rule::parse("maybe").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
