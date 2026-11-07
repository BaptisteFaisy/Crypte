//! Mode de permissions des futurs outils (`/permissions yolo|auto`).
//!
//! `yolo` = exécute sans demander, `auto` (défaut) = demande confirmation
//! à chaque modification. Persisté dans
//! `$HOME/.config/kiro-horror-tui/permissions` (un mot, `auto` si absent,
//! illisible ou inconnu) ; relu au démarrage.
//!
//! `impl App` déporté (handler `/permissions`, `cmds.rs` étant au plafond
//! de 500 lignes) : lit/écrit l'état via les accès `pub(crate)` de [`App`].

use std::path::{Path, PathBuf};

use crate::app::App;

/// Fichier d'état, relatif à `$HOME`.
const STATE_PATH: &str = ".config/kiro-horror-tui/permissions";

/// Mode de permissions (`Auto` par défaut : demande à chaque modification).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Auto,
    Yolo,
}

impl Mode {
    /// Parse insensible à la casse et aux espaces (`None` si inconnu).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "yolo" => Some(Self::Yolo),
            _ => None,
        }
    }

    /// Étiquette affichée et persistée (`auto` / `yolo`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Yolo => "yolo",
        }
    }

    /// Comportement en une phrase (affiché par `/permissions`, en anglais).
    pub fn describe(self) -> &'static str {
        match self {
            Self::Auto => "asks confirmation before every change",
            Self::Yolo => "runs everything without asking",
        }
    }
}

/// Valeurs acceptées, pour les messages d'erreur.
pub fn values() -> &'static str {
    "yolo, auto"
}

/// Chemin du fichier d'état (`None` si `HOME` manquant).
pub fn path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| Path::new(&home).join(STATE_PATH))
}

/// Charge le mode persisté (`Auto` si absent, illisible ou inconnu).
pub fn load() -> Mode {
    path().map(|p| load_from(&p)).unwrap_or_default()
}

/// Charge depuis `path` (`Auto` si absent, illisible ou inconnu).
pub fn load_from(path: &Path) -> Mode {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| Mode::parse(&raw))
        .unwrap_or_default()
}

/// Persiste `mode` dans `path` (crée le dossier parent si besoin).
pub fn save_to(path: &Path, mode: Mode) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, format!("{}\n", mode.label()))
}

impl App {
    /// `/permissions [yolo|auto]` : affiche ou change le mode des futurs
    /// outils (`yolo` = sans demander, `auto` = demande à chaque
    /// modification, en anglais) ; le choix est persisté dans le fichier d'état.
    pub(crate) fn cmd_permissions(&mut self, arg: Option<String>) {
        match arg {
            None => self.push_system(permissions_line(self.permissions)),
            Some(v) => match Mode::parse(&v) {
                None => self.push_system(format!("invalid mode: {v} — values: {}", values())),
                Some(mode) => {
                    self.permissions = mode;
                    let mut msg = permissions_line(mode);
                    match &self.permissions_path {
                        Some(p) => {
                            if let Err(err) = save_to(p, mode) {
                                msg.push_str(&format!(" — could not save: {err}"));
                            }
                        }
                        None => msg.push_str(" — not persisted (HOME missing)"),
                    }
                    self.push_system(msg);
                }
            },
        }
    }
}

/// Ligne d'affichage du mode : `permissions: auto (asks …)` (en anglais).
fn permissions_line(mode: Mode) -> String {
    format!("permissions: {} ({})", mode.label(), mode.describe())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kh-perms-{}-{name}", std::process::id()))
    }

    #[test]
    fn parse_accepte_yolo_auto_insensible_casse() {
        assert_eq!(Mode::parse("yolo"), Some(Mode::Yolo));
        assert_eq!(Mode::parse("  AUTO\n"), Some(Mode::Auto));
        assert_eq!(Mode::parse("turbo"), None);
        assert_eq!(Mode::parse(""), None);
        assert_eq!(Mode::default(), Mode::Auto);
        assert_eq!(Mode::Yolo.label(), "yolo");
        assert_eq!(Mode::Auto.label(), "auto");
    }

    #[test]
    fn roundtrip_fichier_et_inconnu_vaut_auto() {
        let dir = tmp_dir("roundtrip");
        let path = dir.join("sous").join("permissions");
        save_to(&path, Mode::Yolo).unwrap();
        assert_eq!(load_from(&path), Mode::Yolo);
        assert_eq!(load_from(&dir.join("absent")), Mode::Auto);
        std::fs::write(&path, "turbo\n").unwrap();
        assert_eq!(load_from(&path), Mode::Auto);
        std::fs::remove_dir_all(&dir).ok();
    }
}
