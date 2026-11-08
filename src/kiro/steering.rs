//! Steering Kiro (`.kiro/steering/*.md`, projet + global).
//!
//! Frontmatter YAML minimal en tête (`inclusion`, `fileMatchPattern`,
//! `name`, `description`) ; sans frontmatter → `always` (défaut Kiro).
//! `select` retient `always` + `fileMatch` sur les chemins en contexte,
//! `find_manual` sert `/steer`, `auto_for` matche les descriptions
//! `auto` à la demande (mots significatifs, pas de sémantique).

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
    pub description: String,
    pub body: String,
}

/// Mots pleins exclus du matching `auto` (courts déjà coupés à 5).
const STOPWORDS: &[&str] = &[
    "cette", "comme", "entre", "avant", "apres", "après", "aussi", "contre", "about", "which",
    "their", "there", "these", "those",
];

/// Parse un fichier (`name` = nom sans extension).
pub fn parse(name: &str, raw: &str) -> Steering {
    let (front, body) = crate::kiro::frontmatter::split(raw);
    let mut file = Steering {
        name: name.to_string(),
        inclusion: Inclusion::Always,
        patterns: Vec::new(),
        description: String::new(),
        body: body.to_string(),
    };
    if let Some(front) = front {
        apply_frontmatter(&mut file, front);
    }
    file
}

/// Applique le frontmatter (lignes `clé: valeur`, listes `- item`).
fn apply_frontmatter(file: &mut Steering, front: &str) {
    use crate::kiro::frontmatter::unquote;
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
                "name" if !value.is_empty() => file.name = value,
                "description" => file.description = value,
                _ if is_pattern_key(&key) && !value.is_empty() => {
                    file.patterns.push(value);
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

/// Charge un dossier (`.md` triés, illisibles ignorés).
pub fn load_dir(dir: &Path) -> Vec<Steering> {
    crate::kiro::frontmatter::load_md_files(dir, parse)
}

/// Charge global puis projet (chemins injectés).
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

/// Fichier `manual` par nom (exact, comme `skills::find`), pour `/steer`.
pub fn find_manual<'a>(files: &'a [Steering], name: &str) -> Option<&'a Steering> {
    files
        .iter()
        .find(|file| file.inclusion == Inclusion::Manual && file.name == name)
}

/// Fichiers `auto` dont la description matche la demande (mots plein
/// communs, heuristique documentée : Kiro fait du sémantique).
pub fn auto_for<'a>(files: &'a [Steering], prompt: &str) -> Vec<&'a Steering> {
    let words = sig_words(prompt);
    if words.is_empty() {
        return Vec::new();
    }
    files
        .iter()
        .filter(|file| {
            file.inclusion == Inclusion::Auto
                && !file.description.trim().is_empty()
                && sig_words(&file.description)
                    .iter()
                    .any(|word| words.contains(word))
        })
        .collect()
}

/// Mots significatifs : minuscules, alphanumériques, 5+ lettres,
/// hors stop-mots (pas de stemming, déterministe et testé).
fn sig_words(text: &str) -> Vec<String> {
    let mut words: Vec<String> = text
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| word.chars().count() >= 5 && !STOPWORDS.contains(word))
        .map(str::to_string)
        .collect();
    words.sort_unstable();
    words.dedup();
    words
}

/// Insère les blocs `auto` après le run `system` de tête (pinné puis
/// consigne questions, sinon consigne seule) — éphémère, jamais
/// l'historique stocké.
pub fn prefix_auto(wire: &mut Vec<(String, String)>, blocks: &[(String, String)]) {
    prefix_tagged(wire, "steering", blocks);
}

/// Insère des blocs taggés `[tag:nom]` après le run `system` de
/// tête (générique : steering `auto`, powers activés).
pub fn prefix_tagged(wire: &mut Vec<(String, String)>, tag: &str, blocks: &[(String, String)]) {
    if blocks.is_empty() {
        return;
    }
    let at = wire.iter().take_while(|(role, _)| role == "system").count();
    let joined = blocks
        .iter()
        .map(|(name, body)| format!("[{tag}:{name}]\n{body}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    wire.insert(at, (String::from("system"), joined));
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
    fn frontmatter_nom_et_description() {
        let file = parse(
            "fichier",
            "---\nname: joli\ninclusion: auto\ndescription: Règles Rust.\n---\nCorps.",
        );
        assert_eq!(file.name, "joli");
        assert_eq!(file.inclusion, Inclusion::Auto);
        assert_eq!(file.description, "Règles Rust.");
        let vide = parse("v", "---\ndescription:\n---\nx");
        assert!(vide.description.is_empty());
    }

    #[test]
    fn find_manual_exact_et_manuels_seuls() {
        let files = vec![
            parse("m", "---\ninclusion: manual\n---\nHelp."),
            parse("g", "# global"),
        ];
        assert!(find_manual(&files, "m").is_some());
        assert!(find_manual(&files, "g").is_none());
        assert!(find_manual(&files, "M").is_none());
        assert!(find_manual(&files, "x").is_none());
    }

    #[test]
    fn auto_for_mots_pleins_communs() {
        let files = vec![
            parse(
                "r",
                "---\ninclusion: auto\ndescription: Règles du borrow-checker Rust.\n---\nCorps.",
            ),
            parse(
                "s",
                "---\ninclusion: auto\ndescription: Recettes de crêpes.\n---\nMiam.",
            ),
            parse("vide", "---\ninclusion: auto\n---\nSans description."),
            parse(
                "m",
                "---\ninclusion: manual\ndescription: borrow-checker partout.\n---\nNon.",
            ),
        ];
        let names = |prompt: &str| {
            auto_for(&files, prompt)
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(names("explique le borrow checker"), vec!["r"]);
        assert_eq!(names("bonjour"), Vec::<&str>::new());
        assert_eq!(names("des crêpes s'il vous plaît"), vec!["s"]);
    }

    #[test]
    fn prefix_auto_apres_run_system() {
        let sys = |t: &str| (String::from("system"), t.to_string());
        let usr = |t: &str| (String::from("user"), t.to_string());
        let blocks = vec![(String::from("r"), String::from("Corps."))];
        let mut wire = vec![sys("pin"), sys("?"), usr("hi")];
        prefix_auto(&mut wire, &blocks);
        assert_eq!(wire[2], sys("[steering:r]\nCorps."));
        assert_eq!(wire.len(), 4);
        let mut seul = vec![usr("hi")];
        prefix_auto(&mut seul, &blocks);
        assert_eq!(seul[0].0, "system");
        let mut vide = vec![usr("hi")];
        prefix_auto(&mut vide, &[]);
        assert_eq!(vide.len(), 1);
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
