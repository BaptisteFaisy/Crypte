//! Chat persistence: autosave + restore at startup.
//!
//! JSON in `$HOME/.config/kiro-horror-tui/chats.json` (same folder as
//! `permissions`): name, displayed messages, provider history, model,
//! agents.md pin flag, token ledger. Atomic write (tmp + rename).
//! Missing file → fresh single chat ; unreadable or unknown version →
//! fresh start, the file preserved as `chats.json.corrupt`.
//!
//! `impl App` deported here (`enable_history`, autosave): reads/writes
//! state via [`App`]'s `pub(crate)` access. Only `main` enables it —
//! tests, `--smoke` and `--demo` never touch the disk.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::App;
use crate::chat::Message;
use crate::sessions::{ChatSession, Sessions};
use crate::tokens::TokenLedger;

/// State file, relative to `$HOME`.
const STATE_PATH: &str = ".config/kiro-horror-tui/chats.json";
/// Backup suffix when the file is unreadable.
const CORRUPT_SUFFIX: &str = ".corrupt";
/// Format version (unknown → treated as unreadable).
const STATE_VERSION: u32 = 1;
/// Ticks (100 ms) between autosaves (~5 s).
const AUTOSAVE_TICKS: u64 = 50;

/// One serialized chat: display + provider context + model + tokens.
#[derive(Serialize, Deserialize)]
struct SavedChat {
    name: String,
    display: Vec<Message>,
    history: Vec<(String, String)>,
    model: Option<String>,
    pinned: bool,
    ledger: TokenLedger,
}

/// Serialized state: chats + current position.
#[derive(Serialize, Deserialize)]
struct SavedState {
    version: u32,
    current: usize,
    chats: Vec<SavedChat>,
}

/// Load outcome: no file, unreadable (backed up), or restored sessions.
pub enum Load {
    Missing,
    Corrupt,
    Restored(Sessions),
}

/// State file path (`None` when `HOME` is missing).
pub fn path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| Path::new(&home).join(STATE_PATH))
}

/// Saves `sessions` to `path` (creates the parent folder, atomic rename).
pub fn save_to(path: &Path, sessions: &Sessions) -> std::io::Result<()> {
    let chats = sessions
        .iter()
        .map(|c| SavedChat {
            name: c.name.clone(),
            display: c.chat.messages().to_vec(),
            history: c.history.clone(),
            model: c.model.clone(),
            pinned: c.pinned_system,
            ledger: c.ledger.clone(),
        })
        .collect::<Vec<_>>();
    let state = SavedState {
        version: STATE_VERSION,
        current: sessions.index(),
        chats,
    };
    let json = serde_json::to_string(&state)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// Loads `path`: `Missing` when absent, `Corrupt` (backed up, previous
/// backup overwritten) when unreadable or unknown version, else restored.
pub fn load_from(path: &Path) -> Load {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Load::Missing;
    };
    let state: SavedState = match serde_json::from_str::<SavedState>(&raw) {
        Ok(state) if state.version == STATE_VERSION && !state.chats.is_empty() => state,
        _ => {
            let _ = std::fs::write(backup_path(path), raw);
            return Load::Corrupt;
        }
    };
    Load::Restored(into_sessions(state))
}

/// Backup path for an unreadable state file (`<path>.corrupt`).
fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(CORRUPT_SUFFIX);
    PathBuf::from(name)
}

/// Rebuilds live sessions (display re-pushed, so capped at 200).
fn into_sessions(state: SavedState) -> Sessions {
    let chats = state.chats.into_iter().map(from_saved).collect();
    Sessions::from_chats(chats, state.current)
}

/// Rebuilds one chat: no flight in progress, no subagents (ephemeral —
/// their delivered replies stay visible in the display).
fn from_saved(saved: SavedChat) -> ChatSession {
    let mut chat = ChatSession::new(saved.name);
    for msg in saved.display {
        chat.chat.push(msg.role, msg.text);
    }
    chat.history = saved.history;
    chat.model = saved.model;
    chat.ledger = saved.ledger;
    chat.pinned_system = saved.pinned;
    chat
}

/// Re-pins agents.md everywhere: replaces a stale pin, inserts the
/// missing one at the head (`restart to reload` holds with restore).
pub fn repin(sessions: &mut Sessions, agents_md: &str) {
    let pinned = (String::from("system"), agents_md.to_string());
    for idx in 0..sessions.count() {
        let Some(chat) = sessions.get_mut(idx) else {
            continue;
        };
        let stale = chat.pinned_system
            && chat
                .history
                .first()
                .is_some_and(|(role, _)| role == "system");
        if stale {
            chat.history[0] = pinned.clone();
        } else if !chat.pinned_system {
            chat.history.insert(0, pinned.clone());
            chat.pinned_system = true;
        }
    }
}

impl App {
    /// Enables persistence: restores chats (silent), re-pins agents.md
    /// when loaded. Called by `main` only — never tests/`--smoke`/`--demo`.
    pub(crate) fn enable_history(&mut self) {
        if let Some(path) = path() {
            self.enable_history_at(&path);
        }
    }

    /// Same, at an explicit path (tests).
    pub(crate) fn enable_history_at(&mut self, path: &Path) {
        match load_from(path) {
            Load::Missing => {}
            Load::Corrupt => self.push_system(String::from(
                "chats.json unreadable, kept as chats.json.corrupt — fresh start",
            )),
            Load::Restored(sessions) => {
                self.sessions = sessions;
                if let Some(md) = self.agents_md.clone() {
                    repin(&mut self.sessions, &md);
                }
            }
        }
        self.chats_path = Some(path.to_path_buf());
    }

    /// Autosaves when `tick` hits the ~5 s mark (silent, no path = off).
    pub(crate) fn autosave_if_due(&self, tick: u64) {
        if tick % AUTOSAVE_TICKS == 0 {
            self.save_now();
        }
    }

    /// Immediate save (exit path) ; silent, no-op without a state path.
    pub(crate) fn save_now(&self) {
        if let Some(path) = &self.chats_path {
            let _ = save_to(path, &self.sessions);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Role;
    use crate::provider::ProviderConfig;

    /// Unique tmp dir per test (no `tempfile` dep).
    fn tmp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kh-persist-{}-{name}", std::process::id()))
    }

    /// Test app (fake key, no network in these tests).
    fn test_app() -> App {
        let cfg = ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        App::from_config(cfg).unwrap()
    }

    /// Two garnished chats: names, display, history, model, ledger, pin.
    fn garnished() -> Sessions {
        let mut s = Sessions::new();
        let first = s.current_mut();
        first.name = String::from("tombe");
        first.chat.push(Role::User, "qui va là ?");
        first.chat.push(Role::Assistant, "un revenant");
        first
            .history
            .push((String::from("system"), String::from("pin")));
        first
            .history
            .push((String::from("user"), String::from("qui va là ?")));
        first.pinned_system = true;
        first.model = Some(String::from("spectre-1"));
        first.ledger.record(10, 20, 1.0);
        s.add();
        s.current_mut().name = String::from("caveau");
        s.current_mut().chat.push(Role::System, "murmure");
        s.select(1);
        s
    }

    #[test]
    fn roundtrip_restores_chats_current_and_tokens() {
        let dir = tmp_dir("roundtrip");
        let path = dir.join("chats.json");
        save_to(&path, &garnished()).unwrap();
        let Load::Restored(after) = load_from(&path) else {
            panic!("should restore");
        };
        assert_eq!(after.count(), 2);
        assert_eq!(after.index(), 1);
        let first = after.get(0).unwrap();
        assert_eq!(first.name, "tombe");
        assert_eq!(first.chat.messages().len(), 2);
        assert_eq!(first.chat.messages()[0].role, Role::User);
        assert_eq!(first.chat.messages()[1].text, "un revenant");
        assert_eq!(first.history.len(), 2);
        assert!(first.pinned_system);
        assert_eq!(first.model.as_deref(), Some("spectre-1"));
        assert_eq!(first.ledger.total(), 30);
        assert!(!first.sending);
        assert!(first.agents.is_empty());
        assert_eq!(after.get(1).unwrap().name, "caveau");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_is_missing_without_backup() {
        let dir = tmp_dir("missing");
        let path = dir.join("chats.json");
        assert!(matches!(load_from(&path), Load::Missing));
        assert!(!dir.join("chats.json.corrupt").exists());
    }

    #[test]
    fn unreadable_is_corrupt_and_preserved() {
        let dir = tmp_dir("corrupt");
        let path = dir.join("chats.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "{ pas du json").unwrap();
        assert!(matches!(load_from(&path), Load::Corrupt));
        assert_eq!(
            std::fs::read_to_string(dir.join("chats.json.corrupt")).unwrap(),
            "{ pas du json"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_version_is_corrupt() {
        let dir = tmp_dir("version");
        let path = dir.join("chats.json");
        save_to(&path, &garnished()).unwrap();
        let raw =
            std::fs::read_to_string(&path)
                .unwrap()
                .replacen("\"version\":1", "\"version\":999", 1);
        std::fs::write(&path, raw).unwrap();
        assert!(matches!(load_from(&path), Load::Corrupt));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn restore_is_capped_at_200_messages() {
        let dir = tmp_dir("cap");
        let path = dir.join("chats.json");
        let display = (0..250)
            .map(|i| Message {
                role: Role::User,
                text: format!("m{i}"),
            })
            .collect();
        let state = SavedState {
            version: STATE_VERSION,
            current: 0,
            chats: vec![SavedChat {
                name: String::from("foule"),
                display,
                history: vec![],
                model: None,
                pinned: false,
                ledger: TokenLedger::new(),
            }],
        };
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
        let Load::Restored(after) = load_from(&path) else {
            panic!("should restore");
        };
        let msgs = after.get(0).unwrap().chat.messages();
        assert_eq!(msgs.len(), 200);
        assert_eq!(msgs[0].text, "m50");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn repin_replaces_or_inserts_at_head() {
        let mut s = garnished();
        repin(&mut s, "nouveau pin");
        let first = s.get(0).unwrap();
        assert_eq!(
            first.history[0],
            (String::from("system"), String::from("nouveau pin"))
        );
        let second = s.get(1).unwrap();
        assert!(second.pinned_system);
        assert_eq!(second.history[0].0, "system");
    }

    #[test]
    fn enable_history_at_restores_and_arms() {
        let dir = tmp_dir("enable");
        let path = dir.join("chats.json");
        save_to(&path, &garnished()).unwrap();
        let mut app = test_app();
        assert!(app.chats_path.is_none());
        app.enable_history_at(&path);
        assert_eq!(app.chats_path.as_deref(), Some(path.as_path()));
        assert_eq!(app.sessions.count(), 2);
        assert_eq!(app.sessions.current().name, "caveau");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn autosave_writes_on_marks_only() {
        let dir = tmp_dir("autosave");
        let path = dir.join("chats.json");
        let mut app = test_app();
        app.enable_history_at(&path);
        app.autosave_if_due(1);
        assert!(!path.exists());
        app.autosave_if_due(50);
        assert!(path.exists());
        assert!(matches!(load_from(&path), Load::Restored(_)));
        std::fs::remove_dir_all(&dir).ok();
    }
}
