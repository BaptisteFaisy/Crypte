//! Mini-moteur regex pour le champ `matcher` des hooks Kiro.
//!
//! Kiro matche en regex (`\.java$`, `Test\.java$`) : sans dep `regex`,
//! ce module couvre l'utile — échappement `\`, `.`, `*`, `+`, `?`,
//! ancres `^`/`$`, classes `\d \w \s` (+ `\D \W \S`). Sémantique
//! recherche (comme `is_match`) : le motif peut matcher n'importe où,
//! sauf ancré par `^`/`$`. Sensible à la casse, comme Kiro.

/// Atome matchant un caractère.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Atom {
    Lit(char),
    Dot,
    Digit,
    Word,
    Space,
    NotDigit,
    NotWord,
    NotSpace,
}

/// Quantificateur d'un atome (`One` = exact un).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quant {
    One,
    Star,
    Plus,
    Opt,
}

/// Atome quantifié (unité du motif parsé).
#[derive(Debug, Clone, Copy)]
struct Item {
    atom: Atom,
    quant: Quant,
}

/// Vrai si `pattern` matche `text` (recherche, ancres `^`/`$`).
/// Motif vide = matche tout ; motif invalide = lent (littéral).
pub fn matches(pattern: &str, text: &str) -> bool {
    let (items, start, end) = parse(pattern);
    let chars: Vec<char> = text.chars().collect();
    if start {
        return match_rest(&items, 0, &chars, 0, end);
    }
    (0..=chars.len()).any(|at| match_rest(&items, 0, &chars, at, end))
}

/// Parse le motif : items + ancres (`^` initial, `$` final non échappés).
fn parse(pattern: &str) -> (Vec<Item>, bool, bool) {
    let raw: Vec<char> = pattern.chars().collect();
    let mut items = Vec::new();
    let mut start = false;
    let mut end = false;
    let mut chars = raw.as_slice();
    if chars.first() == Some(&'^') {
        start = true;
        chars = &chars[1..];
    }
    let mut idx = 0;
    while idx < chars.len() {
        let chr = chars[idx];
        if chr == '\\' && idx + 1 < chars.len() {
            push_atom(&mut items, class_or_lit(chars[idx + 1]));
            idx += 2;
        } else if chr == '\\' {
            push_atom(&mut items, Atom::Lit('\\'));
            idx += 1;
        } else if chr == '.' {
            push_atom(&mut items, Atom::Dot);
            idx += 1;
        } else if matches!(chr, '*' | '+' | '?') && !items.is_empty() {
            let quant = match chr {
                '*' => Quant::Star,
                '+' => Quant::Plus,
                _ => Quant::Opt,
            };
            items.last_mut().expect("testé non vide").quant = quant;
            idx += 1;
        } else if chr == '$' && idx + 1 == chars.len() {
            end = true;
            idx += 1;
        } else {
            push_atom(&mut items, Atom::Lit(chr));
            idx += 1;
        }
    }
    (items, start, end)
}

/// Empile un atome (quantificateur `One`, écrasé ensuite si besoin).
fn push_atom(items: &mut Vec<Item>, atom: Atom) {
    items.push(Item {
        atom,
        quant: Quant::One,
    });
}

/// Classe `\d \w \s \D \W \S`, sinon littéral échappé (`\.`, `\*`…).
fn class_or_lit(chr: char) -> Atom {
    match chr {
        'd' => Atom::Digit,
        'w' => Atom::Word,
        's' => Atom::Space,
        'D' => Atom::NotDigit,
        'W' => Atom::NotWord,
        'S' => Atom::NotSpace,
        other => Atom::Lit(other),
    }
}

/// Vrai si les items `[nth..]` matchent en préfixe à `pos` (`end` =
/// exiger la fin de texte à l'épuisement, ancre `$`). Backtracking :
/// les quantificateurs essaient du plus glouton au plus court.
fn match_rest(items: &[Item], nth: usize, chars: &[char], pos: usize, end: bool) -> bool {
    let Some(item) = items.get(nth) else {
        return !end || pos == chars.len();
    };
    match item.quant {
        Quant::One => {
            pos < chars.len()
                && atom_hit(item.atom, chars[pos])
                && match_rest(items, nth + 1, chars, pos + 1, end)
        }
        Quant::Opt => {
            match_rest(items, nth + 1, chars, pos, end)
                || (pos < chars.len()
                    && atom_hit(item.atom, chars[pos])
                    && match_rest(items, nth + 1, chars, pos + 1, end))
        }
        Quant::Star => match_repeat(items, nth, chars, pos, end, 0),
        Quant::Plus => match_repeat(items, nth, chars, pos, end, 1),
    }
}

/// Répétition gloutonne (`min` 0 = `*`, 1 = `+`) puis suite, en
/// redescendant jusqu'au plus court (backtracking borné par le texte).
fn match_repeat(
    items: &[Item],
    nth: usize,
    chars: &[char],
    pos: usize,
    end: bool,
    min: usize,
) -> bool {
    let item = items[nth];
    let mut run = 0;
    while pos + run < chars.len() && atom_hit(item.atom, chars[pos + run]) {
        run += 1;
    }
    (min..=run)
        .rev()
        .any(|take| match_rest(items, nth + 1, chars, pos + take, end))
}

/// Vrai si l'atome matche le caractère (`\w` = alnum + `_`, ASCII).
fn atom_hit(atom: Atom, chr: char) -> bool {
    match atom {
        Atom::Lit(want) => chr == want,
        Atom::Dot => true,
        Atom::Digit => chr.is_ascii_digit(),
        Atom::Word => chr.is_ascii_alphanumeric() || chr == '_',
        Atom::Space => chr.is_whitespace(),
        Atom::NotDigit => !chr.is_ascii_digit(),
        Atom::NotWord => !(chr.is_ascii_alphanumeric() || chr == '_'),
        Atom::NotSpace => !chr.is_whitespace(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exemples_kiro_java() {
        assert!(matches("\\.java$", "Test.java"));
        assert!(matches("\\.java$", "src/com/Test.java"));
        assert!(!matches("\\.java$", "Test.javax"));
        assert!(!matches("\\.java$", "Testjava"));
        assert!(matches("Test\\.java$", "src/Test.java"));
        assert!(!matches("Test\\.java$", "src/Best.java"));
        assert!(matches("\\.rs$", "src/main.rs"));
        assert!(!matches("\\.rs$", "src/main.rs.bak"));
    }

    #[test]
    fn ancres_debut_et_fin() {
        assert!(matches("^src/", "src/main.rs"));
        assert!(!matches("^src/", "xsrc/main.rs"));
        assert!(matches("^main$", "main"));
        assert!(!matches("^main$", "main2"));
        assert!(!matches("^main$", "2main"));
        assert!(matches("main", "domain main x"));
    }

    #[test]
    fn quantificateurs_etoile_plus_point() {
        assert!(matches("a*b", "aaab"));
        assert!(matches("a*b", "b"));
        assert!(!matches("a+b", "b"));
        assert!(matches("a+b", "aaab"));
        assert!(matches("colou?r", "color"));
        assert!(matches("colou?r", "colour"));
        assert!(!matches("colou?r", "colouur"));
        assert!(matches(".*\\.md$", "a/b/notes.md"));
        assert!(!matches(".*\\.md$", "a/b/notes.mdx"));
    }

    #[test]
    fn classes_d_w_s() {
        assert!(matches("\\d+", "abc123"));
        assert!(!matches("^\\d+$", "abc123"));
        assert!(matches("^\\d+$", "123"));
        assert!(matches("\\w+\\.rs$", "main.rs"));
        assert!(matches("\\w+\\.rs$", "my-file.rs"));
        assert!(!matches("^\\w+\\.rs$", "my-file.rs"));
        assert!(matches("a\\sb", "a b"));
        assert!(!matches("a\\sb", "ab"));
        assert!(matches("^\\S+$", "abc"));
        assert!(!matches("^\\S+$", "a c"));
    }

    #[test]
    fn echappements_litteraux() {
        assert!(matches("a\\.b", "a.b"));
        assert!(!matches("a\\.b", "axb"));
        assert!(matches("a\\*b", "a*b"));
        assert!(matches("\\\\", "a\\b"));
        assert!(matches("100\\$", "100$"));
        assert!(!matches("100$", "100$x"));
    }

    #[test]
    fn cas_limites_vides() {
        assert!(matches("", "n'importe quoi"));
        assert!(matches("a*", ""));
        assert!(matches("^$", ""));
        assert!(!matches("^$", "x"));
        assert!(!matches("z+", ""));
        assert!(matches("?", "?"));
    }
}
