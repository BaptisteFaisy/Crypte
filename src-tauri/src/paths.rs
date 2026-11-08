//! Chemins disque de l'app Tauri + migration depuis le TUI legacy.
//!
//! Nouveaux chemins (app-id `com.crypte.app`) :
//! - config : `$XDG_CONFIG_HOME/com.crypte.app` (défaut `~/.config/…`)
//! - data : `$XDG_DATA_HOME/com.crypte.app` (défaut `~/.local/share/…`)
//!
//! Migration auto (copie une fois, documentée) depuis le legacy
//! `kiro-horror-tui` : [`migrate`] copie chaque fichier legacy vers sa
//! cible quand la cible manque (jamais d'écrasement). Fichiers migrés :
//! config `chats.json`, `permissions`, `permissions-rules`, `vps.toml` ;
//! data `providers.enc`, `master.key`, `almanac.json`, `checkpoints/`.
//! Ne migrent PAS (partagés) : le broadcast mbox (`BCAST_DIR`),
//! `~/.config/muse/auth.json`, `~/.kiro/`.

use std::path::{Path, PathBuf};

/// App-id Tauri (dossier config/data).
pub const APP_ID: &str = "com.crypte.app";

/// Dossier du TUI legacy (migration seule).
const LEGACY_ID: &str = "kiro-horror-tui";

/// Fichiers config migrés (legacy → cible, même nom).
const MIGRATE_CONFIG: [&str; 4] = ["chats.json", "permissions", "permissions-rules", "vps.toml"];

/// Fichiers data migrés (legacy → cible, même nom ; `checkpoints/` = dossier).
const MIGRATE_DATA: [&str; 4] = ["providers.enc", "master.key", "almanac.json", "checkpoints"];

/// Dossier config de l'app (`None` sans `HOME` ni `XDG_CONFIG_HOME`).
pub fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".config")))?;
    Some(base.join(APP_ID))
}

/// Dossier data de l'app (`None` sans `HOME` ni `XDG_DATA_HOME`).
pub fn data_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".local/share")))?;
    Some(base.join(APP_ID))
}

/// Dossier config legacy (`None` sans `HOME` ni `XDG_CONFIG_HOME`).
pub fn legacy_config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".config")))?;
    Some(base.join(LEGACY_ID))
}

/// Dossier data legacy (`None` sans `HOME` ni `XDG_DATA_HOME`).
pub fn legacy_data_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".local/share")))?;
    Some(base.join(LEGACY_ID))
}

/// Migre le legacy vers les dossiers de l'app (copie une fois : chaque
/// cible existante est gardée). Rend les chemins copiés (log boot).
/// Silencieux et best-effort (un disque en erreur n'empêche pas le boot).
pub fn migrate() -> Vec<PathBuf> {
    let mut copied = Vec::new();
    if let (Some(new), Some(old)) = (config_dir(), legacy_config_dir()) {
        migrate_list(&new, &old, &MIGRATE_CONFIG, &mut copied);
    }
    if let (Some(new), Some(old)) = (data_dir(), legacy_data_dir()) {
        migrate_list(&new, &old, &MIGRATE_DATA, &mut copied);
    }
    copied
}

/// Copie les entrées manquantes de `old` vers `new` (dossiers récursifs).
fn migrate_list(new: &Path, old: &Path, names: &[&str], copied: &mut Vec<PathBuf>) {
    for name in names {
        let target = new.join(name);
        if target.exists() {
            continue;
        }
        let source = old.join(name);
        if matches!(copy_entry(&source, &target), Ok(true)) {
            copied.push(target);
        }
    }
}

/// Copie un fichier ou dossier (crée les parents ; `Ok(false)` = absent).
fn copy_entry(source: &Path, target: &Path) -> std::io::Result<bool> {
    if source.is_dir() {
        std::fs::create_dir_all(target)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            let dest = target.join(entry.file_name());
            if dest.exists() {
                continue;
            }
            copy_entry(&entry.path(), &dest)?;
        }
        return Ok(true);
    }
    if !source.is_file() {
        return Ok(false);
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(source, target)?;
    Ok(true)
}

/// `HOME` non blanc, le cas échéant.
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|h| !h.as_os_str().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Racines temporaires (jamais le vrai `HOME`).
    fn roots(tag: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("crypte-paths-{}-{tag}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        let dirs = (
            base.join("cfg-new"),
            base.join("data-new"),
            base.join("cfg-old"),
            base.join("data-old"),
        );
        std::fs::create_dir_all(&dirs.2).unwrap();
        std::fs::create_dir_all(&dirs.3).unwrap();
        (dirs.0, dirs.1, dirs.2, dirs.3, base)
    }

    #[test]
    fn migre_sans_ecraser() {
        let (cfg_new, data_new, cfg_old, data_old, base) = roots("mig");
        std::fs::write(cfg_old.join("chats.json"), "{}").unwrap();
        std::fs::create_dir_all(&cfg_new).unwrap();
        std::fs::write(cfg_new.join("vps.toml"), "gardé").unwrap();
        std::fs::write(cfg_old.join("vps.toml"), "legacy").unwrap();
        std::fs::create_dir_all(data_old.join("checkpoints/snap")).unwrap();
        let mut copied = Vec::new();
        migrate_list(&cfg_new, &cfg_old, &MIGRATE_CONFIG, &mut copied);
        migrate_list(&data_new, &data_old, &MIGRATE_DATA, &mut copied);
        assert_eq!(std::fs::read(cfg_new.join("chats.json")).unwrap(), b"{}");
        assert_eq!(
            std::fs::read(cfg_new.join("vps.toml")).unwrap(),
            b"gard\xc3\xa9"
        );
        assert!(data_new.join("checkpoints/snap").is_dir());
        assert!(copied.iter().any(|p| p.ends_with("chats.json")));
        assert!(!copied.iter().any(|p| p.ends_with("vps.toml")));
        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn copie_absente_sans_bruit() {
        let (cfg_new, _, _, _, base) = roots("abs");
        let mut copied = Vec::new();
        migrate_list(&cfg_new, &base.join("vide"), &MIGRATE_CONFIG, &mut copied);
        assert!(copied.is_empty());
        std::fs::remove_dir_all(base).ok();
    }
}
