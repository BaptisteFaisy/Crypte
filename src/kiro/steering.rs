//! Steering Kiro (`.kiro/steering/*.md`, projet + global).
//!
//! Frontmatter YAML minimal en tête (`inclusion`, `fileMatchPattern`) ;
//! sans frontmatter → `always` (défaut Kiro). `select` retient `always`
//! + `fileMatch` sur les chemins en contexte (`manual`/`auto` = T2).

use std::path::Path;

/// Mode d'inclusion (défaut : `always`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inclusion {
    Always,
    FileMatch,
    Manual,
    Auto,
}

impl Inclusion {
    /// Parse insensible à la casse, inconnu → `Always`.
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_lowercase().as_str() {
            "filematch" | "file_match" => Self::FileMatch,
            "manual" => Self::Manual,
            "auto" => Self::Auto,
            _ => Self::Always,
        }
    }

    /// Nom court (statut `/kiro`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::FileMatch => "filematch",
            Self::Manual => "manual",
            Self::Auto => "auto",
        }
    }
}

/// Fichier steering parsé (corps sans frontmatter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Steering {
    pub name: String,
    pub inclusion: Inclusion,
    pub patterns: Vec<String>,
    pub body: String,
}

/// Parse un fichier (`name` = nom sans extension).
pub fn parse(name: &str, raw: &str) -> Steering {
    let (front, body) = split_frontmatter(raw);
    let mut file = Steering {
        name: name.to_string(),
        inclusion: Inclusion::Always,
        patterns: Vec::new(),
        body: body.to_string(),
    };
    if let Some(front) = front {
        apply_frontmatter(&mut file, front);
    }
    file
}

/// Sépare frontmatter (`---` en tête) et corps.
fn split_frontmatter(raw: &str) -> (Option<&str>, &str) {
    let mut parts = raw.splitn(3, "---");
    let (head, front, body) = (parts.next(), parts.next(), parts.next());
    match (head, front, body) {
        (Some(head), Some(front), Some(body)) if head.trim().is_empty() => {
            let body = body
                .strip_prefix("\r\n")
                .or_else(|| body.strip_prefix('\n'))
                .unwrap_or(body);
            (Some(front), body)
        }
        _ => (None, raw),
    }
}

/// Applique le frontmatter (lignes `clé: valeur`, listes `- item`).
fn apply_frontmatter(file: &mut Steering, front: &str) {
    let mut key = String::new();
    for line in front.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(item) = line.strip_prefix('-').map(|s| s.trim()) {
            if is_pattern_key(&key) && !item.is_empty() {
                file.patterns.push(unquote(item));
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            key = name.trim().to_lowercase();
            let value = unquote(value.trim());
            match key.as_str() {
                "inclusion" => file.inclusion = Inclusion::parse(&value),
                _ if is_pattern_key(&key) => {
                    if !value.is_empty() {
                        file.patterns.push(value);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Clé de motifs (`fileMatchPattern`, pluriel toléré).
fn is_pattern_key(key: &str) -> bool {
    key.trim_end_matches('s') == "filematchpattern"
}

/// Retire les guillemets simples/doubles autour d'une valeur.
fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

/// Charge un dossier (`.md` triés, illisibles ignorés).
pub fn load_dir(dir: &Path) -> Vec<Steering> {
    let mut paths = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            paths.push(path);
        }
    }
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let raw = std::fs::read_to_string(&path).ok()?;
            let stem = path.file_stem()?.to_string_lossy().into_owned();
            Some(parse(&stem, &raw))
        })
        .collect()
}

/// Charge global puis projet (cwd + home réels).
pub fn load_all() -> Vec<Steering> {
    load_all_in(
        &crate::kiro::project_dir(),
        crate::kiro::global_dir().as_deref(),
    )
}

/// Charge global puis projet (chemins injectés, tests).
pub fn load_all_in(project: &Path, global: Option<&Path>) -> Vec<Steering> {
    let mut out = Vec::new();
    if let Some(global) = global {
        out.extend(load_dir(&global.join("steering")));
    }
    out.extend(load_dir(&project.join(".kiro/steering")));
    out
}

/// Retient `always` + `fileMatch` sur `paths` (relatifs, `/`).
pub fn select<'a>(files: &'a [Steering], paths: &[&str]) -> Vec<&'a Steering> {
    files
        .iter()
        .filter(|file| match file.inclusion {
            Inclusion::Always => true,
            Inclusion::FileMatch => file
                .patterns
                .iter()
                .any(|pat| paths.iter().any(|ctx| crate::kiro::glob::matches(pat, ctx))),
            Inclusion::Manual | Inclusion::Auto => false,
        })
        .collect()
}

/// Corps combinés (nom + corps), prêts à joindre au contexte pinné.
pub fn combined(files: &[Steering], paths: &[&str]) -> Vec<(String, String)> {
    select(files, paths)
        .into_iter()
        .map(|file| (file.name.clone(), file.body.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("kh-kiro-steer-{}-{name}", std::process::id()))
    }

    #[test]
    fn sans_frontmatter_c_est_always() {
        let file = parse("rules", "# Rules\nBe kind.");
        assert_eq!(file.inclusion, Inclusion::Always);
        assert_eq!(file.body, "# Rules\nBe kind.");
        assert!(file.patterns.is_empty());
    }

    #[test]
    fn frontmatter_filematch_str_et_liste() {
        let file = parse(
            "rs",
            "---\ninclusion: fileMatch\nfileMatchPattern: 'src/**/*.rs'\n---\nRust rules.",
        );
        assert_eq!(file.inclusion, Inclusion::FileMatch);
        assert_eq!(file.patterns, vec!["src/**/*.rs"]);
        assert_eq!(file.body, "Rust rules.");
        let file = parse(
            "multi",
            "---\ninclusion: fileMatch\nfileMatchPattern:\n  - a/*.md\n  - \"b/**\"\n---\nBody.",
        );
        assert_eq!(file.patterns, vec!["a/*.md", "b/**"]);
    }

    #[test]
    fn frontmatter_manual_auto_et_inconnu() {
        assert_eq!(
            parse("a", "---\ninclusion: manual\n---\nx").inclusion,
            Inclusion::Manual
        );
        assert_eq!(
            parse("a", "---\ninclusion: AUTO\n---\nx").inclusion,
            Inclusion::Auto
        );
        assert_eq!(
            parse("a", "---\ninclusion: bizarre\n---\nx").inclusion,
            Inclusion::Always
        );
        assert_eq!(Inclusion::Always.as_str(), "always");
        assert_eq!(Inclusion::FileMatch.as_str(), "filematch");
    }

    #[test]
    fn select_ne_garde_que_always_et_filematch_contextuels() {
        let files = vec![
            parse("g", "# global"),
            parse(
                "r",
                "---\ninclusion: fileMatch\nfileMatchPattern: 'src/**'\n---\nRust.",
            ),
            parse("m", "---\ninclusion: manual\n---\nHelp."),
        ];
        let names = |paths: &[&str]| {
            select(&files, paths)
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&[]), vec!["g"]);
        assert_eq!(names(&["src/main.rs"]), vec!["g", "r"]);
        assert_eq!(names(&["README.md"]), vec!["g"]);
    }

    #[test]
    fn load_all_in_fusionne_global_et_projet_tries() {
        let dir = tmp_dir("merge");
        let (global, project) = (dir.join("home/.kiro"), dir.join("proj"));
        std::fs::create_dir_all(global.join("steering")).unwrap();
        std::fs::create_dir_all(project.join(".kiro/steering")).unwrap();
        std::fs::write(global.join("steering/b.md"), "global b").unwrap();
        std::fs::write(project.join(".kiro/steering/a.md"), "project a").unwrap();
        std::fs::write(project.join(".kiro/steering/z.txt"), "ignored").unwrap();
        let files = load_all_in(&project, Some(&global));
        assert_eq!(
            files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            vec!["b", "a"]
        );
        assert!(load_dir(&dir.join("absent")).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
