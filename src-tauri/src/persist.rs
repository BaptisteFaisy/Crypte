//! Persistance des chats : autosave + restauration au démarrage.
//!
//! Port de `persist.rs` du TUI : JSON `chats.json` (nom, messages
//! affichés, historique provider, modèle, pin agents.md, registre),
//! écriture atomique (tmp + rename). Fichier manquant → chat neuf ;
//! illisible ou version inconnue → démarrage neuf, fichier préservé en
//! `chats.json.corrupt`. Seul le binaire active (`enable_history`),
//! jamais les tests.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::chat_state::Message;
use crate::sessions::{ChatSession, Sessions};
use crate::tokens::TokenLedger;

/// Nom du fichier d'état (dossier : `crate::paths::config_dir`).
const STATE_FILE: &str = "chats.json";
/// Suffixe de sauvegarde quand le fichier est illisible.
const CORRUPT_SUFFIX: &str = ".corrupt";
/// Version du format (inconnue → traitée comme illisible).
const STATE_VERSION: u32 = 1;
/// Ticks (100 ms) entre autosaves (~5 s).
pub const AUTOSAVE_TICKS: u64 = 50;

/// Un chat sérialisé : affichage + contexte provider + modèle + tokens.
#[derive(Serialize, Deserialize)]
struct SavedChat {
    name: String,
    display: Vec<Message>,
    history: Vec<(String, String)>,
    model: Option<String>,
    pinned: bool,
    ledger: TokenLedger,
}

/// État sérialisé : chats + position courante.
#[derive(Serialize, Deserialize)]
struct SavedState {
    version: u32,
    current: usize,
    chats: Vec<SavedChat>,
}

/// Sort d'un chargement : pas de fichier, illisible (sauvegardé), ou restauré.
pub enum Load {
    Missing,
    Corrupt,
    Restored(Sessions),
}

/// Chemin du fichier d'état (`None` quand `HOME` manque).
pub fn path() -> Option<PathBuf> {
    crate::paths::config_dir().map(|dir| dir.join(STATE_FILE))
}

/// Sauvegarde `sessions` à `path` (crée le dossier parent, rename atomique).
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

/// Charge `path` : `Missing` si absent, `Corrupt` (sauvegardé, l'ancienne
/// sauvegarde écrasée) si illisible ou version inconnue, sinon restauré
/// (chats vides = tout fermé : « Chat 1 » neuf, silencieux).
pub fn load_from(path: &Path) -> Load {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Load::Missing;
    };
    let state: SavedState = match serde_json::from_str::<SavedState>(&raw) {
        Ok(state) if state.version == STATE_VERSION => state,
        _ => {
            let _ = std::fs::write(backup_path(path), raw);
            return Load::Corrupt;
        }
    };
    Load::Restored(into_sessions(state))
}

/// Chemin de sauvegarde d'un fichier d'état illisible (`<path>.corrupt`).
fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(CORRUPT_SUFFIX);
    PathBuf::from(name)
}

/// Reconstruit les sessions live (affichage re-poussé, donc borné à 200).
fn into_sessions(state: SavedState) -> Sessions {
    let chats = state.chats.into_iter().map(from_saved).collect();
    Sessions::from_chats(chats, state.current)
}

/// Reconstruit un chat : pas de vol en cours, pas de sous-agents (éphémères —
/// leurs réponses livrées restent visibles dans l'affichage).
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

/// Re-pinne agents.md partout : remplace un pin périmé, insère le
/// manquant en tête.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_state::Role;

    /// Dossier tmp unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kh-persist-{}-{name}", std::process::id()))
    }

    /// Deux chats garnis : noms, affichage, historique, modèle, registre, pin.
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
    fn charge_vide_restaure_un_chat_neuf_sans_erreur() {
        let dir = tmp_dir("vide");
        let path = dir.join("chats.json");
        let mut s = Sessions::new();
        s.close(0); // tout fermé, puis quit
        save_to(&path, &s).unwrap();
        let Load::Restored(restored) = load_from(&path) else {
            panic!("un vide valide doit se restaurer, pas être Corrupt");
        };
        assert_eq!(restored.count(), 1);
        assert_eq!(restored.current().name, "Chat 1");
        assert!(!backup_path(&path).exists(), "pas de .corrupt pour un vide");
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
}
