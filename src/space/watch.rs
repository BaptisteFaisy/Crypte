//! Observation du disque sans dépendance : diff de [`scan::scan_tree`] entre
//! deux polls. Capte les écritures faites hors de l'outil shell (sous-agents,
//! éditeur externe) — l'heuristique shell de `scene.rs` ne voit que `shell`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::scan::{self, ProjectTree};

/// Un changement détecté entre deux `poll`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Fichier modifié : lignes ajoutées / retirées (diff de comptes).
    Written {
        path: PathBuf,
        added: u32,
        removed: u32,
    },
    /// Fichier apparu.
    Created { path: PathBuf, lines: u32 },
    /// Fichier disparu.
    Removed { path: PathBuf },
}

/// Mémorise le compte de lignes de chaque fichier d'un projet.
#[derive(Debug, Default)]
pub struct Watcher {
    lines: BTreeMap<PathBuf, u32>,
}

impl Watcher {
    /// Watcher vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Prend l'état initial sans émettre de changement.
    pub fn seed(&mut self, root: &Path) {
        self.lines = snapshot(root);
    }

    /// Compare l'état courant au disque ; changements triés par chemin.
    pub fn poll(&mut self, root: &Path) -> Vec<Change> {
        let now = snapshot(root);
        let mut out: Vec<Change> = Vec::new();
        for (path, lines) in &now {
            match self.lines.get(path) {
                None => out.push(Change::Created {
                    path: path.clone(),
                    lines: *lines,
                }),
                Some(old) if old != lines => out.push(Change::Written {
                    path: path.clone(),
                    added: lines.saturating_sub(*old),
                    removed: old.saturating_sub(*lines),
                }),
                _ => {}
            }
        }
        for path in self.lines.keys() {
            if !now.contains_key(path) {
                out.push(Change::Removed { path: path.clone() });
            }
        }
        self.lines = now;
        out
    }

    /// Nombre de fichiers suivis.
    pub fn tracked(&self) -> usize {
        self.lines.len()
    }
}

/// Compte de lignes par chemin, via le scan existant (mêmes exclusions).
fn snapshot(root: &Path) -> BTreeMap<PathBuf, u32> {
    scan::scan_tree(root)
        .map(|t: ProjectTree| t.files.into_iter().map(|f| (f.path, f.lines)).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmpdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("crypt-watch-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(p.join("src")).expect("mkdir");
        fs::create_dir_all(p.join("target")).expect("mkdir");
        p
    }

    #[test]
    fn seed_n_émet_rien_et_poll_non_plus_sans_changement() {
        let d = tmpdir("seed");
        fs::write(d.join("src/a.rs"), "un\ndeux\n").unwrap();
        let mut w = Watcher::new();
        w.seed(&d);
        assert_eq!(w.tracked(), 1);
        assert!(w.poll(&d).is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn écriture_création_suppression_sont_vues() {
        let d = tmpdir("cycle");
        fs::write(d.join("src/a.rs"), "un\n").unwrap();
        fs::write(d.join("target/ignore.rs"), "x\n").unwrap();
        let mut w = Watcher::new();
        w.seed(&d);
        fs::write(d.join("src/a.rs"), "un\ndeux\ntrois\n").unwrap();
        fs::write(d.join("src/b.rs"), "neuf\n").unwrap();
        let ch = w.poll(&d);
        assert!(
            ch.contains(&Change::Written {
                path: PathBuf::from("src/a.rs"),
                added: 2,
                removed: 0
            }),
            "{ch:?}"
        );
        assert!(
            ch.contains(&Change::Created {
                path: PathBuf::from("src/b.rs"),
                lines: 1
            }),
            "{ch:?}"
        );
        assert!(!ch.iter().any(|c| match c {
            Change::Created { path, .. } => path.starts_with("target"),
            _ => false,
        }));
        fs::remove_file(d.join("src/b.rs")).unwrap();
        let ch2 = w.poll(&d);
        assert!(
            ch2.contains(&Change::Removed {
                path: PathBuf::from("src/b.rs")
            }),
            "{ch2:?}"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn rétrécissement_compte_les_lignes_retirées() {
        let d = tmpdir("shrink");
        fs::write(d.join("a.rs"), "1\n2\n3\n4\n").unwrap();
        let mut w = Watcher::new();
        w.seed(&d);
        fs::write(d.join("a.rs"), "1\n").unwrap();
        let ch = w.poll(&d);
        assert_eq!(
            ch,
            vec![Change::Written {
                path: PathBuf::from("a.rs"),
                added: 0,
                removed: 3
            }]
        );
        let _ = fs::remove_dir_all(&d);
    }
}
