//! Checkpoints : snapshot avant écriture, liste, restauration.
//!
//! `write_tracked(path, content)` snapshotte le contenu précédent (anneau
//! des 20 derniers) puis écrit ; `/checkpoints` liste (plus récent
//! d'abord), `/rewind <n|nom>` restaure (`n` = rang affiché, `nom` =
//! basename exact ou fragment du chemin d'origine, plus récent gagnant).
//!
//! Dossier : `KH_CHECKPOINT_DIR` (tests) sinon
//! `~/.local/share/kiro-horror-tui/checkpoints/`. Snapshot =
//! `{nanos}-{nom sûr}.snap`, 1re ligne = chemin absolu d'origine, suite =
//! octets précédents (binaire OK). Sans `HOME` : écriture simple, sans
//! snapshot (l'app doit marcher partout).
//!
//! Déjà branché : `memory::remember` (snapshot avant ajout).
//!
//! Branchés via `T5State.checkpoint_dir` (`None` = sans snapshot) :
//! `memory::remember`, `spec_cmds::write_doc` (3 docs),
//! `App::spec_check_off` (cochage `tasks.md`).

use std::path::{Path, PathBuf};

/// Snapshots gardés (anneau : les plus vieux sortent).
const RING: usize = 20;
/// Sous-dossier des snapshots (dossier : `crate::paths::data_dir`).
const DIR_NAME: &str = "checkpoints";
/// Extension des snapshots (le reste du dossier est ignoré).
const EXT: &str = "snap";

/// Snapshot : fichier + chemin d'origine (restauration).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub file: PathBuf,
    pub original: PathBuf,
}

/// Dossier des snapshots (`KH_CHECKPOINT_DIR` prime, sinon data app).
pub fn dir() -> Option<PathBuf> {
    if let Some(dir) = crate::kiro::read_env("KH_CHECKPOINT_DIR") {
        return Some(PathBuf::from(dir));
    }
    crate::paths::data_dir().map(|dir| dir.join(DIR_NAME))
}

/// Écrit `content` dans `path` en snapshottant le précédent (s'il existe).
/// Dossier indisponible : écriture simple, sans snapshot.
pub fn write_tracked(path: &Path, content: &str) -> std::io::Result<()> {
    write_via(dir().as_deref(), path, content)
}

/// `write_tracked` avec dossier explicite (`None` = écriture simple).
pub fn write_via(ckpt: Option<&Path>, path: &Path, content: &str) -> std::io::Result<()> {
    match ckpt {
        Some(dir) => write_tracked_in(dir, path, content),
        None => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, content)
        }
    }
}

/// `write_tracked` dans `ckpt` (chemin injecté, tests).
pub fn write_tracked_in(ckpt: &Path, path: &Path, content: &str) -> std::io::Result<()> {
    if let Ok(old) = std::fs::read(path) {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
        };
        std::fs::create_dir_all(ckpt)?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut snap = format!("{abs}\n", abs = abs.display()).into_bytes();
        snap.extend_from_slice(&old);
        std::fs::write(ckpt.join(format!("{nanos}-{}.snap", sanitize(&base))), snap)?;
        prune_in(ckpt);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
}

/// Snapshots, plus récent d'abord (chemin injecté, tests ; illisibles ignorés).
pub fn list_in(dir: &Path) -> Vec<Snapshot> {
    let mut files = snap_files(dir);
    files.reverse();
    files
        .into_iter()
        .filter_map(|file| {
            let bytes = std::fs::read(&file).ok()?;
            let (original, _) = split_snapshot(&bytes)?;
            Some(Snapshot { file, original })
        })
        .collect()
}

/// Restaure `target` dans `dir` : rang `1..=n` ou basename/fragment du
/// chemin d'origine (plus récent gagnant) ; message succès/erreur (anglais).
pub fn rewind_in(dir: &Path, target: &str) -> Result<String, String> {
    let snaps = list_in(dir);
    if snaps.is_empty() {
        return Err(String::from("no checkpoints"));
    }
    let target = target.trim();
    let picked = match target.parse::<usize>() {
        Ok(n) if n >= 1 => snaps.get(n - 1).cloned(),
        _ => snaps
            .iter()
            .find(|snap| {
                snap.original.file_name().is_some_and(|base| base == target)
                    || snap.original.to_string_lossy().contains(target)
            })
            .cloned(),
    };
    let Some(snap) = picked else {
        if target.parse::<usize>().is_ok() {
            return Err(format!("no checkpoint #{target} (1..={})", snaps.len()));
        }
        return Err(format!("no checkpoint matching '{target}'"));
    };
    let unreadable = format!("unreadable snapshot '{}'", snap.file.display());
    let bytes = std::fs::read(&snap.file).map_err(|_| unreadable.clone())?;
    let Some((original, body)) = split_snapshot(&bytes) else {
        return Err(unreadable);
    };
    let restore_err = |err: std::io::Error| format!("cannot restore: {err}");
    if let Some(parent) = original.parent() {
        std::fs::create_dir_all(parent).map_err(restore_err)?;
    }
    std::fs::write(&original, body).map_err(restore_err)?;
    let file = snap
        .file
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    Ok(format!("restored {} (snapshot {file})", original.display()))
}

/// Fichiers `*.snap` triés (anciens d'abord : noms = nanos croissants).
fn snap_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == EXT) {
            files.push(path);
        }
    }
    files.sort();
    files
}

/// Garde les 20 snapshots les plus récents (les vieux sortent).
fn prune_in(dir: &Path) {
    let files = snap_files(dir);
    if files.len() > RING {
        for old in &files[..files.len() - RING] {
            std::fs::remove_file(old).ok();
        }
    }
}

/// Sépare un snapshot : (chemin d'origine, octets) (`None` si malformé).
fn split_snapshot(bytes: &[u8]) -> Option<(PathBuf, Vec<u8>)> {
    let cut = bytes.iter().position(|b| *b == b'\n')?;
    let head = std::str::from_utf8(&bytes[..cut]).ok()?;
    if head.trim().is_empty() {
        return None;
    }
    Some((PathBuf::from(head), bytes[cut + 1..].to_vec()))
}

/// Nom sûr pour fichier (alphanum + `._-`, le reste en `_`, 60 max).
fn sanitize(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = safe
        .trim_matches(['.', '_'])
        .chars()
        .take(60)
        .collect::<String>();
    if trimmed.is_empty() {
        String::from("file")
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kh-checkpoint-{name}"))
    }

    #[test]
    fn cycle_snapshot_liste_rewind_par_rang_et_nom() {
        let dir = tmp_dir("cycle");
        std::fs::remove_dir_all(&dir).ok();
        let ckpt = dir.join("ckpt");
        let target = dir.join("doc.md");
        write_tracked_in(&ckpt, &target, "v1").unwrap();
        assert!(list_in(&ckpt).is_empty()); // créé : rien à snapshotté
        write_tracked_in(&ckpt, &target, "v2").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        write_tracked_in(&ckpt, &target, "v3").unwrap();
        let snaps = list_in(&ckpt);
        assert_eq!(snaps.len(), 2);
        assert_eq!(snaps[0].original, target); // plus récent d'abord
        assert!(rewind_in(&ckpt, "1").unwrap().starts_with("restored "));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v2");
        assert!(rewind_in(&ckpt, "doc.md").unwrap().starts_with("restored "));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v2"); // plus récent
        assert!(rewind_in(&ckpt, "2").unwrap().starts_with("restored "));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v1");
        assert_eq!(
            rewind_in(&ckpt, "9"),
            Err(String::from("no checkpoint #9 (1..=2)"))
        );
        assert_eq!(
            rewind_in(&ckpt, "zzz"),
            Err(String::from("no checkpoint matching 'zzz'"))
        );
        assert_eq!(
            rewind_in(&dir.join("vide"), "1"),
            Err(String::from("no checkpoints"))
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn anneau_garde_vingt_et_ignore_intrus() {
        let dir = tmp_dir("ring");
        std::fs::remove_dir_all(&dir).ok();
        let ckpt = dir.join("ckpt");
        let target = dir.join("doc.md");
        write_tracked_in(&ckpt, &target, "v0").unwrap();
        for i in 1..25 {
            std::thread::sleep(std::time::Duration::from_millis(2));
            write_tracked_in(&ckpt, &target, &format!("v{i}")).unwrap();
        }
        // 24 snapshots pris, 20 gardés (v4..v23).
        assert_eq!(list_in(&ckpt).len(), RING);
        std::fs::write(ckpt.join("note.txt"), "intrus").unwrap();
        std::fs::write(ckpt.join("99999999999999999999-broken.snap"), "sans-retour").unwrap();
        assert_eq!(list_in(&ckpt).len(), RING);
        assert!(rewind_in(&ckpt, "1").unwrap().starts_with("restored "));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v23");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sanitize_et_split_snapshot() {
        // `dir()` (env) est couvert par le test handler (`checkpoint_cmds`,
        // seul à poser `KH_CHECKPOINT_DIR`) ; ici : pur et déterministe.
        assert_eq!(sanitize("a/b c.md"), "a_b_c.md");
        assert_eq!(sanitize("..."), "file");
        let (path, body) = split_snapshot(b"/tmp/x.md\nhello").unwrap();
        assert_eq!(path, PathBuf::from("/tmp/x.md"));
        assert_eq!(body, b"hello");
        assert!(split_snapshot(b"sans-retour").is_none());
        assert!(split_snapshot(b"\nbody").is_none());
    }
}
