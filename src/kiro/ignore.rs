//! `.kiroignore` racine projet (motifs `glob`, `!` = négation).
//!
//! T1 : chargement + compte (statut `/kiro`). Le filtrage effectif
//! arrive en T3 avec les déclencheurs fichiers des hooks.

use std::path::Path;

/// Motifs chargés (ordre du fichier, `bool` = négation).
#[derive(Debug, Clone, Default)]
pub struct Ignore {
    patterns: Vec<(bool, String)>,
}

/// Charge `.kiroignore` (absent = vide, `#` = commentaire).
pub fn load(project: &Path) -> Ignore {
    let mut patterns = Vec::new();
    let Ok(raw) = std::fs::read_to_string(project.join(".kiroignore")) else {
        return Ignore { patterns };
    };
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line.strip_prefix('!') {
            Some(rest) => patterns.push((true, rest.to_string())),
            None => patterns.push((false, line.to_string())),
        }
    }
    Ignore { patterns }
}

impl Ignore {
    /// Nombre de motifs (statut `/kiro`).
    pub fn len(&self) -> usize {
        self.patterns.len()
    }

    /// Aucun motif ?
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Vrai si `path` (relatif projet, `/`) est ignoré : dernier motif
    /// qui matche gagne (`!` = ré-inclut). `dir/` vaut `dir/**`.
    pub fn is_ignored(&self, path: &str) -> bool {
        let mut ignored = false;
        for (negated, pattern) in &self.patterns {
            let full;
            let candidate = if pattern.ends_with('/') {
                full = format!("{pattern}**");
                full.as_str()
            } else {
                pattern.as_str()
            };
            if super::glob::matches(candidate, path) {
                ignored = !negated;
            }
        }
        ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charge_motifs_ignore_commentaires_et_vides() {
        let dir = std::env::temp_dir().join(format!("kh-kiro-ignore-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".kiroignore"), "# deps\ntarget/\n\n!target/keep\n").unwrap();
        let ignore = load(&dir);
        assert_eq!(ignore.len(), 2);
        assert!(!ignore.is_empty());
        assert!(load(&dir.join("absent")).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn is_ignored_dernier_motif_gagne_negation_reinclut() {
        let dir = std::env::temp_dir().join(format!("kh-kiro-ignore-{}-hit", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".kiroignore"), "target/\n*.log\n!keep.log\n").unwrap();
        let ignore = load(&dir);
        assert!(ignore.is_ignored("target/out.rs"));
        assert!(ignore.is_ignored("target/nested/deep.txt"));
        assert!(ignore.is_ignored("debug.log"));
        assert!(!ignore.is_ignored("keep.log"));
        assert!(!ignore.is_ignored("src/main.rs"));
        assert!(!ignore.is_ignored("src/target.rs"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
