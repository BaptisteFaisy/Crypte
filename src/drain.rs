//! Routage des réponses async du provider vers les chats.
//!
//! `impl App` déporté hors `app.rs` (limite 500 lignes) : les handlers
//! `drain_*` lisent/écrivent l'état via les accès `pub(crate)` de [`App`].
//! Les files d'attente ([`crate::bus`]) sont pompées à chaque tombée
//! (zéro délai humain), puis les liens auto retransmettent en éventail.

use std::time::Instant;

use anyhow::Result;

use crate::agents;
use crate::app::{App, Incoming};
use crate::apparition::Omen;
use crate::bus::{LinkEnd, QueuedAsk, QueuedTell, MAX_AUTO_HOPS};
use crate::chat::Role;
use crate::events::Event;
use crate::history::send_window;
use crate::hub::Signal;
use crate::provider::Completion;
use crate::sessions::AgentStatus;
use crate::web::WebAnswer;

impl Incoming {
    /// Tokens (in, out) facturés par la réponse ; `None` si erreur ou
    /// recherche web brute (aucun appel LLM).
    pub(crate) fn usage(&self) -> Option<(u64, u64)> {
        let billed = match self {
            Incoming::Chat(_, r) | Incoming::Compact(_, r) | Incoming::Agent(_, _, _, r) => {
                let done = r.as_ref().ok()?;
                (done.prompt_tokens, done.completion_tokens)
            }
            Incoming::Web(_, r) => {
                let done = r.as_ref().ok()?;
                (done.prompt_tokens, done.completion_tokens)
            }
        };
        let raw_web = matches!(self, Incoming::Web(..)) && billed == (0, 0);
        (!raw_web).then_some(billed)
    }
}

impl App {
    /// Réponse chat : ledger + historique + message (ou erreur système),
    /// puis file d'attente et liens auto (voir [`crate::bus`]).
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
                sess.push_history("assistant", done.text.clone());
                sess.chat.push(Role::Assistant, done.text.clone());
                self.events.emit(&Event::assistant_msg(id, &done));
                self.hub.signal(Signal::Reply { chat: id });
                self.apparition.advance();
                self.pump_chat_inbox(id);
                self.fire_chat_links(id, &done.text);
            }
            Err(err) => {
                let error = format!("{err:#}");
                sess.chat.push(Role::System, format!("error: {error}"));
                self.events.emit(&Event::assistant_error(id, &error));
                self.apparition.omen(Omen::Error);
                self.pump_chat_inbox(id);
            }
        }
    }

    /// Réponse sous-agent : statut + échange sur le chat propriétaire,
    /// ledger + message sur le chat de livraison (`[agent#id]`, ou
    /// `[agent#id from Chat k]` si livré ailleurs), versée aussi dans son
    /// contexte (rôle user, sans déclencher), puis file et liens.
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
                self.apparition.omen(Omen::Error);
                let error = format!("{err:#}");
                self.events
                    .emit(&Event::agent_failed(owner, agent_id, deliver, &error));
                if let Some(dest) = self.sessions.get_mut(deliver) {
                    dest.chat
                        .push(Role::System, format!("{tag} error: {error}"));
                }
                self.pump_agent_inbox(agent_id);
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
        self.events
            .emit(&Event::agent_done(owner, agent_id, deliver, &done));
        if let Some(dest) = self.sessions.get_mut(deliver) {
            dest.ledger
                .record(done.prompt_tokens, done.completion_tokens, 0.0);
            let tagged = format!("{tag} {}", done.text);
            dest.chat.push(Role::Assistant, tagged.clone());
            if dest.sending {
                dest.inbox.push_back(QueuedTell {
                    envelope: tagged,
                    hops: 0,
                    trigger: false,
                });
            } else {
                dest.push_history("user", tagged);
            }
            self.hub.signal(Signal::Deliver {
                agent: agent_id,
                to: deliver,
            });
        }
        self.pump_agent_inbox(agent_id);
        self.fire_agent_links(agent_id, &done.text);
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
                    .push(Role::System, format!("compacted: {before}→{after}"));
                self.events.emit(&Event::compacted(id, before, after));
                self.apparition.omen(Omen::Compact);
            }
            Err(err) => {
                sess.chat
                    .push(Role::System, format!("compaction failed: {err:#}"));
                self.apparition.omen(Omen::Error);
            }
        }
    }

    /// Déclenche la réponse du chat `idx` (historique déjà à jour,
    /// `auto_hops` déjà posé) : fenêtre d'envoi + tâche async.
    /// Sans provider, ou chat inconnu : rien.
    pub(crate) fn launch_chat(&mut self, idx: usize) {
        let Some(provider) = self.provider.clone() else {
            return;
        };
        let history = {
            let Some(sess) = self.sessions.get_mut(idx) else {
                return;
            };
            sess.sending = true;
            sess.sending_since = Some(Instant::now());
            send_window(&sess.history, sess.pinned_system)
        };
        let model = self.effective_model_for(idx);
        let effort = self.thinking.clone();
        let tx = self.tx.clone();
        self.pool.spawn(async move {
            let reply = provider
                .complete_full(&history, &model, effort.as_deref())
                .await;
            let _ = tx.send(Incoming::Chat(idx, reply)).await;
        });
    }

    /// Appel agent (échange déjà à jour) : tâche async sous permis,
    /// réponse livrée à `deliver`. Le contexte système agents.md est
    /// re-préfixé ici (une seule copie, en vol) : il n'est jamais
    /// stocké dans l'agent (voir `cmd_agent`). Sans provider : rien.
    pub(crate) fn launch_agent(
        &mut self,
        owner: usize,
        agent_id: usize,
        deliver: usize,
        mut history: Vec<(String, String)>,
    ) {
        let Some(provider) = self.provider.clone() else {
            return;
        };
        if let Some(md) = &self.agents_md {
            history.insert(0, (String::from("system"), md.clone()));
        }
        self.events
            .emit(&Event::agent_ask(owner, agent_id, deliver, &history));
        self.hub.signal(Signal::Ask {
            from: deliver,
            agent: agent_id,
        });
        let model = self.effective_model();
        let effort = self.thinking.clone();
        let tx = self.tx.clone();
        self.pool.spawn(async move {
            let reply = provider
                .complete_full(&history, &model, effort.as_deref())
                .await;
            let _ = tx
                .send(Incoming::Agent(owner, agent_id, deliver, reply))
                .await;
        });
    }

    /// File du chat `id` : verse tout à l'historique (déjà affichés à
    /// l'enfilement) et déclenche UNE réponse groupée si au moins une
    /// entrée l'exige (`trigger`, sinon simple versement contexte).
    /// Rien si occupé, inconnu, file vide, ou déconnecté (file gardée).
    fn pump_chat_inbox(&mut self, id: usize) {
        if self.provider.is_none() {
            return;
        }
        let Some(sess) = self.sessions.get_mut(id) else {
            return;
        };
        if sess.sending || sess.inbox.is_empty() {
            return;
        }
        let mut hops = 0u8;
        let mut trigger = false;
        while let Some(queued) = sess.inbox.pop_front() {
            if queued.trigger {
                trigger = true;
                hops = hops.max(queued.hops);
            }
            sess.push_history("user", queued.envelope);
        }
        if !trigger {
            return;
        }
        sess.auto_hops = hops;
        self.launch_chat(id);
    }

    /// File de l'agent : déclenche le premier ask en attente (les autres
    /// suivront à chaque tombée). Rien si inconnu, en cours, file vide,
    /// ou déconnecté (file gardée).
    fn pump_agent_inbox(&mut self, agent_id: usize) {
        if self.provider.is_none() {
            return;
        }
        let ready = {
            let Some((owner, agent)) = self.sessions.find_agent_mut(agent_id) else {
                return;
            };
            if agent.status == AgentStatus::Running {
                return;
            }
            let Some(entry) = agent.inbox.pop_front() else {
                return;
            };
            agent.push_user(entry.text);
            agent.status = AgentStatus::Running;
            agent.auto_hops = entry.hops;
            (owner, entry.deliver, agent.history.clone())
        };
        let (owner, deliver, history) = ready;
        self.launch_agent(owner, agent_id, deliver, history);
    }

    /// Liens auto depuis le chat `id` : retransmet `text` à chaque cible
    /// liée (relais hops+1, coupé au-delà de [`MAX_AUTO_HOPS`]).
    fn fire_chat_links(&mut self, id: usize, text: &str) {
        let from = LinkEnd::Chat(id);
        let hops = self.sessions.get(id).map(|s| s.auto_hops).unwrap_or(0);
        for to in self.bus.targets(from) {
            self.forward_link(from, to, text, hops);
        }
    }

    /// Liens auto depuis l'agent : retransmet `text` à chaque cible liée.
    fn fire_agent_links(&mut self, agent_id: usize, text: &str) {
        let from = LinkEnd::Agent(agent_id);
        let hops = match self.sessions.find_agent_mut(agent_id) {
            Some((_, agent)) => agent.auto_hops,
            None => return,
        };
        for to in self.bus.targets(from) {
            self.forward_link(from, to, text, hops);
        }
    }

    /// Retransmet une réponse le long d'un lien (relais hops+1) : vers
    /// un chat (tell auto) ou un agent (ask auto). Plafonné (`capped` +
    /// note à la source, voir [`MAX_AUTO_HOPS`]).
    fn forward_link(&mut self, from: LinkEnd, to: LinkEnd, text: &str, hops: u8) {
        let next = hops.saturating_add(1);
        if next > MAX_AUTO_HOPS {
            self.bus.capped += 1;
            self.note_link_capped(from);
            return;
        }
        self.bus.forwarded += 1;
        match to {
            LinkEnd::Chat(idx) => self.auto_tell(idx, from, text, next),
            LinkEnd::Agent(agent_id) => self.auto_ask(agent_id, text, next),
        }
    }

    /// Tell automatique (lien) : déposé aussitôt dans le chat cible
    /// (déclenché ou enfilé, voir `deliver_tell`). Cible inconnue
    /// (chats jamais supprimés) : ignoré.
    fn auto_tell(&mut self, idx: usize, from: LinkEnd, text: &str, hops: u8) {
        if self.sessions.get(idx).is_none() {
            return;
        }
        self.hub.signal(match from {
            LinkEnd::Chat(c) => Signal::Tell { from: c, to: idx },
            LinkEnd::Agent(a) => Signal::Deliver { agent: a, to: idx },
        });
        let envelope = format!("[auto {}] {text}", from.label());
        self.deliver_tell(idx, envelope, hops);
    }

    /// Ask automatique (lien) vers l'agent : déclenché s'il est libre
    /// (livré chez son propriétaire), enfilé sinon. Inconnu : ignoré.
    fn auto_ask(&mut self, agent_id: usize, text: &str, hops: u8) {
        let Some((owner, agent)) = self.sessions.find_agent_mut(agent_id) else {
            return;
        };
        if agent.status == AgentStatus::Running {
            agent.inbox.push_back(QueuedAsk {
                deliver: owner,
                text: text.to_string(),
                hops,
            });
            return;
        }
        agent.push_user(text.to_string());
        agent.status = AgentStatus::Running;
        agent.auto_hops = hops;
        let history = agent.history.clone();
        self.launch_agent(owner, agent_id, owner, history);
    }

    /// Note « chaîne coupée » dans le chat source (ou propriétaire).
    fn note_link_capped(&mut self, from: LinkEnd) {
        let msg = format!("link cut: {MAX_AUTO_HOPS}-hop chain limit reached");
        match from {
            LinkEnd::Chat(idx) => {
                if let Some(sess) = self.sessions.get_mut(idx) {
                    sess.chat.push(Role::System, msg);
                }
            }
            LinkEnd::Agent(agent_id) => {
                let owner = self
                    .sessions
                    .find_agent_mut(agent_id)
                    .map(|(owner, _)| owner);
                if let Some(owner) = owner {
                    if let Some(sess) = self.sessions.get_mut(owner) {
                        sess.chat.push(Role::System, msg);
                    }
                }
            }
        }
    }

    /// Web answer: ledger if synthesized (else 0 tokens, raw),
    /// `[web:backend]` message (provider history untouched).
    pub(crate) fn drain_web(&mut self, id: usize, reply: Result<WebAnswer>) {
        let Some(sess) = self.sessions.get_mut(id) else {
            return;
        };
        match reply {
            Ok(done) => {
                if done.prompt_tokens != 0 || done.completion_tokens != 0 {
                    sess.ledger
                        .record(done.prompt_tokens, done.completion_tokens, 0.0);
                }
                sess.chat.push(
                    Role::Assistant,
                    format!("[web:{}] {}", done.backend, done.text),
                );
                self.events
                    .emit(&Event::web_answer(id, done.backend, &done.text));
                self.apparition.advance();
            }
            Err(err) => {
                sess.chat
                    .push(Role::System, format!("web search failed: {err:#}"));
                self.apparition.omen(Omen::Error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventLog;

    /// `drain_chat` OK append une ligne `assistant_msg` au journal.
    #[test]
    fn drain_chat_ok_emits_assistant_msg() {
        let path =
            std::env::temp_dir().join(format!("crypte-drain-test-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut app = App::offline().unwrap();
        app.events = EventLog::new(Some(path.clone()));
        let done = Completion {
            text: String::from("boo"),
            prompt_tokens: 3,
            completion_tokens: 5,
        };
        app.drain_chat(0, Ok(done));
        let content = std::fs::read_to_string(&path).unwrap();
        let line = content.lines().next().unwrap();
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["type"], "assistant_msg");
        assert_eq!(v["chat"], 1);
        assert_eq!(v["text"], "boo");
        assert_eq!(v["completion_tokens"], 5);
        let _ = std::fs::remove_file(&path);
    }
}

/// Étiquette de réponse agent : `[agent#id]` (+ ` de Chat k` si livré
/// sur un autre chat que son propriétaire).
fn agent_tag(owner: usize, deliver: usize, agent_id: usize) -> String {
    if owner == deliver {
        format!("[agent#{agent_id}]")
    } else {
        format!("[agent#{agent_id} from Chat {}]", owner + 1)
    }
}
