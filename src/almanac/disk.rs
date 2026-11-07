//! Fichier de l'almanach : chemin par défaut, lecture, écriture atomique.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::DayTally;

/// Dossier de l'app sous le répertoire de données.
const APP_DIR: &str = "kiro-horror-tui";
/// Nom du fichier de l'almanach.
const FILE: &str = "almanac.json";
/// Version du format disque.
const VERSION: u32 = 1;

/// Format disque (versionné pour de futures migrations).
#[derive(Serialize, Deserialize)]
struct Disk {
    #[serde(default)]
    version: u32,
    days: BTreeMap<NaiveDate, DayTally>,
}

/// `$XDG_DATA_HOME/kiro-horror-tui/almanac.json` (chemin absolu seulement),
/// sinon `$HOME/.local/share/kiro-horror-tui/almanac.json`.
pub(super) fn default_path() -> Result<PathBuf> {
    let xdg = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    if let Some(dir) = xdg.filter(|d| d.is_absolute()) {
        return Ok(dir.join(APP_DIR).join(FILE));
    }
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .context("missing HOME (almanac path not found)")?;
    Ok(PathBuf::from(home)
        .join(".local/share")
        .join(APP_DIR)
        .join(FILE))
}

/// Lit les jours du fichier ; absent = vide, illisible = erreur.
pub(super) fn read_days(path: &Path) -> Result<BTreeMap<NaiveDate, DayTally>> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
    };
    let disk: Disk = serde_json::from_str(&raw)
        .with_context(|| format!("unreadable almanac {}", path.display()))?;
    Ok(disk.days)
}

/// Écrit les jours (fichier temporaire propre au processus puis
/// renommage : jamais de fichier à moitié écrit).
pub(super) fn write_days(path: &Path, days: &BTreeMap<NaiveDate, DayTally>) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let disk = Disk {
        version: VERSION,
        days: days.clone(),
    };
    let json = serde_json::to_string_pretty(&disk)?;
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::almanac::{today, Almanac};

    /// Fichier d'almanach jetable, propre à chaque test.
    fn tmp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kh-almanac-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        dir.join("nested").join(FILE)
    }

    #[test]
    fn record_persists_and_merges_two_crypts() {
        let path = tmp_file("merge");
        let mut first = Almanac::open(path.clone());
        let mut second = Almanac::open(path.clone());
        first.record(10, 5);
        second.record(1, 1);
        assert_eq!(first.error(), None);
        let reopened = Almanac::open(path.clone());
        let tally = reopened.day(today());
        assert_eq!((tally.total(), tally.calls), (17, 2));
        std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).ok();
    }

    #[test]
    fn corrupt_file_is_never_overwritten() {
        let path = tmp_file("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        let mut a = Almanac::open(path.clone());
        assert!(a.error().unwrap().contains("unreadable almanac"));
        a.record(3, 4);
        assert_eq!(a.day(today()).total(), 7);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
        std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).ok();
    }
}
