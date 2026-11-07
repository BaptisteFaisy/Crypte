//! Commandes slash du chat (`/usage`, `/new`, `/thinking`, `/model`,
//! `/agent`, `/agents`, `/ask`, `/tell`, `/compact`, `/connect`, `/help`).
//!
//! `parse` matche le premier mot ; `/agent`, `/ask` et `/tell` consomment
//! le reste de la ligne (cible + texte), sinon le reste est ignoré.
//! `usage_text` met en forme le registre du chat courant et le registre
//! cumulé de la session.

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
    Agent {
        target: Option<usize>,
        prompt: String,
    },
    Agents,
    Ask {
        agent: usize,
        text: String,
    },
    Tell {
        target: usize,
        text: String,
    },
    Compact,
    Hint(&'static str),
    Connect(Option<String>),
    Unknown(String),
}

/// Aides d'usage (commandes `/tell` `/ask` `/agent` mal formées).
pub const TELL_USAGE: &str = "usage: /tell <n> <text> — talk to chat n";
pub const ASK_USAGE: &str = "usage: /ask #<id> <text> — follow up with an agent";
pub const AGENT_USAGE: &str = "usage: /agent [@<n>] <text> — sub-agent (reply goes to chat n)";

/// Analyse une saisie : `None` si ce n'est pas une commande.
///
/// Matche le premier mot ; `/agent` (cible `@<n>` optionnelle), `/ask`
/// et `/tell` (cible + texte) consomment le reste, `/thinking` et
/// `/model` prennent un argument optionnel.
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
        "/connect" => Some(Command::Connect(opt_arg(&mut parts))),
        "/agent" => parse_agent(trimmed, word),
        "/ask" => parse_ask(trimmed, word),
        "/tell" => parse_tell(trimmed, word),
        _ => Some(Command::Unknown(word.to_string())),
    }
}

/// Second mot optionnel (`/thinking [niveau]`, `/model [nom|reset]`,
/// `/connect [openai|muse]`).
fn opt_arg(parts: &mut std::str::SplitWhitespace<'_>) -> Option<String> {
    parts.next().map(str::to_string)
}

/// `/agent [@<n>] <texte...>` : `@<n>` (chiffres) route la réponse vers
/// le chat n ; `@` suivi d'autre chose fait partie du prompt. Vide ou
/// cible sans texte → `Unknown("/agent")`, `@0` → aide d'usage.
fn parse_agent(trimmed: &str, word: &str) -> Option<Command> {
    let rest = trimmed[word.len()..].trim();
    if rest.is_empty() {
        return Some(Command::Unknown(word.to_string()));
    }
    if let Some(after) = rest.strip_prefix('@') {
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let text = after[digits.len()..].trim();
            if text.is_empty() {
                return Some(Command::Unknown(word.to_string()));
            }
            match digits.parse::<usize>().unwrap_or(0) {
                n if n >= 1 => {
                    return Some(Command::Agent {
                        target: Some(n),
                        prompt: text.to_string(),
                    })
                }
                _ => return Some(Command::Hint(AGENT_USAGE)),
            }
        }
    }
    Some(Command::Agent {
        target: None,
        prompt: rest.to_string(),
    })
}

/// `/ask #<id> <texte...>` (`#` optionnel) : suite de conversation avec
/// l'agent ; mal formé → aide d'usage.
fn parse_ask(trimmed: &str, word: &str) -> Option<Command> {
    let rest = trimmed[word.len()..].trim();
    let mut parts = rest.splitn(2, char::is_whitespace);
    let token = parts.next().unwrap_or("").trim();
    let num = token.strip_prefix('#').unwrap_or(token);
    let text = parts.next().map(str::trim).unwrap_or("");
    match (num.parse::<usize>().ok(), text.is_empty()) {
        (Some(id), false) if id >= 1 => Some(Command::Ask {
            agent: id,
            text: text.to_string(),
        }),
        _ => Some(Command::Hint(ASK_USAGE)),
    }
}

/// `/tell <n> <texte...>` : parler au chat n (1-based) ; mal formé →
/// aide d'usage.
fn parse_tell(trimmed: &str, word: &str) -> Option<Command> {
    let rest = trimmed[word.len()..].trim();
    let mut parts = rest.splitn(2, char::is_whitespace);
    let target = parts.next().unwrap_or("").trim().parse::<usize>().ok();
    let text = parts.next().map(str::trim).unwrap_or("");
    match (target, text.is_empty()) {
        (Some(n), false) if n >= 1 => Some(Command::Tell {
            target: n,
            text: text.to_string(),
        }),
        _ => Some(Command::Hint(TELL_USAGE)),
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
        "model: {model}\n\
         thinking: {thinking}\n\
         CHAT — in={} out={} total={}\n\
         last call: in={last_in} out={last_out}\n\
         tps: last={} avg={}\n\
         cost: {}\n\
         SESSION TOTAL — in={} out={} total={}\n\
         avg tps: {}",
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

/// Aide : les 11 commandes + rappel des touches + contexte agents.md.
pub fn help_text() -> &'static str {
    "Commands:\n\
     /usage — token usage (current chat + session total)\n\
     /new — new chat (resets the current chat)\n\
     /thinking [level] — reasoning level (default/low/medium/high)\n\
     /model [name|reset] — show or change the model\n\
     /agent [@<n>] <text> — sub-agent (reply goes to chat n if targeted)\n\
     /agents — list active sub-agents\n\
     /ask #<id> <text> — follow up with an agent\n\
     /tell <n> <text> — talk to chat n\n\
     /compact — compact the current chat history\n\
     /connect [openai|muse] — connect (status if no argument)\n\
     /help — this help\n\
     Keys: F2 grimoire · F3/F4 chats · F5 scare · F6 bestiary · Ctrl+N\n\
     Context: ./AGENTS.md loaded at startup (restart to reload)"
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
            Some(Command::Agent {
                target: None,
                prompt: String::from("txt")
            })
        );
        assert_eq!(
            parse("/agent   "),
            Some(Command::Unknown(String::from("/agent")))
        );
        assert_eq!(parse("/agents"), Some(Command::Agents));
        assert_eq!(parse("/compact"), Some(Command::Compact));
        assert_eq!(parse("/connect"), Some(Command::Connect(None)));
        assert_eq!(
            parse("/connect muse"),
            Some(Command::Connect(Some(String::from("muse"))))
        );
        assert_eq!(
            parse("/connect openai x"),
            Some(Command::Connect(Some(String::from("openai"))))
        );
        assert!(valid_effort("default"));
        assert!(valid_effort("low"));
        assert!(valid_effort("medium"));
        assert!(valid_effort("high"));
        assert!(!valid_effort("ultra"));
        assert!(!valid_effort("HIGH"));
    }

    #[test]
    fn parse_tell_ok_et_rejets() {
        assert_eq!(
            parse("/tell 2 bonjour"),
            Some(Command::Tell {
                target: 2,
                text: String::from("bonjour")
            })
        );
        assert_eq!(parse("/tell"), Some(Command::Hint(TELL_USAGE)));
        assert_eq!(parse("/tell 2"), Some(Command::Hint(TELL_USAGE)));
        assert_eq!(parse("/tell x bonjour"), Some(Command::Hint(TELL_USAGE)));
        assert_eq!(parse("/tell 0 bonjour"), Some(Command::Hint(TELL_USAGE)));
    }

    #[test]
    fn parse_ask_diese_optionnel_et_rejets() {
        let want = Some(Command::Ask {
            agent: 3,
            text: String::from("suite"),
        });
        assert_eq!(parse("/ask #3 suite"), want);
        assert_eq!(parse("/ask 3 suite"), want);
        assert_eq!(parse("/ask"), Some(Command::Hint(ASK_USAGE)));
        assert_eq!(parse("/ask #3"), Some(Command::Hint(ASK_USAGE)));
        assert_eq!(parse("/ask x suite"), Some(Command::Hint(ASK_USAGE)));
    }

    #[test]
    fn parse_agent_cible_optionnelle() {
        assert_eq!(
            parse("/agent @2 fais ça"),
            Some(Command::Agent {
                target: Some(2),
                prompt: String::from("fais ça")
            })
        );
        // `@` non suivi de chiffres = début du prompt.
        assert_eq!(
            parse("/agent @demain fais ça"),
            Some(Command::Agent {
                target: None,
                prompt: String::from("@demain fais ça")
            })
        );
        assert_eq!(
            parse("/agent @2"),
            Some(Command::Unknown(String::from("/agent")))
        );
        assert_eq!(parse("/agent @0 x"), Some(Command::Hint(AGENT_USAGE)));
    }
}
