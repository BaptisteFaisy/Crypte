//! Routage des réponses async du provider vers les chats.
//!
//! `impl App` déporté hors `app.rs` (limite 500 lignes) : les handlers
//! `drain_*` lisent/écrivent l'état via les accès `pub(crate)` de [`App`].

use anyhow::Result;

use crate::agents;
use crate::app::App;
use crate::chat::Role;
use crate::provider::Completion;
use crate::sessions::AgentStatus;

impl App {
    /// Réponse chat : ledger + historique + message (ou erreur système).
    pub(crate) fn drain_chat(&mut self, id: usize, reply: Result<Completion>) {
        let Some(sess) = self.sessions.get_mut(id) else {
            return;
        };
        sess.sending = false;
        let elapsed = sess
            .sending_since
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        sess.sending_since = None;
        match reply {
            Ok(done) => {
                sess.ledger
                    .record(done.prompt_tokens, done.completion_tokens, elapsed);
                sess.history
                    .push(("assistant".to_string(), done.text.clone()));
                sess.chat.push(Role::Assistant, done.text);
            }
            Err(err) => sess.chat.push(Role::System, format!("erreur: {err:#}")),
        }
    }

    /// Réponse sous-agent : statut + échange sur le chat propriétaire,
    /// ledger + message sur le chat de livraison (`[agent#id]`, ou
    /// `[agent#id de Chat k]` si livré ailleurs).
    pub(crate) fn drain_agent(
        &mut self,
        owner: usize,
        agent_id: usize,
        deliver: usize,
        reply: Result<Completion>,
    ) {
        let tag = agent_tag(owner, deliver, agent_id);
        let done = match reply {
            Ok(done) => done,
            Err(err) => {
                self.fail_agent(owner, agent_id);
                if let Some(dest) = self.sessions.get_mut(deliver) {
                    dest.chat
                        .push(Role::System, format!("{tag} erreur: {err:#}"));
                }
                return;
            }
        };
        if let Some(sess) = self.sessions.get_mut(owner) {
            if let Some(agent) = sess.agents.iter_mut().find(|a| a.id == agent_id) {
                agent.prompt_tokens = done.prompt_tokens;
                agent.completion_tokens = done.completion_tokens;
                agent.status = AgentStatus::Done;
                agent.push_assistant(done.text.clone());
            }
        }
        if let Some(dest) = self.sessions.get_mut(deliver) {
            dest.ledger
                .record(done.prompt_tokens, done.completion_tokens, 0.0);
            dest.chat
                .push(Role::Assistant, format!("{tag} {}", done.text));
        }
    }

    /// Marque un agent échoué sur son chat propriétaire (réponse en erreur).
    fn fail_agent(&mut self, owner: usize, agent_id: usize) {
        if let Some(sess) = self.sessions.get_mut(owner) {
            if let Some(agent) = sess.agents.iter_mut().find(|a| a.id == agent_id) {
                agent.status = AgentStatus::Failed;
            }
        }
    }

    /// Résumé reçu : historique remplacé (pinné + résumé + 4 derniers).
    /// En cas d'échec, l'historique reste intact (erreur système seule).
    pub(crate) fn drain_compact(&mut self, id: usize, reply: Result<Completion>) {
        let Some(sess) = self.sessions.get_mut(id) else {
            return;
        };
        match reply {
            Ok(done) => {
                sess.ledger
                    .record(done.prompt_tokens, done.completion_tokens, 0.0);
                let before = sess.history.len();
                let compacted =
                    agents::compacted_history(&sess.history, &done.text, sess.pinned_system);
                sess.history = compacted;
                let after = sess.history.len();
                sess.chat
                    .push(Role::System, format!("compacté : {before}→{after}"));
            }
            Err(err) => sess
                .chat
                .push(Role::System, format!("compaction échouée : {err:#}")),
        }
    }
}

/// Étiquette de réponse agent : `[agent#id]` (+ ` de Chat k` si livré
/// sur un autre chat que son propriétaire).
fn agent_tag(owner: usize, deliver: usize, agent_id: usize) -> String {
    if owner == deliver {
        format!("[agent#{agent_id}]")
    } else {
        format!("[agent#{agent_id} de Chat {}]", owner + 1)
    }
}
