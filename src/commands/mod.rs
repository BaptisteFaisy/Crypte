//! Commandes slash (`/usage` `/new` `/thinking` `/model` `/agent` `/agents`
//! `/ask` `/tell` `/msg` `/link` `/links` `/unlink` `/compact` `/connect` `/permissions` `/web` `/tools` `/kiro` `/help`).
//!
//! `parse` matche le premier mot (cibles + texte consomment le reste).

use crate::bus::{LinkEnd, Target};

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
    Agents {
        all: bool,
    },
    Ask {
        agent: usize,
        text: String,
    },
    Tell {
        target: usize,
        text: String,
    },
    Msg {
        to: MsgTarget,
        text: String,
    },
    Link {
        from: LinkEnd,
        to: LinkEnd,
    },
    Links,
    Unlink {
        from: LinkEnd,
        to: LinkEnd,
    },
    Compact,
    Hint(&'static str),
    Connect(Option<String>),
    Permissions(Option<String>),
    Web(String),
    Tools(Option<String>),
    Kiro,
    Unknown(String),
}

/// Cible `/msg` : une cible (`2`, `@2`, `#3`) ou tous les autres chats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgTarget {
    One(Target),
    All,
}

/// Aides d'usage (commandes mal formées).
pub const TELL_USAGE: &str = "usage: /tell <n> <text> — talk to chat n";
pub const ASK_USAGE: &str = "usage: /ask #<id> <text> — follow up with an agent";
pub const AGENT_USAGE: &str = "usage: /agent [@<n>] <text> — sub-agent (reply goes to chat n)";
pub const MSG_USAGE: &str = "usage: /msg <n|@n|#id|all> <text> — talk to a chat, an agent, all";
pub const LINK_USAGE: &str = "usage: /link <n|#id> <n|#id> — replies from one go to the other";
pub const UNLINK_USAGE: &str = "usage: /unlink <n|#id> <n|#id> — remove a link";
pub const WEB_USAGE: &str = "usage: /web <query> — web search with summarized sources";

/// Analyse une saisie : `None` si ce n'est pas une commande.
///
/// Matche le premier mot ; cibles + texte consomment le reste,
/// `/thinking` `/model` `/connect` `/permissions` `/tools` ont un arg optionnel.
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
        "/agents" => Some(Command::Agents {
            all: opt_arg(&mut parts).as_deref() == Some("all"),
        }),
        "/compact" => Some(Command::Compact),
        "/connect" => Some(Command::Connect(opt_arg(&mut parts))),
        "/permissions" => Some(Command::Permissions(opt_arg(&mut parts))),
        "/tools" => Some(Command::Tools(opt_arg(&mut parts))),
        "/kiro" => Some(Command::Kiro),
        "/links" => Some(Command::Links),
        "/agent" => parse_agent(trimmed, word),
        "/ask" => parse_ask(trimmed, word),
        "/tell" => parse_tell(trimmed, word),
        "/msg" => parse_msg(trimmed, word),
        "/link" => parse_link(trimmed, word),
        "/unlink" => parse_unlink(trimmed, word),
        "/web" => parse_web(trimmed, word),
        _ => Some(Command::Unknown(word.to_string())),
    }
}

/// Second mot optionnel (`/thinking` `/model` `/connect` `/permissions`).
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

/// `/msg <cible|all> <texte...>` : `2`/`@2` chat, `#3` agent, `all`
/// tous les autres chats ; mal formé → aide d'usage.
fn parse_msg(trimmed: &str, word: &str) -> Option<Command> {
    let rest = trimmed[word.len()..].trim();
    let mut parts = rest.splitn(2, char::is_whitespace);
    let token = parts.next().unwrap_or("").trim();
    let text = parts.next().map(str::trim).unwrap_or("");
    if text.is_empty() {
        return Some(Command::Hint(MSG_USAGE));
    }
    if token.eq_ignore_ascii_case("all") {
        return Some(Command::Msg {
            to: MsgTarget::All,
            text: text.to_string(),
        });
    }
    match crate::bus::parse_target(token) {
        Some(target) => Some(Command::Msg {
            to: MsgTarget::One(target),
            text: text.to_string(),
        }),
        None => Some(Command::Hint(MSG_USAGE)),
    }
}

/// Deux extrémités (`2`/`@2`/`#3`) séparées par un espace, rien de plus.
fn parse_two_ends(rest: &str) -> Option<(LinkEnd, LinkEnd)> {
    let mut parts = rest.split_whitespace();
    let from = crate::bus::parse_target(parts.next()?).map(LinkEnd::from_target)?;
    let to = crate::bus::parse_target(parts.next()?).map(LinkEnd::from_target)?;
    parts.next().is_none().then_some((from, to))
}

/// `/link <a> <b>` : les réponses de `a` vont à `b` ; mal formé → aide.
fn parse_link(trimmed: &str, word: &str) -> Option<Command> {
    match parse_two_ends(trimmed[word.len()..].trim()) {
        Some((from, to)) => Some(Command::Link { from, to }),
        None => Some(Command::Hint(LINK_USAGE)),
    }
}

/// `/unlink <a> <b>` : retire le lien ; mal formé → aide.
fn parse_unlink(trimmed: &str, word: &str) -> Option<Command> {
    match parse_two_ends(trimmed[word.len()..].trim()) {
        Some((from, to)) => Some(Command::Unlink { from, to }),
        None => Some(Command::Hint(UNLINK_USAGE)),
    }
}

/// `/web <requête...>` : recherche web ; vide → aide d'usage.
fn parse_web(trimmed: &str, word: &str) -> Option<Command> {
    let rest = trimmed[word.len()..].trim();
    if rest.is_empty() {
        return Some(Command::Hint(WEB_USAGE));
    }
    Some(Command::Web(rest.to_string()))
}

/// Niveaux d'effort acceptés : default/low/medium/high uniquement.
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

/// Aide : les 19 commandes + rappel des touches + contexte agents.md.
pub fn help_text() -> &'static str {
    "Commands:\n\
     /usage — token usage (chat + session + all-time almanac)\n\
     /new — new chat (resets the current chat)\n\
     /thinking [level] — reasoning level (default/low/medium/high)\n\
     /model [name|reset] — show or change the model\n\
     /agent [@<n>] <text> — sub-agent (reply goes to chat n if targeted)\n\
     /agents [all] — list sub-agents (all = every chat)\n\
     /ask #<id> <text> — follow up with an agent\n\
     /tell <n> <text> — talk to chat n\n\
     /msg <n|@n|#id|all> <text> — talk to a chat, an agent, all\n\
     /link <a> <b> — replies from a go to b (auto)\n\
     /links — list auto links\n\
     /unlink <a> <b> — remove a link\n\
     /compact — compact the current chat history\n\
     /connect [openai|muse] — connect (status if no argument)\n\
     /permissions [yolo|auto] — tools mode: ask or run free (remembered)\n\
     /web <query> — web search + summary with cited sources\n\
     /tools [on|off] — model tools: browser, desktop, shell (default on)\n\
     /kiro — kiro .kiro/ status (steering, specs, hooks, mcp)\n\
     /help — this help\n\
     Keys: F5 scare · F6 bestiary · F7 hub · Ctrl+N\n\
     Context: ./AGENTS.md loaded at startup (restart to reload)"
}

#[cfg(test)]
mod tests;
