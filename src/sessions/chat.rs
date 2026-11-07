//! Un chat : affichage, historique, tokens, sous-agents.

use std::collections::VecDeque;
use std::time::Instant;

use crate::bus::QueuedTell;
use crate::chat::ChatState;
use crate::history::trim_history;
use crate::tokens::TokenLedger;

use super::subagents::SubAgent;

/// Un chat : affichage, historique provider, usage tokens, envoi en cours.
pub struct ChatSession {
    pub name: String,
    pub chat: ChatState,
    pub history: Vec<(String, String)>,
    pub ledger: TokenLedger,
    pub sending: bool,
    pub sending_since: Option<Instant>,
    /// Modèle du chat (`None` = défaut provider).
    pub model: Option<String>,
    /// Sous-agents lancés depuis ce chat.
    pub agents: Vec<SubAgent>,
    /// Vrai si `history[0]` est le contexte agents.md pinné.
    pub pinned_system: bool,
    /// Tells en attente (affichés aussitôt, déclenchés dès que libre).
    pub inbox: VecDeque<QueuedTell>,
    /// Relais auto de la réponse en vol (0 = déclenchée par le user).
    pub auto_hops: u8,
}

impl ChatSession {
    /// Nouveau chat vide avec ce nom.
    pub fn new(name: String) -> Self {
        Self {
            name,
            chat: ChatState::new(),
            history: Vec::new(),
            ledger: TokenLedger::new(),
            sending: false,
            sending_since: None,
            model: None,
            agents: Vec::new(),
            pinned_system: false,
            inbox: VecDeque::new(),
            auto_hops: 0,
        }
    }

    /// Épingle le contexte agents.md en tête d'historique (chat neuf).
    pub fn pin_system(&mut self, content: &str) {
        self.history
            .push((String::from("system"), content.to_string()));
        self.pinned_system = true;
    }

    /// Ajoute un tour provider, borné LRU (`HISTORY_MAX`, tête pinnée gardée).
    pub fn push_history(&mut self, role: &str, text: String) {
        self.history.push((String::from(role), text));
        trim_history(&mut self.history, self.pinned_system);
    }

    /// Octets heap estimés du chat (capacités des buffers : nom, affichage,
    /// historique provider, modèle, file, agents). Mesure analytique,
    /// déterministe à scénario fixé (hors slack allocateur).
    pub fn heap_bytes(&self) -> usize {
        let mut bytes = self.name.capacity() + self.chat.heap_bytes();
        bytes += heap_pairs(&self.history);
        bytes += self.model.as_ref().map(|m| m.capacity()).unwrap_or(0);
        bytes += self
            .inbox
            .iter()
            .map(|queued| queued.envelope.capacity())
            .sum::<usize>();
        bytes += self
            .agents
            .iter()
            .map(|agent| {
                agent.prompt.capacity()
                    + heap_pairs(&agent.history)
                    + agent
                        .inbox
                        .iter()
                        .map(|ask| ask.text.capacity())
                        .sum::<usize>()
            })
            .sum::<usize>();
        bytes
    }
}

/// Heap d'un échange : rôles + contenus (+ slack du `Vec`).
fn heap_pairs(pairs: &Vec<(String, String)>) -> usize {
    pairs.capacity() * std::mem::size_of::<(String, String)>()
        + pairs
            .iter()
            .map(|(role, content)| role.capacity() + content.capacity())
            .sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HISTORY_MAX;

    #[test]
    fn push_history_borne_lru_garde_pin() {
        let mut chat = ChatSession::new(String::from("Chat 1"));
        chat.pin_system("pin");
        for i in 0..250 {
            chat.push_history("user", format!("m{i}"));
        }
        assert_eq!(chat.history.len(), HISTORY_MAX);
        assert_eq!(chat.history[0].1, "pin");
        assert_eq!(chat.history[1].1, "m51");
    }

    fn garni() -> ChatSession {
        let mut chat = ChatSession::new(String::from("Chat 1"));
        chat.pin_system("pin");
        for i in 0..40 {
            chat.push_history("user", format!("question {i}"));
            chat.push_history("assistant", format!("reponse {i}"));
        }
        chat
    }

    #[test]
    fn heap_bytes_deterministe_et_croit_avec_le_contenu() {
        let vide = ChatSession::new(String::from("Chat 1"));
        let plein = garni();
        assert_eq!(garni().heap_bytes(), plein.heap_bytes());
        assert!(plein.heap_bytes() > vide.heap_bytes());
        let contenus: usize = plein.history.iter().map(|(_, c)| c.len()).sum();
        assert!(plein.heap_bytes() >= contenus);
    }
}
