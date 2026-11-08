//! Commandes §6 (critique) : `critic_set` (Kiro sort/rentre) +
//! `critic_smack` (coup centré, rend la réplique).
//!
//! Les critiques partent à chaque envoi (`note_user_send`, câblé par
//! l'intégrateur) et arrivent par event `critic` (tick).

use std::sync::Mutex;

use tauri::State;

use crate::state::{lock, ToolsBackend};
use crate::views::{EnabledView, TextView};

/// `critic_set` : `/critic on|off` (éteint, Kiro rentre).
#[tauri::command]
pub fn critic_set(
    backend: State<Mutex<ToolsBackend>>,
    enabled: bool,
) -> Result<EnabledView, String> {
    let mut backend = lock(&backend)?;
    backend.phantom.set_on(enabled);
    Ok(EnabledView { enabled })
}

/// `critic_smack` : frappe Kiro au centre (clic frontend), rend sa
/// réplique (`OUCH`/`KO`, jamais vide quand il est sorti).
#[tauri::command]
pub fn critic_smack(backend: State<Mutex<ToolsBackend>>) -> Result<TextView, String> {
    let mut backend = lock(&backend)?;
    let text = backend
        .phantom
        .smack_center()
        .unwrap_or_else(|| String::from("Kiro is resting. (/critic on to wake him)"));
    Ok(TextView { text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smack_rend_une_replique_puis_ko() {
        let dir = std::env::temp_dir().join(format!("bt-cmd-critic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut backend = ToolsBackend::new_in(&dir);
        backend.phantom.tick();
        let first = backend.phantom.smack_center().expect("réplique");
        assert!(!first.is_empty());
        backend.phantom.smack_center();
        let ko = backend.phantom.smack_center().expect("K.O.");
        assert!(!ko.is_empty());
        backend.phantom.set_on(false);
        assert_eq!(backend.phantom.smack_center(), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
