//! Journal JSONL des exécutions d'outils (vie privée : nom + durées).
//!
//! Une ligne par appel dans `~/.local/share/kiro-horror-tui/tools.jsonl` :
//! horodatage, chat, outil, succès, millisecondes, tailles (args et sortie
//! ne sont JAMAIS journalisés : ils peuvent contenir des secrets saisis).
//! Captures dans `…/shots/` (best-effort, erreurs ignorées comme l'almanach).

use std::path::PathBuf;

use serde::Serialize;

/// Ligne de journal (tailles seules, jamais le contenu).
#[derive(Debug, Serialize)]
struct Entry {
    ts: String,
    chat: usize,
    tool: String,
    ok: bool,
    ms: u64,
    args_chars: usize,
    out_chars: usize,
}

/// Dossier partagé (`None` si `HOME` manquant, journal désactivé).
pub fn dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".local/share/kiro-horror-tui"))
}

/// Chemin du journal (`None` si `HOME` manquant).
pub fn path() -> Option<PathBuf> {
    dir().map(|d| d.join("tools.jsonl"))
}

/// Chemin frais pour une capture (`None` si le dossier est incréable).
pub fn shot_path(tool: &str) -> Option<PathBuf> {
    let dir = dir()?.join("shots");
    std::fs::create_dir_all(&dir).ok()?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Some(dir.join(format!("{tool}-{}-{nanos}.png", std::process::id())))
}

/// Ajoute une ligne au journal (best-effort : échec silencieux).
pub fn append(chat: usize, tool: &str, ok: bool, ms: u64, args_chars: usize, out_chars: usize) {
    let Some(path) = path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let entry = Entry {
        ts: chrono::Local::now().to_rfc3339(),
        chat,
        tool: tool.to_string(),
        ok,
        ms,
        args_chars,
        out_chars,
    };
    if let Ok(raw) = serde_json::to_string(&entry) {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(file, "{raw}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entree_sans_contenu_sensible() {
        let entry = Entry {
            ts: String::from("t"),
            chat: 1,
            tool: String::from("browser_fill"),
            ok: true,
            ms: 12,
            args_chars: 40,
            out_chars: 3,
        };
        let raw = serde_json::to_string(&entry).unwrap();
        assert!(raw.contains("browser_fill"));
        assert!(!raw.contains("password"));
    }

    #[test]
    fn chemin_capture_avec_pid() {
        let path = shot_path("browser_shot").unwrap();
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("browser_shot-"));
        assert!(name.ends_with(".png"));
        assert!(name.contains(&std::process::id().to_string()));
    }
}
