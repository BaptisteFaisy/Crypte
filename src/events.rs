//! Flux d'événements temps réel (JSONL) : observer une Crypte distante.
//!
//! Avec `--events-log <fichier>` (ou `KH_EVENTS_LOG`), chaque message des
//! chats et chaque spawn/réponse de sous-agent est appendé en JSON (une
//! ligne par événement). Depuis le poste local :
//! `ssh vps tail -F /var/log/crypte/events.jsonl` — quasi temps réel,
//! aucun port exposé. Désactivé par défaut (zéro coût, zéro écriture).
//!
//! Conventions : `ts` = secondes epoch, `chat`/`owner`/`deliver` en
//! 1-based (comme la sidebar), `agent` = id global du sous-agent.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::provider::Completion;

/// Un fait observable de la session (chats + sous-agents).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Started {
        ts: u64,
        version: String,
    },
    ChatCreated {
        ts: u64,
        chat: usize,
        name: String,
    },
    ChatClosed {
        ts: u64,
        chat: usize,
        name: String,
    },
    UserMsg {
        ts: u64,
        chat: usize,
        text: String,
    },
    AssistantMsg {
        ts: u64,
        chat: usize,
        text: String,
        prompt_tokens: u64,
        completion_tokens: u64,
    },
    AssistantError {
        ts: u64,
        chat: usize,
        error: String,
    },
    AgentSpawned {
        ts: u64,
        agent: usize,
        owner: usize,
        deliver: usize,
        prompt: String,
    },
    AgentAsk {
        ts: u64,
        agent: usize,
        owner: usize,
        deliver: usize,
        text: String,
    },
    AgentDone {
        ts: u64,
        agent: usize,
        owner: usize,
        deliver: usize,
        text: String,
        prompt_tokens: u64,
        completion_tokens: u64,
    },
    AgentFailed {
        ts: u64,
        agent: usize,
        owner: usize,
        deliver: usize,
        error: String,
    },
    Compacted {
        ts: u64,
        chat: usize,
        before: usize,
        after: usize,
    },
    WebAnswer {
        ts: u64,
        chat: usize,
        backend: String,
        text: String,
    },
}

/// Secondes epoch (0 si l'horloge est illisible).
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Event {
    /// Démarrage de session (première ligne du flux, délimite les runs).
    pub fn started() -> Self {
        Self::Started {
            ts: now(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Nouveau chat (`idx` 0-based, émis en 1-based).
    pub fn chat_created(idx: usize, name: &str) -> Self {
        Self::ChatCreated {
            ts: now(),
            chat: idx + 1,
            name: name.to_string(),
        }
    }

    /// Chat fermé (`idx` 0-based, émis en 1-based).
    pub fn chat_closed(idx: usize, name: &str) -> Self {
        Self::ChatClosed {
            ts: now(),
            chat: idx + 1,
            name: name.to_string(),
        }
    }

    /// Message user déposé dans le chat (`idx` 0-based).
    pub fn user_msg(idx: usize, text: &str) -> Self {
        Self::UserMsg {
            ts: now(),
            chat: idx + 1,
            text: text.to_string(),
        }
    }

    /// Réponse du chat (`idx` 0-based) + usage tokens.
    pub fn assistant_msg(idx: usize, done: &Completion) -> Self {
        Self::AssistantMsg {
            ts: now(),
            chat: idx + 1,
            text: done.text.clone(),
            prompt_tokens: done.prompt_tokens,
            completion_tokens: done.completion_tokens,
        }
    }

    /// Réponse du chat en erreur (`idx` 0-based).
    pub fn assistant_error(idx: usize, error: &str) -> Self {
        Self::AssistantError {
            ts: now(),
            chat: idx + 1,
            error: error.to_string(),
        }
    }

    /// Sous-agent lancé (`owner`/`deliver` 0-based).
    pub fn agent_spawned(agent: usize, owner: usize, deliver: usize, prompt: &str) -> Self {
        Self::AgentSpawned {
            ts: now(),
            agent,
            owner: owner + 1,
            deliver: deliver + 1,
            prompt: prompt.to_string(),
        }
    }

    /// Appel agent (`/ask`, file, lien auto) : le texte est le dernier
    /// message user de l'échange (déjà poussé avant l'appel).
    pub fn agent_ask(
        owner: usize,
        agent: usize,
        deliver: usize,
        history: &[(String, String)],
    ) -> Self {
        let text = history
            .iter()
            .rev()
            .find(|(role, _)| role == "user")
            .map(|(_, text)| text.clone())
            .unwrap_or_default();
        Self::AgentAsk {
            ts: now(),
            agent,
            owner: owner + 1,
            deliver: deliver + 1,
            text,
        }
    }

    /// Réponse d'agent livrée + usage tokens.
    pub fn agent_done(owner: usize, agent: usize, deliver: usize, done: &Completion) -> Self {
        Self::AgentDone {
            ts: now(),
            agent,
            owner: owner + 1,
            deliver: deliver + 1,
            text: done.text.clone(),
            prompt_tokens: done.prompt_tokens,
            completion_tokens: done.completion_tokens,
        }
    }

    /// Réponse d'agent en erreur.
    pub fn agent_failed(owner: usize, agent: usize, deliver: usize, error: &str) -> Self {
        Self::AgentFailed {
            ts: now(),
            agent,
            owner: owner + 1,
            deliver: deliver + 1,
            error: error.to_string(),
        }
    }

    /// Historique compacté (`before` → `after` messages).
    pub fn compacted(idx: usize, before: usize, after: usize) -> Self {
        Self::Compacted {
            ts: now(),
            chat: idx + 1,
            before,
            after,
        }
    }

    /// Réponse de recherche web (`/web`).
    pub fn web_answer(idx: usize, backend: &str, text: &str) -> Self {
        Self::WebAnswer {
            ts: now(),
            chat: idx + 1,
            backend: backend.to_string(),
            text: text.to_string(),
        }
    }
}

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

    /// Append une ligne JSON (échec silencieux : l'observateur ne doit
    /// jamais casser le TUI). Fichier rouvert à chaque fois (logrotate OK).
    pub fn emit(&self, event: &Event) {
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

    /// Chemin temporaire unique pour ces tests (jamais commité).
    fn tmp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "crypte-events-test-{}-{tag}.jsonl",
            std::process::id()
        ))
    }

    #[test]
    fn chat_closed_emits_snake_case_1based() {
        let v = serde_json::to_value(Event::chat_closed(1, "Chat 2")).unwrap();
        assert_eq!(v["type"], "chat_closed");
        assert_eq!(v["chat"], 2);
        assert_eq!(v["name"], "Chat 2");
    }

    #[test]
    fn emit_appends_valid_jsonl() {
        let path = tmp_path("append");
        let _ = std::fs::remove_file(&path);
        let log = EventLog::new(Some(path.clone()));
        log.emit(&Event::started());
        log.emit(&Event::user_msg(0, "hello"));
        log.emit(&Event::agent_ask(
            0,
            7,
            1,
            &[("user".to_string(), "dig".to_string())],
        ));
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["type"], "started");
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["type"], "user_msg");
        assert_eq!(second["chat"], 1);
        assert_eq!(second["text"], "hello");
        let third: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(third["type"], "agent_ask");
        assert_eq!(third["agent"], 7);
        assert_eq!(third["text"], "dig");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn disabled_log_writes_nothing() {
        let path = tmp_path("disabled");
        let _ = std::fs::remove_file(&path);
        EventLog::disabled().emit(&Event::started());
        assert!(!path.exists());
    }
}
