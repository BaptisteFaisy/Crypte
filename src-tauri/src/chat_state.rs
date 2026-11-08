//! Historique des messages + brouillon de saisie.
//!
//! Port du sous-ensemble sans rendu de `chat/mod.rs` du TUI : `Role`,
//! `Message` (sérialisés, mêmes formes), `ChatState` (historique borné
//! à 200 + brouillon `input`). Le rendu (wrap, gouttière, défilement,
//! sélection souris, saisie masquée du flow `/connect`) vit côté Svelte.

use serde::{Deserialize, Serialize};

/// Historique borné (les plus vieux sortent au-delà du cap).
const MAX_MESSAGES: usize = 200;

/// Émetteur d'un message.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

/// Un message : rôle + texte brut.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

/// Historique + brouillon de saisie.
#[derive(Debug, Default)]
pub struct ChatState {
    messages: Vec<Message>,
    /// Dernier brouillon connu (le frontend possède la saisie ; le
    /// backend le mémorise pour `chat_state`/`ctx_gauge` quand il le reçoit).
    pub input: String,
}

impl ChatState {
    /// État vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Ajoute un message à l'historique (borné à 200, les vieux sortent).
    pub fn push(&mut self, role: Role, text: impl Into<String>) {
        self.messages.push(Message {
            role,
            text: text.into(),
        });
        let overflow = self.messages.len().saturating_sub(MAX_MESSAGES);
        if overflow > 0 {
            self.messages.drain(..overflow);
        }
    }

    /// Lecture seule de l'historique.
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Vide l'historique affiché (`/clear`).
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Octets heap estimés (messages + brouillon ; analytique).
    pub fn heap_bytes(&self) -> usize {
        self.messages.capacity() * std::mem::size_of::<Message>()
            + self
                .messages
                .iter()
                .map(|message| message.text.capacity())
                .sum::<usize>()
            + self.input.capacity()
    }

    /// Prend le brouillon (le vide) pour l'envoyer.
    pub fn take_input(&mut self) -> String {
        std::mem::take(&mut self.input)
    }

    /// Colle `s` (déjà assaini) à la fin du brouillon.
    pub fn input_insert(&mut self, s: &str) {
        self.input.push_str(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_borne_a_200_les_vieux_sortent() {
        let mut chat = ChatState::new();
        for i in 0..250 {
            chat.push(Role::User, format!("m{i}"));
        }
        assert_eq!(chat.messages().len(), 200);
        assert_eq!(chat.messages()[0].text, "m50");
    }

    #[test]
    fn roles_serialises_en_minuscules() {
        let msg = Message {
            role: Role::Assistant,
            text: String::from("boo"),
        };
        let v = serde_json::to_value(&msg).unwrap();
        assert_eq!(v["role"], "assistant");
    }

    #[test]
    fn take_input_vide_le_brouillon() {
        let mut chat = ChatState::new();
        chat.input_insert("salut");
        assert_eq!(chat.take_input(), "salut");
        assert!(chat.input.is_empty());
    }
}
