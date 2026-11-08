//! Journal JSONL des événements (`--events-log` / `KH_EVENTS_LOG`).
//!
//! Reprend de `src/events.rs` (TUI) le puits append-only ; l'enum
//! `Event` elle-même vit dans la tranche core (canal `crypte-event`)
//! pour éviter toute duplication — ce module accepte toute valeur
//! sérialisable. Fichier rouvert à chaque ligne (logrotate OK),
//! échec silencieux (l'observateur ne casse jamais l'app).

use std::path::PathBuf;

/// Variable d'environnement de repli (comme le TUI).
pub const ENV_VAR: &str = "KH_EVENTS_LOG";

/// Append-only JSONL (désactivé = `None`, émission gratuite).
#[derive(Debug, Clone, Default)]
pub struct EventLog {
    path: Option<PathBuf>,
}

impl EventLog {
    /// Journal vers `path` (`None` = désactivé).
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    /// Journal désactivé (zéro écriture).
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Résout le chemin : option CLI puis `KH_EVENTS_LOG` (comme `main.rs`).
    pub fn resolve(cli: Option<String>) -> Self {
        let path = cli
            .or_else(|| std::env::var(ENV_VAR).ok())
            .map(PathBuf::from);
        Self::new(path)
    }

    /// Journal actif ?
    pub fn active(&self) -> bool {
        self.path.is_some()
    }

    /// Append une valeur sérialisable (une ligne JSON + `\n`).
    pub fn emit<T: serde::Serialize>(&self, event: &T) {
        let Some(path) = &self.path else { return };
        let Ok(mut line) = serde_json::to_string(event) else {
            return;
        };
        line.push('\n');
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Chemin temporaire unique pour ces tests (jamais commit).
    fn tmp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("bt-events-{}-{tag}.jsonl", std::process::id()))
    }

    #[test]
    fn emit_appends_valid_jsonl() {
        let path = tmp_path("append");
        let _ = std::fs::remove_file(&path);
        let log = EventLog::new(Some(path.clone()));
        assert!(log.active());
        log.emit(&json!({"type": "started", "ts": 1}));
        log.emit(&json!({"type": "user_msg", "chat": 1, "text": "hello"}));
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["type"], "started");
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["chat"], 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn disabled_log_writes_nothing() {
        let log = EventLog::disabled();
        assert!(!log.active());
        log.emit(&json!({"type": "started"}));
    }
}
