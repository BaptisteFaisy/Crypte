//! Commandes slash du chat (`/usage`, `/new`, `/thinking`, `/model`,
//! `/agent`, `/agents`, `/compact`, `/help`).
//!
//! `parse` matche le premier mot ; seul `/agent` consomme le reste de la
//! ligne, sinon le reste est ignoré. `usage_text` met en forme le registre
//! du chat courant et le registre cumulé de la session.

use crate::tokens::{fmt_cost, fmt_tps, TokenLedger};

/// Vrai si la saisie est une commande (commence par `/` après trim).
pub fn is_command(input: &str) -> bool {
    input.trim().starts_with('/')
}

/// Commande reconnue (ou mot inconnu après `/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Usage,
    New,
    Help,
    Thinking(Option<String>),
    Model(Option<String>),
    Agent(String),
    Agents,
    Compact,
    Unknown(String),
}

/// Analyse une saisie : `None` si ce n'est pas une commande.
///
/// Matche le premier mot ; `/agent` consomme le reste (trimmé, vide →
/// `Unknown`), `/thinking` et `/model` prennent un argument optionnel.
pub fn parse(input: &str) -> Option<Command> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let word = parts.next().unwrap_or("");
    match word {
        "/usage" => Some(Command::Usage),
        "/new" => Some(Command::New),
        "/help" => Some(Command::Help),
        "/thinking" => Some(Command::Thinking(opt_arg(&mut parts))),
        "/model" => Some(Command::Model(opt_arg(&mut parts))),
        "/agents" => Some(Command::Agents),
        "/compact" => Some(Command::Compact),
        "/agent" => parse_agent(trimmed, word),
        _ => Some(Command::Unknown(word.to_string())),
    }
}

/// Second mot optionnel (`/thinking [niveau]`, `/model [nom|reset]`).
fn opt_arg(parts: &mut std::str::SplitWhitespace<'_>) -> Option<String> {
    parts.next().map(str::to_string)
}

/// `/agent <texte...>` : texte trimmé ; vide → `Unknown("/agent")`.
fn parse_agent(trimmed: &str, word: &str) -> Option<Command> {
    let rest = trimmed[word.len()..].trim();
    if rest.is_empty() {
        Some(Command::Unknown(word.to_string()))
    } else {
        Some(Command::Agent(rest.to_string()))
    }
}

/// Niveaux d'effort acceptés : default/low/medium/high uniquement.
pub fn valid_effort(s: &str) -> bool {
    matches!(s, "default" | "low" | "medium" | "high")
}

/// Texte multi-lignes : modèle + thinking + usage chat courant et session.
pub fn usage_text(
    current: &TokenLedger,
    total: &TokenLedger,
    model: &str,
    thinking: &str,
) -> String {
    let (last_in, last_out) = current.last();
    format!(
        "modèle : {model}\n\
         thinking : {thinking}\n\
         CHAT — in={} out={} total={}\n\
         dernier appel : in={last_in} out={last_out}\n\
         tps : dernier={} moy={}\n\
         coût : {}\n\
         TOTAL SESSION — in={} out={} total={}\n\
         tps moyen : {}",
        current.prompt,
        current.completion,
        current.total(),
        fmt_tps(current.last_tps()),
        fmt_tps(current.avg_tps()),
        fmt_cost(current.cost()),
        total.prompt,
        total.completion,
        total.total(),
        fmt_tps(total.avg_tps()),
    )
}

/// Aide : les 8 commandes + rappel des touches + contexte agents.md.
pub fn help_text() -> &'static str {
    "Commandes :\n\
     /usage — usage tokens (chat courant + session totale)\n\
     /new — nouveau chat (remise à zéro du chat courant)\n\
     /thinking [niveau] — niveau de réflexion (default/low/medium/high)\n\
     /model [nom|reset] — afficher ou changer le modèle\n\
     /agent <texte> — lancer un sous-agent avec ces instructions\n\
     /agents — lister les sous-agents actifs\n\
     /compact — compacter l'historique du chat courant\n\
     /help — cette aide\n\
     Touches : F2 · F3 · F4 · F5 · Ctrl+N\n\
     Contexte : ./AGENTS.md chargé au démarrage (restart pour recharger)"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_matches_commands_first_word_only() {
        assert_eq!(parse("/usage"), Some(Command::Usage));
        assert_eq!(parse("  /new x  "), Some(Command::New));
        assert_eq!(parse("/help"), Some(Command::Help));
    }

    #[test]
    fn parse_rejects_plain_text() {
        assert_eq!(parse("hello"), None);
        assert_eq!(parse("  bonjour /usage"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn parse_unknown_keeps_word() {
        assert_eq!(parse("/zzz"), Some(Command::Unknown(String::from("/zzz"))));
        assert!(is_command("/zzz"));
        assert!(!is_command("hello"));
    }

    #[test]
    fn parse_thinking_model_agent_agents_compact() {
        assert_eq!(
            parse("/thinking high"),
            Some(Command::Thinking(Some(String::from("high"))))
        );
        assert_eq!(parse("/thinking"), Some(Command::Thinking(None)));
        assert_eq!(parse("/model"), Some(Command::Model(None)));
        assert_eq!(
            parse("/agent txt"),
            Some(Command::Agent(String::from("txt")))
        );
        assert_eq!(
            parse("/agent   "),
            Some(Command::Unknown(String::from("/agent")))
        );
        assert_eq!(parse("/agents"), Some(Command::Agents));
        assert_eq!(parse("/compact"), Some(Command::Compact));
        assert!(valid_effort("default"));
        assert!(valid_effort("low"));
        assert!(valid_effort("medium"));
        assert!(valid_effort("high"));
        assert!(!valid_effort("ultra"));
        assert!(!valid_effort("HIGH"));
    }
}
