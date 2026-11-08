//! AI questions to the user: ```question block, yellow pill.
//!
//! The AI asks a multiple-choice question by writing a ` ```question `
//! block (2 to 4 short options, one question per reply). The TUI detects
//! it and shows it rendered (`render_display`), then remembers the
//! pending question: the chat pill turns candle yellow and the user
//! answers with the digit (`resolve_answer`) or in free words — any
//! sent message answers. `QUESTION_SYSTEM` teaches the format to the
//! model (injected into every request, see `history`).

/// Instructions sent to the model (one multiple-choice question per reply).
pub const QUESTION_SYSTEM: &str = "To ask the user a real multiple-choice question, \
write a ```question block (2 to 4 short options):\n\
```question\n<question in one sentence>\n1. <option>\n2. <option>\n```\n\
One question per reply, free text around it allowed. \
Otherwise answer normally without this block.";

/// Accepted options (the prompt says 2 to 4, up to 6 tolerated).
const MIN_OPTIONS: usize = 2;
const MAX_OPTIONS: usize = 6;

/// Question waiting for an answer (one chat = at most one).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingQuestion {
    pub question: String,
    pub options: Vec<String>,
}

/// Display text (first valid block replaced by its render) +
/// pending question (`None` if no valid block, text unchanged).
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

/// Render of a question: intro, numbered options, reply reminder.
pub fn render(q: &PendingQuestion) -> String {
    let mut out = format!("— the crypt asks you: {}\n", q.question);
    for (i, opt) in q.options.iter().enumerate() {
        out.push_str(&format!("  {}) {}\n", i + 1, opt));
    }
    out.push_str("  (reply with the number or in your own words)");
    out
}

/// Digit (`1`, `2.`…) → option label ; other text → `None`
/// (free reply, handled normally).
pub fn resolve_answer(q: &PendingQuestion, input: &str) -> Option<String> {
    let cleaned = input.trim().trim_end_matches(['.', ')', ':']).trim();
    let n: usize = cleaned.parse().ok()?;
    q.options.get(n.wrapping_sub(1)).cloned()
}

/// Prefixes the question instructions (after the `system` head, else first).
/// Single call site helper for agent wires (`launch_agent`).
pub fn prefix(wire: &mut Vec<(String, String)>) {
    let head_system = wire.first().is_some_and(|(role, _)| role == "system");
    let at = usize::from(head_system && !wire.is_empty());
    wire.insert(at, (String::from("system"), QUESTION_SYSTEM.to_string()));
}

/// Opening line: ``` + `question` (case and spaces tolerated).
fn is_start(line: &str) -> bool {
    line.trim_start()
        .strip_prefix("```")
        .is_some_and(|rest| rest.trim().eq_ignore_ascii_case("question"))
}

/// Closing line: any ``` (the block may stay open).
fn is_end(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

/// Block content: JSON (`{"question","options"}`) or lines
/// (question then bulleted options) ; 2 to 6 options else `None`.
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
    Some(PendingQuestion { question, options })
}

/// Tolerated JSON variant: `question` (string) + `options`/`choices`.
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
    })
}

/// First line: optional `Question:` prefix removed (never panics
/// on non-boundary cuts: `get`, never a raw index).
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

/// Option line: bullet removed (`1.`, `2)`, `-`, `*`, `•`…).
/// ASCII digits guarantee a safe cut (`t[digits..]`).
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

    /// Sample question (two options).
    fn sample() -> PendingQuestion {
        PendingQuestion {
            question: String::from("Shall we go on?"),
            options: vec![String::from("Yes"), String::from("No")],
        }
    }

    /// Question detected in `text` (via the display render).
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
