//! Commande §8 : `hub_snapshot` (esprits + orbes, dalles).
//!
//! Sync sur la dernière vue sessions connue puis snapshot (sans pas
//! de simu : les pas tournent dans `tick_10hz`, l'event `hub_tick`
//! part à ~10 Hz via la boucle de l'intégrateur). Rendu iso côté
//! frontend (canvas/SVG) depuis ces positions.

use std::sync::Mutex;

use tauri::State;

use crate::hub::{Glyph, Hub, Key, Mood};
use crate::state::{lock, ToolsBackend};
use crate::views::{HubSnapshotView, OrbView, SpiritView};

/// `hub_snapshot` : esprits (`chat:<n>` 1-based comme la sidebar) +
/// orbes en vol (positions dalles, `ROOM` = 10).
#[tauri::command]
pub fn hub_snapshot(backend: State<Mutex<ToolsBackend>>) -> Result<HubSnapshotView, String> {
    let mut backend = lock(&backend)?;
    let view = backend.hub_view.clone();
    backend.hub.sync(&view);
    Ok(snapshot_of(&backend.hub))
}

/// Snapshot depuis un hub (réutilisé par la boucle `hub_tick`).
pub fn snapshot_of(hub: &Hub) -> HubSnapshotView {
    let spirits = hub
        .spectres
        .iter()
        .map(|s| SpiritView {
            key: spirit_key(s.key),
            x: s.x,
            y: s.y,
            mood: mood_word(s.mood),
            bubble: hub
                .bubbles
                .iter()
                .find(|b| b.key == s.key)
                .map(|b| glyph_word(b.glyph)),
        })
        .collect();
    let orbs = hub
        .orbs
        .iter()
        .map(|orb| {
            let (to_x, to_y) = hub.pos(orb.to).unwrap_or(orb.from);
            OrbView {
                from_x: orb.from.0,
                from_y: orb.from.1,
                to_x,
                to_y,
                progress: orb.t.clamp(0.0, 1.0),
            }
        })
        .collect();
    HubSnapshotView { spirits, orbs }
}

/// Clé d'esprit : `chat:<n>` (1-based, comme la sidebar) ou `agent:<id>`.
fn spirit_key(key: Key) -> String {
    match key {
        Key::Agent(i) => format!("chat:{}", i + 1),
        Key::Sub(id) => format!("agent:{id}"),
    }
}

/// Humeur en mot contrat.
fn mood_word(mood: Mood) -> String {
    match mood {
        Mood::Idle => String::from("idle"),
        Mood::Waiting => String::from("waiting"),
        Mood::Running => String::from("running"),
        Mood::Done => String::from("done"),
        Mood::Failed => String::from("failed"),
    }
}

/// Glyphe de bulle en mot contrat (jamais d'emoji).
fn glyph_word(glyph: Glyph) -> String {
    match glyph {
        Glyph::Talk => String::from("talk"),
        Glyph::Alert => String::from("alert"),
        Glyph::Ask => String::from("ask"),
        Glyph::Sleep => String::from("sleep"),
        Glyph::Love => String::from("love"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{AgentStatus, AgentSync, ChatSync, Signal, SyncView};

    #[test]
    fn cles_1_based_et_orbe_en_vol() {
        let mut hub = Hub::new();
        hub.sync(&SyncView {
            chats: vec![
                ChatSync {
                    sending: false,
                    agents: vec![AgentSync {
                        id: 1,
                        status: AgentStatus::Running,
                    }],
                },
                ChatSync {
                    sending: true,
                    agents: Vec::new(),
                },
            ],
        });
        hub.signal(Signal::Tell { from: 0, to: 1 });
        let snap = snapshot_of(&hub);
        assert_eq!(snap.spirits.len(), 3);
        assert!(snap
            .spirits
            .iter()
            .any(|s| s.key == "chat:1" && s.mood == "idle"));
        assert!(snap
            .spirits
            .iter()
            .any(|s| s.key == "chat:2" && s.mood == "waiting"));
        assert!(snap.spirits.iter().any(|s| s.key == "agent:1"));
        assert_eq!(snap.orbs.len(), 1);
        assert_eq!(snap.orbs[0].progress, 0.0);
    }
}
