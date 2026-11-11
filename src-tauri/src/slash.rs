//! Miettes du parseur slash : garde d'envoi + niveaux d'effort.
//!
//! Port du strict nécessaire de `commands/mod.rs` du TUI : `chat_send`
//! refuse le texte slash (les actions UI passent par leurs commandes),
//! `chat_set_thinking` valide contre `valid_effort`.

/// Vrai si la saisie est une commande (commence par `/` après trim).
pub fn is_command(input: &str) -> bool {
    input.trim().starts_with('/')
}

/// Niveaux d'effort de raisonnement reconnus.
pub fn valid_effort(s: &str) -> bool {
    matches!(s, "default" | "low" | "medium" | "high")
}

/// Valeurs d'effort reconnues (sondées via `valid_effort`, source unique).
pub fn effort_values() -> Vec<&'static str> {
    [
        "minimal", "none", "low", "medium", "high", "xhigh", "max", "auto",
    ]
    .into_iter()
    .filter(|v| valid_effort(v))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_detection_and_effort_values() {
        assert!(is_command("/help"));
        assert!(is_command("  /model x"));
        assert!(!is_command("hello"));
        assert!(!is_command(""));
        assert!(valid_effort("low"));
        assert!(valid_effort("default"));
        assert!(!valid_effort("max"));
        assert_eq!(effort_values(), vec!["low", "medium", "high"]);
    }
}
