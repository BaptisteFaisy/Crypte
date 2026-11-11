//! Un chat : affichage, historique, tokens, sous-agents.
//!
//! Port de `sessions/chat.rs` du TUI (références `crate::chat` adaptées
//! vers `crate::chat_state`).

use std::collections::VecDeque;
use std::time::Instant;

use crate::bus::QueuedTell;
use crate::chat_state::{ChatState, Role};
use crate::history::trim_history;
use crate::question::PendingQuestion;
use crate::tokens::TokenLedger;

use crate::subagents::SubAgent;

/// Un chat : affichage, historique provider, usage tokens, envoi en cours.
pub struct ChatSession {
    pub name: String,
    pub chat: ChatState,
    pub history: Vec<(String, String)>,
    pub ledger: TokenLedger,
    pub sending: bool,
    pub sending_since: Option<Instant>,
    /// Époque du vol en cours : bumpée à chaque stop ; une
    /// réponse d'époque révolue est ignorée (`drain_chat`).
    pub epoch: u64,
    /// Réponse annexe en vol (`/compact`, `/web` : indexée, non bloquante).
    pub waiting: bool,
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
    /// Question de l'IA en attente (`None` = aucune, pastille jaune sinon).
    pub pending: Option<PendingQuestion>,
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
            epoch: 0,
            waiting: false,
            model: None,
            agents: Vec::new(),
            pinned_system: false,
            inbox: VecDeque::new(),
            auto_hops: 0,
            pending: None,
        }
    }

    /// Épingle le contexte agents.md en tête d'historique (chat neuf).
    pub fn pin_system(&mut self, content: &str) {
        self.history
            .push((String::from("system"), content.to_string()));
        self.pinned_system = true;
    }

    /// Vide la conversation (`/clear`) : affichage + historique provider,
    /// question en attente effacée ; pinné, modèle, tokens et agents gardés.
    pub fn clear(&mut self) {
        self.chat.clear();
        self.history.truncate(usize::from(self.pinned_system));
        self.pending = None;
    }

    /// Ajoute un tour provider, borné LRU (`HISTORY_MAX`, tête pinnée gardée).
    pub fn push_history(&mut self, role: &str, text: String) {
        self.history.push((String::from(role), text));
        trim_history(&mut self.history, self.pinned_system);
    }

    /// Stoppe le vol en cours : époque bumpée (la réponse
    /// tardive sera ignorée), flags effacés, note système. Rend vrai
    /// si un vol a été stoppé (faux si rien n'était en vol).
    pub fn stop_sending(&mut self) -> bool {
        if !self.sending {
            return false;
        }
        self.sending = false;
        self.sending_since = None;
        self.epoch += 1;
        self.chat
            .push(Role::System, String::from("réponse stoppée (Ctrl+C)"));
        true
    }

    /// Réponse reçue : mémorise la question éventuelle, rend le texte affiché.
    pub fn track_question(&mut self, raw: &str) -> String {
        let (shown, asked) = crate::question::render_display(raw);
        self.pending = asked;
        shown
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
            .pending
            .as_ref()
            .map(|q| q.question.capacity() + q.options.iter().map(|o| o.capacity()).sum::<usize>())
            .unwrap_or(0);
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
    fn clear_vide_conversation_garde_pinne() {
        use crate::chat_state::Role;
        let mut chat = ChatSession::new(String::from("Chat 1"));
        chat.pin_system("pin");
        chat.push_history("user", String::from("salut"));
        chat.chat.push(Role::User, "salut");
        chat.pending =
            crate::question::render_display("```question\nOn continue ?\n1. Oui\n2. Non\n```").1;
        assert!(chat.pending.is_some());
        chat.clear();
        assert!(chat.chat.messages().is_empty());
        assert_eq!(
            chat.history,
            vec![(String::from("system"), String::from("pin"))]
        );
        assert!(chat.pending.is_none());
        // Sans pinné : historique totalement vidé.
        let mut chat = ChatSession::new(String::from("Chat 2"));
        chat.push_history("user", String::from("x"));
        chat.clear();
        assert!(chat.history.is_empty());
        assert!(chat.chat.messages().is_empty());
    }

    #[test]
    fn stop_sending_coupe_le_vol_bumpe_l_epoque_et_note() {
        let mut chat = ChatSession::new(String::from("Chat 1"));
        assert!(!chat.stop_sending(), "rien en vol : faux");
        assert_eq!(chat.epoch, 0);
        assert!(chat.chat.messages().is_empty(), "rien en vol : muet");
        chat.sending = true;
        chat.sending_since = Some(std::time::Instant::now());
        assert!(chat.stop_sending());
        assert!(!chat.sending);
        assert!(chat.sending_since.is_none());
        assert_eq!(chat.epoch, 1);
        let last = chat.chat.messages().last().unwrap();
        assert!(last.text.contains("stoppée"), "note : {:?}", last.text);
        assert!(!chat.stop_sending(), "second stop : faux");
        assert_eq!(chat.epoch, 1, "époque figée sans vol");
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
