//! Un chat : affichage, historique, tokens, sous-agents.

use std::collections::VecDeque;
use std::time::Instant;

use crate::bus::QueuedTell;
use crate::chat::ChatState;
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

    /// Tokens des sous-agents du chat (in + out ; un agent `Running`
    /// vaut 0 tant que sa réponse n'est pas arrivée).
    pub fn agents_total(&self) -> u64 {
        self.agents
            .iter()
            .map(|a| a.prompt_tokens.saturating_add(a.completion_tokens))
            .fold(0, u64::saturating_add)
    }
}
