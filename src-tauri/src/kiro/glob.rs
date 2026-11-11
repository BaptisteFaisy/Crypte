//! Glob minimal (`*`, `**`, `?`) pour `fileMatch` et `.kiroignore`.
//!
//! Pas de dep `glob` : `*` ne traverse pas `/`, `**` oui, `?` vaut un
//! caractère (jamais `/`). Chemins normalisés (`\` → `/`, `./` retiré).

/// Vrai si `path` matche `pattern`.
pub fn matches(pattern: &str, path: &str) -> bool {
    match_segments(&split(pattern), &split(path))
}

/// Découpe normalisée (séparateurs unifiés, vides et `.` retirés).
fn split(s: &str) -> Vec<String> {
    s.replace('\\', "/")
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .map(str::to_string)
        .collect()
}

/// Matche segment par segment (`**` = 0..n segments).
fn match_segments(pat: &[String], segs: &[String]) -> bool {
    match pat.first() {
        None => segs.is_empty(),
        Some(first) if first == "**" => {
            (0..=segs.len()).any(|skip| match_segments(&pat[1..], &segs[skip..]))
        }
        Some(first) => match segs.first() {
            Some(head) => match_segment(first, head) && match_segments(&pat[1..], &segs[1..]),
            None => false,
        },
    }
}

/// Matche un segment (`*` = 0..n caractères, `?` = 1 caractère).
fn match_segment(pat: &str, text: &str) -> bool {
    let pat: Vec<char> = pat.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < text.len() {
        if pi < pat.len() && (pat[pi] == '?' || pat[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == '*' {
            star = Some(pi);
            pi += 1;
            mark = ti;
        } else if let Some(pos) = star {
            pi = pos + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < pat.len() && pat[pi] == '*' {
        pi += 1;
    }
    pi == pat.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etoile_ne_traverse_pas_le_slash() {
        assert!(matches("*.md", "notes.md"));
        assert!(!matches("*.md", "docs/notes.md"));
        assert!(matches("src/*.rs", "src/main.rs"));
        assert!(!matches("src/*.rs", "src/kiro/mod.rs"));
    }

    #[test]
    fn double_etoile_traverse_les_dossiers() {
        assert!(matches("src/**/*.rs", "src/main.rs"));
        assert!(matches("src/**/*.rs", "src/kiro/mod.rs"));
        assert!(matches("**/*.md", "notes.md"));
        assert!(matches("**/*.md", "a/b/notes.md"));
        assert!(!matches("src/**/*.rs", "src/main.md"));
    }

    #[test]
    fn point_d_interrogation_vaut_un_caractere() {
        assert!(matches("test?.rs", "test1.rs"));
        assert!(!matches("test?.rs", "test12.rs"));
        assert!(!matches("?.md", "a/b.md"));
    }

    #[test]
    fn normalise_separateurs_et_points() {
        assert!(matches("src/*.rs", "src\\main.rs"));
        assert!(matches("./*.md", "notes.md"));
        assert!(matches("docs", "docs"));
        assert!(!matches("", "x"));
    }
}
