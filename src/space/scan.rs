//! Lecture des projets depuis le disque (DESIGN §9).
//!
//! Fonctions **pures** autant que possible : le projet actif est le dossier où
//! Crypt est lancé ; les autres projets sont les sous-dossiers contenant un
//! `.git` dans une liste configurable de dossiers de travail (ex. `~/code`).
//! On ignore `target/`, `.git/`, `node_modules/`, les binaires.

// Module pas encore branche sur la vue 3D : `scan_tree`/`list_projects` ne sont
// appeles de nulle part, donc tout le module remonte en `dead_code`. A retirer
// des que le rendu 3D lit les projets (DESIGN §9).
#![allow(dead_code)]

use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Un dossier du projet (une plaque au sol).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirNode {
    /// Chemin relatif à la racine du projet.
    pub path: PathBuf,
    /// Index du dossier parent dans [`ProjectTree::dirs`] (`None` = racine).
    pub parent: Option<usize>,
}

/// Un fichier du projet (une pile de dalles).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileNode {
    /// Chemin relatif à la racine du projet.
    pub path: PathBuf,
    /// Index du dossier contenant dans [`ProjectTree::dirs`].
    pub dir: usize,
    /// Nombre de lignes (pour les dalles).
    pub lines: u32,
}

/// L'arbre d'un projet : dossiers et fichiers, racine comprise.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ProjectTree {
    pub root: PathBuf,
    pub dirs: Vec<DirNode>,
    pub files: Vec<FileNode>,
}

/// Dossiers ignorés : sorties de build, dépendances, caches.
const IGNORED_DIRS: &str = "target node_modules dist build out __pycache__ vendor";

/// Extensions lues comme du texte ; les autres fichiers sont ignorés.
const TEXT_EXTENSIONS: &str = "\
    rs toml md txt json yaml yml html css scss sass less js mjs cjs jsx ts tsx \
    sh bash zsh fish py rb go c h cc cpp hpp cs java kt kts swift scala sql xml \
    svg ini cfg conf env lock csv tsv vue svelte lua php pl ex exs erl hs ml r \
    dart zig nim proto graphql tf gradle properties patch diff log dockerfile";

/// Vrai si `name` figure dans la liste `names` (séparée par des espaces).
fn in_list(names: &str, name: &str) -> bool {
    names
        .split_whitespace()
        .any(|n| n.eq_ignore_ascii_case(name))
}

/// Octets lus pour décider « texte ou binaire » (presence d'un octet NUL).
const SAMPLE_BYTES: usize = 4096;

/// Octets lus au plus pour compter les lignes (garde-fou sur les monstres).
const LINE_CAP_BYTES: u64 = 8 * 1024 * 1024;

/// Taille du tampon de lecture.
const CHUNK_BYTES: usize = 64 * 1024;

/// Profondeur maximale du parcours (garde-fou).
const MAX_DEPTH: usize = 32;

/// Parcourt `root` et construit son arbre (DESIGN §9).
///
/// La racine est `dirs[0]` (`parent: None`). Le parcours est deterministe :
/// dossiers et fichiers sont tries par chemin. Les binaires, les dossiers
/// caches et les sorties de build sont sautes.
pub fn scan_tree(root: &Path) -> std::io::Result<ProjectTree> {
    if !root.is_dir() {
        return Err(std::io::ErrorKind::NotFound.into());
    }
    let mut tree = ProjectTree {
        root: root.to_path_buf(),
        ..ProjectTree::default()
    };
    walk(root, root, None, 0, &mut tree);
    Ok(tree)
}

/// Parcours recursif : remplit `tree` (dossiers, puis fichiers rattaches).
///
/// Un dossier illisible ne fait pas echouer le scan ; les liens symboliques ne
/// sont pas suivis (pas de boucle, pas de double compte).
fn walk(root: &Path, dir: &Path, parent: Option<usize>, depth: usize, tree: &mut ProjectTree) {
    let index = tree.dirs.len();
    tree.dirs.push(DirNode {
        path: relative(root, dir),
        parent,
    });
    if depth >= MAX_DEPTH {
        return;
    }
    let mut subs: Vec<PathBuf> = Vec::new();
    for entry in sorted_entries(dir) {
        let path = entry.path();
        match entry.file_type() {
            Ok(ft) if ft.is_dir() => {
                if !is_ignored_dir(&path) {
                    subs.push(path);
                }
            }
            Ok(ft) if ft.is_file() => push_file(root, &path, index, tree),
            _ => {}
        }
    }
    for sub in subs {
        walk(root, &sub, Some(index), depth + 1, tree);
    }
}

/// Ajoute un fichier texte a l'arbre (binaire, cache ou illisible : ignore).
fn push_file(root: &Path, path: &Path, dir: usize, tree: &mut ProjectTree) {
    if !is_text_file(path) {
        return;
    }
    if let Ok(lines) = count_lines(path) {
        tree.files.push(FileNode {
            path: relative(root, path),
            dir,
            lines,
        });
    }
}

/// Nombre de lignes d'un fichier (lu en binaire, par morceaux).
///
/// Les sauts de ligne sont comptes ; on ajoute 1 si le fichier ne finit pas
/// par un saut de ligne. La lecture est bornee a [`LINE_CAP_BYTES`].
fn count_lines(path: &Path) -> std::io::Result<u32> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut buf = vec![0u8; CHUNK_BYTES];
    let mut lines: u64 = 0;
    let mut last: Option<u8> = None;
    let mut read: u64 = 0;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        lines += buf[..n].iter().filter(|b| **b == b'\n').count() as u64;
        last = Some(buf[n - 1]);
        read += n as u64;
        if read >= LINE_CAP_BYTES {
            break;
        }
    }
    if let Some(byte) = last {
        if byte != b'\n' {
            lines += 1;
        }
    }
    Ok(lines.min(u32::MAX as u64) as u32)
}

/// Vrai si le fichier est du texte visible (ni cache, ni binaire).
fn is_text_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.starts_with('.') {
        return false;
    }
    if has_nul_byte(path) {
        return false;
    }
    match path.extension().and_then(|e| e.to_str()) {
        // Sans extension (`Makefile`, `LICENSE`) : l'echantillon decide.
        None => true,
        Some(ext) => in_list(TEXT_EXTENSIONS, ext),
    }
}

/// Vrai si l'echantillon lu contient un octet NUL (donc un binaire).
fn has_nul_byte(path: &Path) -> bool {
    let mut buf = vec![0u8; SAMPLE_BYTES];
    match fs::File::open(path).and_then(|f| BufReader::new(f).read(&mut buf)) {
        Ok(n) => buf[..n].contains(&0),
        // Illisible : mieux vaut le sauter que de le compter faux.
        Err(_) => true,
    }
}

/// Vrai si le dossier est ignore (cache, `target/`, `node_modules/`...).
fn is_ignored_dir(path: &Path) -> bool {
    match path.file_name().and_then(|n| n.to_str()) {
        None => true,
        Some(name) => name.starts_with('.') || in_list(IGNORED_DIRS, name),
    }
}

/// Entrees d'un dossier triees par chemin (parcours deterministe).
fn sorted_entries(dir: &Path) -> Vec<fs::DirEntry> {
    let mut entries: Vec<fs::DirEntry> = match fs::read_dir(dir) {
        Ok(read) => read.flatten().collect(),
        Err(_) => Vec::new(),
    };
    entries.sort_by_key(fs::DirEntry::path);
    entries
}

/// Chemin relatif a la racine du projet (la racine donne un chemin vide).
fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

/// Projets connus : le projet actif d'abord, puis les depots des workspaces.
///
/// Un workspace est un dossier (`~/code`) ; chaque sous-dossier qui contient un
/// `.git` est un projet. Les doublons sont retires, les dossiers absents
/// ignores.
pub fn list_projects(workspaces: &[PathBuf], active: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    push_unique(&mut out, active);
    for ws in workspaces {
        for sub in sorted_paths(ws) {
            if is_project_dir(&sub) {
                push_unique(&mut out, &sub);
            }
        }
    }
    out
}

/// Vrai si le dossier est un depot git (`.git` present).
pub fn is_project_dir(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// Sous-dossiers directs d'un workspace, tries par chemin.
fn sorted_paths(dir: &Path) -> Vec<PathBuf> {
    let mut subs: Vec<PathBuf> = sorted_entries(dir)
        .into_iter()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();
    subs.sort();
    subs
}

/// Ajoute un chemin s'il n'est pas deja liste.
fn push_unique(list: &mut Vec<PathBuf>, candidate: &Path) {
    if !is_duplicate(list, candidate) {
        list.push(candidate.to_path_buf());
    }
}

/// Vrai si `candidate` est deja dans `list` (egalement via son chemin canonique).
fn is_duplicate(list: &[PathBuf], candidate: &Path) -> bool {
    if list.iter().any(|p| p == candidate) {
        return true;
    }
    match fs::canonicalize(candidate) {
        Ok(canon) => list
            .iter()
            .any(|p| fs::canonicalize(p).is_ok_and(|q| q == canon)),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Repertoire temporaire jetable (unique par nom de test).
    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kh-scan-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// Ecrit un fichier (chemin relatif possible : `src/main.rs`).
    fn write(root: &Path, rel: &str, body: &[u8]) -> PathBuf {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, body).expect("write");
        path
    }

    /// Cree un dossier (avec ses parents).
    fn mkdir(root: &Path, rel: &str) -> PathBuf {
        let path = root.join(rel);
        std::fs::create_dir_all(&path).expect("mkdir");
        path
    }

    /// Lignes comptees pour un fichier de l'arbre.
    fn lines_of(tree: &ProjectTree, rel: &str) -> Option<u32> {
        tree.files
            .iter()
            .find(|f| f.path == PathBuf::from(rel))
            .map(|f| f.lines)
    }

    #[test]
    fn ignores_build_dirs_hidden_and_binaries() {
        let root = temp_root("ignores");
        write(&root, "src/lib.rs", b"let x = 1;\n");
        write(&root, "target/debug/build.rs", b"let x = 1;\n");
        write(&root, ".git/config", b"[core]\n");
        write(&root, "node_modules/lib/index.js", b"1\n");
        write(&root, ".secret", b"x\n");
        write(&root, "assets/logo.png", b"\x89PNG\x00\x00");
        let tree = scan_tree(&root).expect("scan");
        assert_eq!(tree.files.len(), 1);
        assert_eq!(tree.files[0].path, PathBuf::from("src/lib.rs"));
        let dirs: Vec<String> = tree
            .dirs
            .iter()
            .map(|d| d.path.to_string_lossy().to_string())
            .collect();
        assert!(!dirs.iter().any(|d| d.contains("target")));
        assert!(!dirs.iter().any(|d| d.contains("node_modules")));
        assert!(!dirs.iter().any(|d| d.contains(".git")));
    }

    #[test]
    fn counts_lines_of_text_files() {
        let root = temp_root("lines");
        write(&root, "empty.txt", b"");
        write(&root, "one.txt", b"hello");
        write(&root, "two.txt", b"a\nb\n");
        write(&root, "three.txt", b"a\nb\nc");
        let tree = scan_tree(&root).expect("scan");
        assert_eq!(lines_of(&tree, "empty.txt"), Some(0));
        assert_eq!(lines_of(&tree, "one.txt"), Some(1));
        assert_eq!(lines_of(&tree, "two.txt"), Some(2));
        assert_eq!(lines_of(&tree, "three.txt"), Some(3));
    }

    #[test]
    fn links_dirs_to_their_parent() {
        let root = temp_root("parents");
        write(&root, "src/deep/mod.rs", b"x\n");
        write(&root, "src/main.rs", b"x\n");
        let tree = scan_tree(&root).expect("scan");
        let index_of = |rel: &str| {
            tree.dirs
                .iter()
                .position(|d| d.path == PathBuf::from(rel))
                .unwrap_or_else(|| panic!("dir {rel}"))
        };
        let root_i = index_of("");
        let src_i = index_of("src");
        let deep_i = index_of("src/deep");
        assert_eq!(tree.dirs[root_i].parent, None);
        assert_eq!(tree.dirs[src_i].parent, Some(root_i));
        assert_eq!(tree.dirs[deep_i].parent, Some(src_i));
        let main = tree
            .files
            .iter()
            .find(|f| f.path == PathBuf::from("src/main.rs"))
            .expect("main.rs");
        assert_eq!(main.dir, src_i);
    }

    #[test]
    fn skips_binary_files() {
        let root = temp_root("binary");
        write(&root, "code.rs", b"let x = 1;\n");
        write(&root, "blob.rs", b"\x00\x01\x02binary\x00");
        write(&root, "Makefile", b"all:\n\techo hi\n");
        let tree = scan_tree(&root).expect("scan");
        assert_eq!(tree.files.len(), 2);
        assert!(tree
            .files
            .iter()
            .any(|f| f.path == PathBuf::from("Makefile")));
        assert!(tree
            .files
            .iter()
            .all(|f| f.path != PathBuf::from("blob.rs")));
    }

    #[test]
    fn orders_files_deterministically() {
        let root = temp_root("order");
        for name in ["b.rs", "a.rs", "c.rs"] {
            write(&root, name, b"x\n");
        }
        let tree = scan_tree(&root).expect("scan");
        let names: Vec<String> = tree
            .files
            .iter()
            .map(|f| f.path.to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["a.rs", "b.rs", "c.rs"]);
    }

    #[test]
    fn lists_active_first_then_workspace_repos() {
        let ws = temp_root("projects-ws");
        let active = temp_root("projects-active");
        mkdir(&ws, "alpha/.git");
        mkdir(&ws, "beta/.git");
        mkdir(&ws, "not-a-repo");
        mkdir(&active, ".git");
        let projects = list_projects(&[ws.clone()], &active);
        assert_eq!(projects.first(), Some(&active));
        assert_eq!(projects.len(), 3);
        assert!(projects.iter().any(|p| p.ends_with("alpha")));
        assert!(projects.iter().any(|p| p.ends_with("beta")));
        assert!(!projects.iter().any(|p| p.ends_with("not-a-repo")));
    }

    #[test]
    fn dedups_active_already_in_workspace() {
        let ws = temp_root("dedup-ws");
        let repo = ws.join("repo");
        mkdir(&ws, "repo/.git");
        let projects = list_projects(&[ws.clone()], &repo);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0], repo);
    }

    #[test]
    fn ignores_missing_workspaces_and_roots() {
        let active = temp_root("missing-active");
        let ghost = std::env::temp_dir().join("kh-scan-ghost-does-not-exist");
        std::fs::remove_dir_all(&ghost).ok();
        let projects = list_projects(&[ghost.clone()], &active);
        assert_eq!(projects, vec![active]);
        assert!(scan_tree(&ghost).is_err());
    }

    #[test]
    fn detects_git_projects() {
        let root = temp_root("isdir");
        assert!(!is_project_dir(&root));
        mkdir(&root, ".git");
        assert!(is_project_dir(&root));
        // Un worktree a un `.git` fichier : c'est un depot aussi.
        let file_git = temp_root("isdir-file");
        write(&file_git, ".git", b"gitdir: /elsewhere\n");
        assert!(is_project_dir(&file_git));
    }
}
