//! Questions de l'IA au user : bloc ```question, pastille jaune.
//!
//! Port de `question.rs` du TUI (pur, sans état) + panneau navigable
//! (#104) : l'IA écrit un bloc ```question (2 à 4 options), le backend
//! le détecte, le montre rendu (`render_display`) et mémorise la
//! question en attente (pastille jaune, curseur `step`/`selected`,
//! style zerocode) ; l'utilisateur répond ↑↓ + Entrée, par le chiffre
//! (`resolve_answer`) ou en mots libres. `QUESTION_SYSTEM` enseigne le
//! format au modèle (injecté dans chaque requête, voir `history`).

/// Instructions envoyées au modèle (une question à choix par réponse).
pub const QUESTION_SYSTEM: &str = "To ask the user a real multiple-choice question, \
write a ```question block (2 to 4 short options):\n\
```question\n<question in one sentence>\n1. <option>\n2. <option>\n```\n\
One question per reply, free text around it allowed. \
Otherwise answer normally without this block.";

/// Options acceptées (le prompt dit 2 à 4, jusqu'à 6 tolérées).
const MIN_OPTIONS: usize = 2;
const MAX_OPTIONS: usize = 6;

/// Question en attente de réponse (un chat = au plus une).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingQuestion {
    pub question: String,
    pub options: Vec<String>,
    /// Highlighted option in the panel (zerocode style).
    pub cursor: usize,
}

impl PendingQuestion {
    /// Moves the cursor by `delta` options (clamped, zerocode style).
    pub fn step(&mut self, delta: isize) {
        let last = self.options.len().saturating_sub(1) as isize;
        let at = self.cursor as isize + delta;
        self.cursor = at.clamp(0, last.max(0)) as usize;
    }

    /// Label of the highlighted option (`""` when none).
    pub fn selected(&self) -> &str {
        self.options.get(self.cursor).map_or("", String::as_str)
    }
}

/// Texte affiché (premier bloc valide remplacé par son rendu) +
/// question en attente (`None` si aucun bloc valide, texte inchangé).
pub fn render_display(text: &str) -> (String, Option<PendingQuestion>) {
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if !is_start(lines[i]) {
            i += 1;
            continue;
        }
        let mut end = i + 1;
        while end < lines.len() && !is_end(lines[end]) {
            end += 1;
        }
        let closed = end < lines.len();
        if let Some(found) = parse_block(&lines[i + 1..end].join("\n")) {
            let mut out: Vec<String> = lines[..i].iter().map(|s| s.to_string()).collect();
            out.push(render(&found));
            if closed {
                out.extend(lines[end + 1..].iter().map(|s| s.to_string()));
            }
            return (out.join("\n"), Some(found));
        }
        i = if closed { end + 1 } else { lines.len() };
    }
    (text.to_string(), None)
}

/// Rendu d'une question : intro, options numérotées, rappel de réponse.
pub fn render(q: &PendingQuestion) -> String {
    let mut out = format!("— the crypt asks you: {}\n", q.question);
    for (i, opt) in q.options.iter().enumerate() {
        out.push_str(&format!("  {}) {}\n", i + 1, opt));
    }
    out.push_str("  (↑↓ + Enter, number or words)");
    out
}

/// Chiffre (`1`, `2.`…) → libellé de l'option ; autre texte → `None`
/// (réponse libre, traitée normalement).
pub fn resolve_answer(q: &PendingQuestion, input: &str) -> Option<String> {
    let cleaned = input.trim().trim_end_matches(['.', ')', ':']).trim();
    let n: usize = cleaned.parse().ok()?;
    q.options.get(n.wrapping_sub(1)).cloned()
}

/// Préfixe la consigne des questions (après la tête `system`, sinon en tête).
/// Appelé par `launch_agent` (un seul site d'appel dans le TUI).
pub fn prefix(wire: &mut Vec<(String, String)>) {
    let head_system = wire.first().is_some_and(|(role, _)| role == "system");
    let at = usize::from(head_system && !wire.is_empty());
    wire.insert(at, (String::from("system"), QUESTION_SYSTEM.to_string()));
}

/// Ligne d'ouverture : ``` + `question` (casse et espaces tolérées).
fn is_start(line: &str) -> bool {
    line.trim_start()
        .strip_prefix("```")
        .is_some_and(|rest| rest.trim().eq_ignore_ascii_case("question"))
}

/// Ligne de fermeture : n'importe quel ``` (le bloc peut rester ouvert).
fn is_end(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

/// Contenu du bloc : JSON (`{"question","options"}`) ou lignes
/// (question puis options à puces) ; 2 à 6 options sinon `None`.
fn parse_block(content: &str) -> Option<PendingQuestion> {
    let trimmed = content.trim();
    if trimmed.starts_with('{') {
        if let Some(found) = parse_json(trimmed) {
            return Some(found);
        }
    }
    let mut lines = trimmed.lines().map(str::trim).filter(|l| !l.is_empty());
    let question = clean_question(lines.next()?);
    let options: Vec<String> = lines.map(clean_option).filter(|o| !o.is_empty()).collect();
    if question.is_empty() || !(MIN_OPTIONS..=MAX_OPTIONS).contains(&options.len()) {
        return None;
    }
    Some(PendingQuestion {
        question,
        options,
        cursor: 0,
    })
}

/// Variante JSON tolérée : `question` (chaîne) + `options`/`choices`.
fn parse_json(content: &str) -> Option<PendingQuestion> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;
    let question = value.get("question")?.as_str()?.trim();
    let items = value
        .get("options")
        .or_else(|| value.get("choices"))?
        .as_array()?;
    let options: Vec<String> = items
        .iter()
        .filter_map(|item| item.as_str())
        .map(|opt| opt.trim().to_string())
        .filter(|opt| !opt.is_empty())
        .collect();
    if question.is_empty() || !(MIN_OPTIONS..=MAX_OPTIONS).contains(&options.len()) {
        return None;
    }
    Some(PendingQuestion {
        question: question.to_string(),
        options,
        cursor: 0,
    })
}

/// Première ligne : préfixe `Question:` éventuel retiré (jamais de
/// panique sur coupes non-frontières : `get`, jamais d'index brut).
fn clean_question(line: &str) -> String {
    let trimmed = line.trim();
    if let Some(head) = trimmed.get(..8) {
        if head.eq_ignore_ascii_case("question") {
            let rest = trimmed
                .get(8..)
                .unwrap_or("")
                .trim_start_matches([' ', '\t', ':', '-', '–', '—'])
                .trim();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    trimmed.to_string()
}

/// Ligne d'option : puce retirée (`1.`, `2)`, `-`, `*`, `•`…).
/// Chiffres ASCII : coupe sûre (`t[digits..]`).
fn clean_option(line: &str) -> String {
    let mut text = line.trim();
    let digits = text.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        text = text[digits..].trim_start();
        text = text
            .strip_prefix(['.', ')', ':', '-', '–', '—'])
            .map(str::trim_start)
            .unwrap_or(text);
    }
    text = text
        .strip_prefix(['-', '*', '•', '>', '→'])
        .map(str::trim_start)
        .unwrap_or(text);
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Question d'exemple (deux options).
    fn sample() -> PendingQuestion {
        PendingQuestion {
            question: String::from("Shall we go on?"),
            options: vec![String::from("Yes"), String::from("No")],
            cursor: 0,
        }
    }

    /// Question détectée dans `text` (via le rendu d'affichage).
    fn asked(text: &str) -> Option<PendingQuestion> {
        render_display(text).1
    }

    #[test]
    fn detects_valid_block_with_prefix_and_mixed_bullets() {
        let text =
            "blabla\n```question\nQuestion: Shall we go on?\n1. Yes\n- No\n* Maybe\n```\nend";
        let found = asked(text).unwrap();
        assert_eq!(found.question, "Shall we go on?");
        assert_eq!(found.options, ["Yes", "No", "Maybe"]);
    }

    #[test]
    fn detects_skips_invalid_and_keeps_valid() {
        let bad = "```question\nLonely?\n1. only\n```";
        let good = "```question\nGood?\n1. a\n2. b\n```";
        assert!(asked(bad).is_none(), "one option: invalid");
        assert!(asked("```question\nNo options\n```").is_none());
        let both = format!("{bad}\n{good}");
        assert_eq!(asked(&both).unwrap().question, "Good?");
    }

    #[test]
    fn detects_tolerates_unclosed_json_and_bounds() {
        let open = "```question\nOpen?\n1. a\n2. b";
        assert_eq!(asked(open).unwrap().options.len(), 2);
        let json = "```question\n{\"question\": \"J?\", \"options\": [\"x\", \"y\"]}\n```";
        assert_eq!(
            asked(json).unwrap(),
            PendingQuestion {
                question: String::from("J?"),
                options: vec![String::from("x"), String::from("y")],
                cursor: 0,
            }
        );
        let seven = (1..=7)
            .map(|i| format!("{i}. o{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(asked(&format!("```question\nQ?\n{seven}\n```")).is_none());
        assert!(asked("```rust\nlet x = 1;\n```").is_none());
    }

    #[test]
    fn render_display_replaces_block_and_keeps_rest() {
        let (shown, found) =
            render_display("before\n```question\nShall we go on?\n1. Yes\n2. No\n```\nafter");
        assert_eq!(found.unwrap(), sample());
        assert!(
            shown.starts_with("before\n— the crypt asks you: Shall we go on?\n  1) Yes\n  2) No")
        );
        assert!(shown.ends_with("after"));
        assert!(!shown.contains("```"));
        let (same, none) = render_display("no block");
        assert_eq!(same, "no block");
        assert!(none.is_none());
    }

    #[test]
    fn resolve_answer_digit_or_free() {
        let q = sample();
        assert_eq!(resolve_answer(&q, "1"), Some(String::from("Yes")));
        assert_eq!(resolve_answer(&q, "  2. "), Some(String::from("No")));
        assert_eq!(resolve_answer(&q, "2)"), Some(String::from("No")));
        assert!(resolve_answer(&q, "0").is_none());
        assert!(resolve_answer(&q, "3").is_none());
        assert!(resolve_answer(&q, "maybe tomorrow").is_none());
        assert!(resolve_answer(&q, "").is_none());
    }

    #[test]
    fn cursor_clamped_and_selection_follows() {
        let mut q = sample();
        assert_eq!(q.selected(), "Yes");
        q.step(-1);
        assert_eq!(q.cursor, 0, "up at top: clamped");
        q.step(1);
        assert_eq!(q.selected(), "No");
        q.step(9);
        assert_eq!(q.cursor, 1, "down at bottom: clamped");
        q.step(-9);
        assert_eq!(q.cursor, 0);
    }

    #[test]
    fn prefix_inserts_after_system_head_else_first() {
        let mut headed = vec![
            (String::from("system"), String::from("pin")),
            (String::from("user"), String::from("hi")),
        ];
        prefix(&mut headed);
        assert_eq!(headed[0].1, "pin");
        assert_eq!(headed[1].1, QUESTION_SYSTEM);
        let mut bare = vec![(String::from("user"), String::from("hi"))];
        prefix(&mut bare);
        assert_eq!(bare[0].1, QUESTION_SYSTEM);
    }

    #[test]
    fn system_prompt_teaches_the_format() {
        assert!(QUESTION_SYSTEM.contains("```question"));
        assert!(QUESTION_SYSTEM.contains("2 to 4"));
    }
}
