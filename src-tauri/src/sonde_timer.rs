//! Sonde mbox sur timer → events `crypte-event`.
//!
//! Remplace `tick()` + `sonde::tour` (sonde.rs:57, throttle 15 s) et
//! `eveil` inotify (NON porté, cf. PLAN §4 : inotify → poll `sonde`).
//! L'intégrateur arme un timer Tokio à [`SONDE_INTERVAL_SECS`] qui appelle
//! [`SondePoll::poll`] puis émet chaque valeur sur `crypte-event`.
//!
//! Partage des eaux avec backend-core : la sonde ne livre QUE les events
//! `type == "mailbox"` (inbox/bus/ordres/acks/hb/alerte, 64 max/tour,
//! 300 ch, cf. sonde.rs) ; les events chats/agents restent au pont drain.

use std::path::Path;

use crate::mbox::format::BcastDir;
use crate::pump::harvest;

/// Cadence du timer (miroir sonde.rs `INTERVALLE = 15`, `NOTIFIER_INTERVAL`).
pub const SONDE_INTERVAL_SECS: u64 = 15;

/// Curseur de moisson sur le journal JSONL partagé (voir `pump::harvest`).
pub struct SondePoll {
    cursor: u64,
}

impl SondePoll {
    pub fn new() -> Self {
        Self { cursor: 0 }
    }

    /// Un tour `sonde::scrute` + moisson des events `mailbox` nouveaux.
    /// Premier appel = amorçage offsets (zéro event, cf. sonde.rs:96-100).
    pub fn poll(&mut self, dir: &BcastDir, journal: &Path) -> Vec<serde_json::Value> {
        let log = crate::events::EventLog::new(Some(journal.to_path_buf()));
        crate::mbox::sonde::scrute(dir, &log);
        let (events, cursor) = harvest(journal, self.cursor);
        self.cursor = cursor;
        events
            .into_iter()
            .filter(|v| v.get("type").and_then(|t| t.as_str()) == Some("mailbox"))
            .collect()
    }
}

impl Default for SondePoll {
    fn default() -> Self {
        Self::new()
    }
}

/// Arme le timer sonde (production : appeler dans `setup()`, Tauri v2
/// `Emitter::emit` sur `crypte-event`, payload = `Event::mailbox` sérialisé
/// = `CrypteEvent::mailbox`, contracts.ts).
pub async fn spawn_sonde_timer(handle: tauri::AppHandle, journal: std::path::PathBuf) {
    use tauri::Emitter;
    let mut sonde = SondePoll::new();
    let dir = BcastDir::defaut();
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(SONDE_INTERVAL_SECS));
    loop {
        tick.tick().await;
        for event in sonde.poll(&dir, &journal) {
            let _ = handle.emit("crypte-event", &event);
        }
    }
}
