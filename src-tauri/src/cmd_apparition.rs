//! Commandes §7 (ambiance) : `apparition_state` + `spook`.
//!
//! Priorité TUI : envoi (chaudron) → agents (araignée) → présage →
//! rotation 20 s ; jump scare 2 s (F5). `spook` secoue aussi le hub,
//! Kiro et le jeu ouvert (éclair, comme le TUI).

use std::sync::Mutex;

use tauri::State;

use crate::bestiary::Creature;
use crate::state::{lock, ToolsBackend};
use crate::views::{ApparitionView, CreatureView};

/// `apparition_state` : créature du panneau + présage éventuel.
#[tauri::command]
pub fn apparition_state(backend: State<Mutex<ToolsBackend>>) -> Result<ApparitionView, String> {
    let backend = lock(&backend)?;
    Ok(apparition_view(&backend))
}

/// `spook` : jump scare (F5) + état du panneau.
#[tauri::command]
pub fn spook(backend: State<Mutex<ToolsBackend>>) -> Result<ApparitionView, String> {
    let mut backend = lock(&backend)?;
    backend.spook();
    if backend.haunt_open {
        backend.haunt.thunder();
    }
    Ok(apparition_view(&backend))
}

/// Vue contrat depuis l'état (réutilisée par la boucle `apparition`).
pub fn apparition_view(backend: &ToolsBackend) -> ApparitionView {
    let creature = backend
        .apparition
        .current(backend.chat_sending, backend.chat_agents);
    let omen = if backend.apparition.scaring() {
        Some(String::from("scare"))
    } else {
        backend.last_omen.as_ref().map(|(kind, _)| kind.clone())
    };
    ApparitionView {
        creature: creature_view(creature),
        omen,
    }
}

/// Créature pour le contrat (grille 32×32 incluse).
pub fn creature_view(creature: &Creature) -> CreatureView {
    CreatureView {
        slug: creature.slug.to_string(),
        name: creature.name.to_string(),
        lore: creature.lore.to_string(),
        grid: creature.grid(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priorites_envoi_agents_presage() {
        let dir = std::env::temp_dir().join(format!("bt-cmd-app-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut backend = ToolsBackend::new_in(&dir);
        backend.note_chat_activity(true, 0);
        let sending = apparition_view(&backend);
        assert_eq!(sending.creature.slug, "chaudron");
        assert_eq!(sending.omen, None);
        backend.note_chat_activity(false, 2);
        assert_eq!(apparition_view(&backend).creature.slug, "araignee");
        backend.note_omen("error");
        backend.note_chat_activity(false, 0);
        let omen = apparition_view(&backend);
        assert_eq!(omen.creature.slug, "main-tombe");
        assert_eq!(omen.omen.as_deref(), Some("error"));
        backend.spook();
        let scare = apparition_view(&backend);
        assert_eq!(scare.omen.as_deref(), Some("scare"));
        assert_eq!(scare.creature.grid.len(), 32);
        assert_eq!(scare.creature.grid[0].chars().count(), 32);
        std::fs::remove_dir_all(&dir).ok();
    }
}
