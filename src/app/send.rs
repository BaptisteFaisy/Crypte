//! Envoi du chat courant (`send`, déporté hors `app.rs`, budget 500 lignes).

use std::time::Instant;

use super::{App, Incoming};
use crate::chat::Role;
use crate::commands;
use crate::connect::NOT_CONNECTED;
use crate::history::{send_window, short_name};

impl App {
    /// Enfile l'input : commande locale ou complétion en tâche de fond.
    /// Un chat sans message user prend le nom du premier message
    /// (ignore le system pinné : historique non vide mais sans user).
    pub(super) fn send(&mut self) {
        let id = self.sessions.index();
        let text = self.sessions.current().chat.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        if commands::is_command(&text) {
            self.sessions.current_mut().chat.take_input();
            self.run_command(&text);
            return;
        }
        let Some(provider) = self.provider.clone() else {
            self.push_system(String::from(NOT_CONNECTED));
            return;
        };
        if self.sessions.current().sending {
            return;
        }
        // Premier envoi réel : lecture unique d'agents.md (pin + annonce).
        self.ensure_agents_loaded();
        if let Some(used) = self.context_over_limit(&text) {
            self.push_system(format!(
                "context full ({}), send refused: /compact to continue",
                crate::tokens::fmt_context(used, self.context_limit())
            ));
            return;
        }
        let model = self.effective_model();
        let effort = self.thinking.clone();
        let tx = self.tx.clone();
        let sess = self.sessions.current_mut();
        sess.chat.take_input();
        if !sess.history.iter().any(|(role, _)| role == "user") {
            sess.name = short_name(&text);
        }
        sess.chat.push(Role::User, text.clone());
        sess.chat.scroll.to_bottom();
        self.events.emit(&crate::events::Event::user_msg(id, &text));
        sess.push_history("user", text);
        let history = send_window(&sess.history, sess.pinned_system);
        sess.sending = true;
        sess.auto_hops = 0;
        sess.sending_since = Some(Instant::now());
        if self.tools_on {
            let wire = crate::tools::flow::tooled_wire(history);
            self.pool.spawn(async move {
                let tools = crate::tools::defs();
                let reply = provider
                    .complete_with_tools(&wire, &tools, &model, effort.as_deref())
                    .await;
                let _ = tx.send(Incoming::Chat(id, reply)).await;
            });
        } else {
            self.pool.spawn(async move {
                let reply = provider
                    .complete_full(&history, &model, effort.as_deref())
                    .await;
                let _ = tx.send(Incoming::Chat(id, reply)).await;
            });
        }
    }
}
