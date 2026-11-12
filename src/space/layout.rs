//! Mise en place du monde : où se trouvent les projets, dossiers et fichiers.
//!
//! Fonctions pures — la caméra (`cam`) et le rendu (`render`) interrogent le
//! même [`Layout`], donc un fichier a UNE position dans le monde.
//! Tailles reprises de `docs/design/project-space/DESIGN.md` §6.1.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::proj::Vec3;
use super::scan::{FileNode, ProjectTree};

/// Côté d'une dalle (carré au sol), unités monde.
pub const SLAB: f32 = 110.0;
/// Hauteur d'une dalle.
pub const SLAB_H: f32 = 12.0;
/// Pas vertical entre deux dalles d'une même pile.
pub const SLAB_PITCH: f32 = 22.0;
/// Pas horizontal entre deux fichiers d'un même dossier.
pub const FILE_PITCH: f32 = 150.0;
/// Marge intérieure d'une plaque de dossier.
pub const FOLDER_PAD: f32 = 85.0;
/// Pas entre deux rangées de dossiers.
pub const ROW_PITCH: f32 = 620.0;
/// Écart entre deux quartiers (projets) : 3 500–9 000 dans le design.
pub const DISTRICT_PITCH: f32 = 4500.0;
/// Au-delà, les fichiers d'un dossier sont regroupés en une seule pile
/// (contrainte « ≥ 30 fps sur 500 fichiers » du brief).
pub const MAX_FILES_PER_FOLDER: usize = 24;

/// Un dossier posé au sol : sa plaque.
#[derive(Debug, Clone, PartialEq)]
pub struct DirBox {
    pub path: PathBuf,
    /// Coin avant-gauche au sol (`y = 0`).
    pub origin: Vec3,
    pub w: f32,
    pub d: f32,
    /// Nombre de fichiers du dossier.
    pub files: usize,
}

/// Un fichier posé au sol : la base de sa pile de dalles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FileBox {
    pub base: Vec3,
    /// Index du dossier dans [`Layout::dirs`].
    pub dir: usize,
}

/// Un quartier (projet) posé au sol : sa plaque extérieure et son nom.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectPlate {
    pub name: String,
    pub origin: Vec3,
    pub w: f32,
    pub d: f32,
    /// Le projet actif : cadre orange pointillé « you are here ».
    pub active: bool,
}

/// Le monde d'un projet.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub dirs: Vec<DirBox>,
    files: BTreeMap<PathBuf, FileBox>,
    /// Dossiers trop denses : une pile unique au centre de la plaque.
    grouped: BTreeMap<usize, FileBox>,
    /// Quartiers : un par projet (le premier est l'actif).
    pub projects: Vec<ProjectPlate>,
}

/// Origine du quartier d'un projet (`0` = projet actif, au centre ; les autres
/// s'écartent alternativement à droite et à gauche).
pub fn district_origin(index: usize) -> Vec3 {
    if index == 0 {
        return Vec3::default();
    }
    let ring = ((index + 1) / 2) as f32;
    let side = if index % 2 == 1 { 1.0 } else { -1.0 };
    Vec3::new(side * DISTRICT_PITCH * ring, 0.0, 0.0)
}

impl Layout {
    /// Construit la disposition d'un arbre de projet (projet actif seul).
    pub fn build(tree: &ProjectTree) -> Self {
        let name = tree
            .root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| tree.root.to_string_lossy().to_string());
        Self::build_many(&[(name, tree.clone(), true)])
    }

    /// Pose plusieurs quartiers : un par projet, décalés de
    /// [`district_origin`]. Les chemins du projet actif restent relatifs ;
    /// ceux des autres sont préfixés par leur racine (clés uniques).
    pub fn build_many(entries: &[(String, ProjectTree, bool)]) -> Self {
        let mut out = Self::default();
        for (i, (name, tree, active)) in entries.iter().enumerate() {
            let off = district_origin(i);
            let dirs = dir_boxes(tree, off);
            let base = out.dirs.len();
            for (file, rank) in ranked(tree) {
                let Some(dir) = dirs.get(file.dir) else {
                    continue;
                };
                let key = if *active {
                    file.path.clone()
                } else {
                    tree.root.join(&file.path)
                };
                out.files.insert(key, slot_of(dir, rank, base + file.dir));
                let g = base + file.dir;
                if dir.files > MAX_FILES_PER_FOLDER && !out.grouped.contains_key(&g) {
                    out.grouped.insert(g, group_center(dir, g));
                }
            }
            let (w, d) = bounds(&dirs);
            out.projects.push(ProjectPlate {
                name: name.clone(),
                origin: off,
                w,
                d,
                active: *active,
            });
            out.dirs.extend(dirs);
        }
        out
    }

    /// Position d'un fichier.
    pub fn file(&self, path: &Path) -> Option<&FileBox> {
        self.files.get(path)
    }

    /// Pile regroupée d'un dossier trop dense.
    pub fn grouped_of(&self, dir: usize) -> Option<&FileBox> {
        self.grouped.get(&dir)
    }

    /// Tous les fichiers posés, par chemin croissant.
    pub fn files(&self) -> impl Iterator<Item = (&PathBuf, &FileBox)> {
        self.files.iter()
    }

    /// Centre du projet actif (cible de caméra au repos).
    pub fn center(&self) -> Vec3 {
        self.projects
            .iter()
            .find(|p| p.active)
            .or_else(|| self.projects.first())
            .map(|p| p.origin + Vec3::new(p.w / 2.0, 0.0, -p.d / 2.0))
            .unwrap_or_default()
    }
}

/// Une plaque par dossier, rangée en profondeur.
fn dir_boxes(tree: &ProjectTree, off: Vec3) -> Vec<DirBox> {
    tree.dirs
        .iter()
        .enumerate()
        .map(|(i, dir)| {
            let n = tree.files.iter().filter(|f| f.dir == i).count();
            DirBox {
                path: dir.path.clone(),
                origin: off + Vec3::new(0.0, 0.0, -(i as f32) * ROW_PITCH),
                w: plate_width(n),
                d: SLAB + FOLDER_PAD * 2.0,
                files: n,
            }
        })
        .collect()
}

/// Emprise d'un quartier : largeur de sa plus grande plaque, profondeur de
/// ses rangées de dossiers.
fn bounds(dirs: &[DirBox]) -> (f32, f32) {
    let w = dirs.iter().map(|b| b.w).fold(0.0, f32::max);
    (w.max(SLAB), dirs.len() as f32 * ROW_PITCH)
}

/// Largeur d'une plaque pour `n` fichiers (bornée au regroupement).
fn plate_width(n: usize) -> f32 {
    let n = n.clamp(1, MAX_FILES_PER_FOLDER) as f32;
    FOLDER_PAD * 2.0 + (n - 1.0) * FILE_PITCH + SLAB
}

/// Fichiers triés par dossier puis par chemin, avec leur rang dans le dossier.
fn ranked(tree: &ProjectTree) -> Vec<(&FileNode, usize)> {
    let mut out: Vec<(&FileNode, usize)> = Vec::new();
    for i in 0..tree.dirs.len() {
        let mut siblings: Vec<&FileNode> = tree.files.iter().filter(|f| f.dir == i).collect();
        siblings.sort_by(|a, b| a.path.cmp(&b.path));
        out.extend(siblings.into_iter().enumerate().map(|(r, f)| (f, r)));
    }
    out
}

/// Position au sol du fichier de rang `rank` dans la plaque `dir`.
fn slot_of(dir: &DirBox, rank: usize, dir_idx: usize) -> FileBox {
    let r = rank.min(MAX_FILES_PER_FOLDER - 1) as f32;
    FileBox {
        base: Vec3::new(
            dir.origin.x + FOLDER_PAD + SLAB / 2.0 + r * FILE_PITCH,
            0.0,
            dir.origin.z + FOLDER_PAD + SLAB / 2.0,
        ),
        dir: dir_idx,
    }
}

/// Centre d'une plaque (pile regroupée d'un dossier trop dense).
fn group_center(dir: &DirBox, dir_idx: usize) -> FileBox {
    FileBox {
        base: Vec3::new(dir.origin.x + dir.w / 2.0, 0.0, dir.origin.z + dir.d / 2.0),
        dir: dir_idx,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::scan::DirNode;

    fn tree(dirs: &[&str], files: &[(&str, usize, u32)]) -> ProjectTree {
        ProjectTree {
            root: PathBuf::from("/p"),
            dirs: dirs
                .iter()
                .enumerate()
                .map(|(i, p)| DirNode {
                    path: PathBuf::from(p),
                    parent: (i > 0).then_some(0),
                })
                .collect(),
            files: files
                .iter()
                .map(|(p, d, l)| FileNode {
                    path: PathBuf::from(p),
                    dir: *d,
                    lines: *l,
                })
                .collect(),
        }
    }

    #[test]
    fn quartier_actif_au_centre() {
        assert_eq!(district_origin(0), Vec3::default());
        assert!(district_origin(1).x > 3000.0);
        assert!(district_origin(2).x < -3000.0);
        assert!(district_origin(3).x > district_origin(1).x);
    }

    #[test]
    fn fichiers_alignés_dans_leur_dossier() {
        let l = Layout::build(&tree(&["src"], &[("src/a.rs", 0, 10), ("src/b.rs", 0, 10)]));
        let a = l.file(Path::new("src/a.rs")).expect("posé");
        let b = l.file(Path::new("src/b.rs")).expect("posé");
        assert!((b.base.x - a.base.x - FILE_PITCH).abs() < 0.01);
        assert_eq!(a.base.z, b.base.z);
    }

    #[test]
    fn dossiers_en_profondeur() {
        let l = Layout::build(&tree(
            &["src", "tests"],
            &[("src/a.rs", 0, 10), ("tests/b.rs", 1, 10)],
        ));
        assert_eq!(l.dirs.len(), 2);
        assert!((l.dirs[0].origin.z - l.dirs[1].origin.z).abs() > ROW_PITCH - 0.01);
        assert_eq!(l.dirs[1].files, 1);
    }

    #[test]
    fn largeur_plaque_croît_puis_sature() {
        assert!(plate_width(5) > plate_width(1));
        assert_eq!(plate_width(0), plate_width(1));
        assert_eq!(plate_width(999), plate_width(MAX_FILES_PER_FOLDER));
    }

    #[test]
    fn dossier_trop_dense_regroupé() {
        let files: Vec<(String, usize, u32)> = (0..MAX_FILES_PER_FOLDER + 3)
            .map(|i| (format!("src/f{i:02}.rs"), 0, 10))
            .collect();
        let refs: Vec<(&str, usize, u32)> =
            files.iter().map(|(p, d, l)| (p.as_str(), *d, *l)).collect();
        let l = Layout::build(&tree(&["src"], &refs));
        assert!(l.grouped_of(0).is_some());
        assert!(l.file(Path::new("src/f00.rs")).is_some());
    }

    #[test]
    fn dossier_courant_non_regroupé() {
        let l = Layout::build(&tree(&["src"], &[("src/a.rs", 0, 10)]));
        assert!(l.grouped_of(0).is_none());
        // Le centre est le milieu du quartier actif, pas l'origine.
        assert!((l.center().z + ROW_PITCH / 2.0).abs() < 0.01);
    }
}
