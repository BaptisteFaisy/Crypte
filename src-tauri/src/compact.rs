//! Compaction d'historique (`chat_compact`).
//!
//! Constantes et pliage repris du TUI (`cmds.rs` + `agents.rs`) : seuil,
//! consigne de résumé, `[pinné?]` + `[résumé]` + 4 derniers.

/// Historique minimal pour compacter (en dessous : « nothing to compact »).
pub const COMPACT_MIN: usize = 6;
/// Derniers messages gardés (hors pinné) après compaction.
pub const COMPACT_KEPT: usize = 4;
/// Consigne de résumé envoyée au modèle.
pub const COMPACT_INSTRUCTION: &str = "Summarize this conversation in under 20 lines: established facts, decisions made, useful context for what follows. Reply with only the summary, no introduction.";

/// Historique sans l'entrée pinnée (requête de résumé méta).
pub fn without_pinned(history: &[(String, String)], pinned: bool) -> &[(String, String)] {
    if pinned {
        history.get(1..).unwrap_or(&[])
    } else {
        history
    }
}

/// Historique compacté : `[pinné?]` + `[résumé]` + 4 derniers (hors pinné).
pub fn compacted_history(
    history: &[(String, String)],
    summary: &str,
    pinned: bool,
) -> Vec<(String, String)> {
    let mut out = Vec::with_capacity(COMPACT_KEPT + 2);
    if pinned {
        if let Some(first) = history.first() {
            out.push(first.clone());
        }
    }
    out.push((
        String::from("system"),
        format!("Compacted summary: {summary}"),
    ));
    out.extend(
        without_pinned(history, pinned)
            .iter()
            .rev()
            .take(COMPACT_KEPT)
            .rev()
            .cloned(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Entrée d'historique courte (tests).
    fn msg(role: &str, text: &str) -> (String, String) {
        (role.to_string(), text.to_string())
    }

    #[test]
    fn compacted_history_garde_pinne_resume_et_4_derniers() {
        let mut history = vec![msg("system", "pin")];
        history.extend((0..10).map(|i| msg("user", &format!("m{i}"))));
        let out = compacted_history(&history, "tout va", true);
        assert_eq!(out.len(), 1 + 1 + COMPACT_KEPT);
        assert_eq!(out[0], msg("system", "pin"));
        assert_eq!(out[1].1, "Compacted summary: tout va");
        assert_eq!(out[2].1, "m6");
        assert_eq!(out[5].1, "m9");
        let plain: Vec<_> = (0..10).map(|i| msg("user", &format!("m{i}"))).collect();
        let out = compacted_history(&plain, "tout va", false);
        assert_eq!(out.len(), 1 + COMPACT_KEPT);
        assert_eq!(out[0].1, "Compacted summary: tout va");
        assert_eq!(without_pinned(&history, true).len(), 10);
        assert_eq!(without_pinned(&plain, false).len(), 10);
    }
}
