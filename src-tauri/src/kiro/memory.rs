//! Mémoire (`.kiro/MEMORY.md` projet + `~/.kiro/MEMORY.md` global).
//!
//! Faits durables sur le user et le projet, chargés au pin après agents.md
//! et le steering (ordre : agents.md → steering `always` → mémoire).
//! `/remember <texte>` ajoute une ligne à la mémoire projet (fichier créé
//! si absent) ; la mémoire globale ne s'édite qu'à la main.

use std::path::Path;

/// Mémoire chargée : portée (`project` / `global`) + corps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Memory {
    pub scope: &'static str,
    pub body: String,
}

/// Charge projet + global (chemins injectés ; absents ignorés).
pub fn load_all_in(project: &Path, global: Option<&Path>) -> Vec<Memory> {
    let mut out = Vec::new();
    if let Some(global) = global {
        if let Some(body) = read_non_empty(&global.join("MEMORY.md")) {
            out.push(Memory {
                scope: "global",
                body,
            });
        }
    }
    if let Some(body) = read_non_empty(&project.join(".kiro/MEMORY.md")) {
        out.push(Memory {
            scope: "project",
            body,
        });
    }
    out
}

/// Ajoute `text` à la mémoire projet (dossier + fichier créés si
/// absents), avec snapshot checkpoint du contenu précédent.
pub fn remember(project: &Path, text: &str) -> std::io::Result<()> {
    let path = project.join(".kiro/MEMORY.md");
    crate::kiro::checkpoint::write_tracked(&path, &appended(&path, text))
}

/// `remember` avec dossier checkpoints du backend (`None` = sans
/// snapshot). Ajout Tauri (hermétisme + cohérence du backend).
pub fn remember_via(project: &Path, text: &str, ckpt: Option<&Path>) -> std::io::Result<()> {
    let path = project.join(".kiro/MEMORY.md");
    crate::kiro::checkpoint::write_via(ckpt, &path, &appended(&path, text))
}

/// `remember` avec dossier checkpoints injecté (`None` = sans snapshot).
#[cfg(test)]
pub fn remember_in(project: &Path, text: &str, ckpt: Option<&Path>) -> std::io::Result<()> {
    let path = project.join(".kiro/MEMORY.md");
    let out = appended(&path, text);
    match ckpt {
        Some(dir) => crate::kiro::checkpoint::write_tracked_in(dir, &path, &out),
        None => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, out)
        }
    }
}

/// Contenu après ajout (`\n` final garanti, `text` trimmé).
fn appended(path: &Path, text: &str) -> String {
    let mut out = std::fs::read_to_string(path).unwrap_or_default();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(text.trim());
    out.push('\n');
    out
}

/// Lit un fichier, `None` si illisible ou blanc (évite un system vide).
fn read_non_empty(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("kh-memory-{}-{name}", std::process::id()))
    }

    #[test]
    fn charge_projet_et_global_blancs_ignores() {
        let dir = tmp_dir("load");
        let (global, project) = (dir.join("home/.kiro"), dir.join("proj"));
        std::fs::create_dir_all(&global).unwrap();
        std::fs::create_dir_all(project.join(".kiro")).unwrap();
        std::fs::write(global.join("MEMORY.md"), "fait global").unwrap();
        std::fs::write(project.join(".kiro/MEMORY.md"), "  \n").unwrap();
        let mem = load_all_in(&project, Some(&global));
        assert_eq!(
            mem,
            vec![Memory {
                scope: "global",
                body: String::from("fait global"),
            }]
        );
        assert!(load_all_in(&dir.join("vide"), None).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remember_cree_puis_ajoute_ligne_a_ligne() {
        let dir = tmp_dir("remember");
        let ckpt = dir.join("ckpt");
        std::fs::create_dir_all(&dir).unwrap();
        remember_in(&dir, "premier fait", Some(&ckpt)).unwrap();
        remember_in(&dir, "  second fait  ", Some(&ckpt)).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(".kiro/MEMORY.md")).unwrap(),
            "premier fait\nsecond fait\n"
        );
        // Ajout snapshoté : le rewind retrouve la version précédente.
        assert_eq!(crate::kiro::checkpoint::list_in(&ckpt).len(), 1);
        assert!(crate::kiro::checkpoint::rewind_in(&ckpt, "1").is_ok());
        assert_eq!(
            std::fs::read_to_string(dir.join(".kiro/MEMORY.md")).unwrap(),
            "premier fait\n"
        );
        // Fichier existant sans `\n` final : recollé proprement.
        std::fs::write(dir.join(".kiro/MEMORY.md"), "brut").unwrap();
        remember_in(&dir, "suite", None).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(".kiro/MEMORY.md")).unwrap(),
            "brut\nsuite\n"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
