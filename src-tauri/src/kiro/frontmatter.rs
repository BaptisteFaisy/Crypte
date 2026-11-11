//! Frontmatter YAML minimal (`---` en tête), partagé par les lecteurs `.kiro/`.
//!
//! Steering, agents customs et skills parsent le même en-tête : ce module
//! porte la seule implémentation du découpage et des valeurs scalaires
//! (`clé: valeur`, une par ligne). Les listes (`- item`) restent locales
//! au steering (seul à en avoir besoin).

/// Sépare frontmatter (`---` en tête) et corps.
pub fn split(raw: &str) -> (Option<&str>, &str) {
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

/// Valeur scalaire d'une clé (`None` si absente) : lignes `clé: valeur`,
/// clé insensible à la casse, valeur déquotée, listes et commentaires ignorés.
pub fn value(front: &str, key: &str) -> Option<String> {
    for line in front.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
            continue;
        }
        if let Some((name, raw)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case(key) {
                return Some(unquote(raw.trim()));
            }
        }
    }
    None
}

/// Charge les `.md` d'un dossier (triés, illisibles ignorés), chacun parsé
/// par `parse` (tige du fichier + contenu) — seul parcours de ce type.
pub fn load_md_files<T>(dir: &std::path::Path, parse: impl Fn(&str, &str) -> T) -> Vec<T> {
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

/// Retire les guillemets simples/doubles autour d'une valeur.
pub fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_coupe_frontmatter_et_corps() {
        let (front, body) = split("---\nname: x\n---\nBody.");
        assert_eq!(front.unwrap().trim(), "name: x");
        assert_eq!(body, "Body.");
        let (front, body) = split("# sans frontmatter");
        assert!(front.is_none());
        assert_eq!(body, "# sans frontmatter");
        // `---` hors tête : pas de frontmatter.
        let (front, _) = split("texte\n---\nname: x\n---\nBody.");
        assert!(front.is_none());
    }

    #[test]
    fn value_lit_scalaires_ignore_listes_et_commentaires() {
        let front = "# gloss\nname: \"bob\"\nlist:\n- item\ndescription: aide";
        assert_eq!(value(front, "name").as_deref(), Some("bob"));
        assert_eq!(value(front, "DESCRIPTION").as_deref(), Some("aide"));
        assert_eq!(value(front, "list").as_deref(), Some(""));
        assert_eq!(value(front, "absent"), None);
    }

    #[test]
    fn load_md_files_trie_et_ignore_non_md() {
        let dir = std::env::temp_dir().join(format!("kh-front-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.md"), "B").unwrap();
        std::fs::write(dir.join("a.md"), "A").unwrap();
        std::fs::write(dir.join("z.txt"), "Z").unwrap();
        let got = load_md_files(&dir, |stem, raw| format!("{stem}={raw}"));
        assert_eq!(got, vec!["a=A", "b=B"]);
        assert!(load_md_files::<String>(&dir.join("absent"), |s, r| format!("{s}{r}")).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unquote_retire_guillemets_apparies() {
        assert_eq!(unquote("'a'"), "a");
        assert_eq!(unquote("\"b\""), "b");
        assert_eq!(unquote("c"), "c");
        assert_eq!(unquote("'mixte\""), "'mixte\"");
    }
}
