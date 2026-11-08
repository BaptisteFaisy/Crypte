//! Routage des réponses async du provider vers les chats.
//!
//! Port de `drain.rs` du TUI : les handlers `drain_*` reçoivent l'état
//! et une émission d'`Event` (déclenchements, files et liens auto : voir
//! `drain_bus`). Hors tranche (documenté pour l'intégrateur) : signaux
//! hub, présages apparition, boucle outils (voir `DrainOutcome`), specs
//! kiro, web, hooks, critique — le frontend se rafraîchit via
//! `crypte-event` + `chat_state`/`agents_list`.

use anyhow::Result;

use crate::bus::QueuedTell;
use crate::chat_state::Role;
use crate::compact::compacted_history;
use crate::core::CoreInner;
use crate::drain_bus::{
    agent_tag, fire_agent_links, fire_chat_links, pump_agent_inbox, pump_chat_inbox,
};
use crate::events::Event;
use crate::provider::Completion;
use crate::sessions::AgentStatus;

/// Émission d'un fait vers le JSONL + `crypte-event`.
type Emit<'a> = &'a dyn Fn(Event);

/// Sort d'un drain chat : fini, ignoré (époque révolue), ou appels
/// d'outils à traiter (tranche kiro-tools : `drain_tool_calls`, sinon
/// `finish_chat_text` pour le rendu texte seul).
pub enum DrainOutcome {
    Done,
    Ignored,
    ToolCalls {
        chat: usize,
        done: Completion,
        elapsed: f64,
    },
}

/// Réponse chat : ledger + historique + message (ou erreur système),
/// puis file d'attente et liens auto (voir [`crate::bus`]).
/// Époque révolue (vol stoppé puis renvoyé) : ignorée.
pub fn drain_chat(
    inner: &mut CoreInner,
    emit: Emit,
    id: usize,
    epoch: u64,
    reply: Result<Completion>,
) -> DrainOutcome {
    if inner.sessions.get(id).is_some_and(|s| s.epoch != epoch) {
        return DrainOutcome::Ignored;
    }
    let elapsed = {
        let Some(sess) = inner.sessions.get_mut(id) else {
            return DrainOutcome::Ignored;
        };
        sess.sending = false;
        let elapsed = sess
            .sending_since
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        sess.sending_since = None;
        elapsed
    };
    match reply {
        Ok(done) => {
            if !done.tool_calls.is_empty() {
                // Tranche kiro-tools : boucle outils (budget, confirm, exec).
                return DrainOutcome::ToolCalls {
                    chat: id,
                    done,
                    elapsed,
                };
            }
            finish_chat_text(inner, emit, id, &done, elapsed);
            DrainOutcome::Done
        }
        Err(err) => {
            let Some(sess) = inner.sessions.get_mut(id) else {
                return DrainOutcome::Ignored;
            };
            let error = format!("{err:#}");
            sess.chat.push(Role::System, format!("error: {error}"));
            emit(Event::assistant_error(id, &error));
            pump_chat_inbox(inner, id);
            DrainOutcome::Done
        }
    }
}

/// Rendu texte seul d'une réponse chat : ledger, historique, message,
/// événement, puis file et liens. Sert le drain sans outils et le repli
/// sans tranche kiro-tools (`DrainOutcome::ToolCalls` non consommé).
pub fn finish_chat_text(
    inner: &mut CoreInner,
    emit: Emit,
    id: usize,
    done: &Completion,
    elapsed: f64,
) {
    let Some(sess) = inner.sessions.get_mut(id) else {
        return;
    };
    sess.ledger
        .record(done.prompt_tokens, done.completion_tokens, elapsed);
    sess.push_history("assistant", done.text.clone());
    let shown = sess.track_question(&done.text);
    sess.chat.push(Role::Assistant, shown);
    emit(Event::assistant_msg(id, done));
    pump_chat_inbox(inner, id);
    let text = done.text.clone();
    fire_chat_links(inner, emit, id, &text);
}

/// Réponse sous-agent : statut + échange sur le chat propriétaire,
/// ledger + message sur le chat de livraison (`[agent#id]`, ou
/// `[agent#id from Chat k]` si livré ailleurs), versée aussi dans son
/// contexte (rôle user, sans déclencher), puis file et liens.
pub fn drain_agent(
    inner: &mut CoreInner,
    emit: Emit,
    owner: usize,
    agent_id: usize,
    deliver: usize,
    reply: Result<Completion>,
) {
    let tag = agent_tag(owner, deliver, agent_id);
    let done = match reply {
        Ok(done) => done,
        Err(err) => {
            drain_agent_error(inner, emit, owner, agent_id, deliver, &tag, err);
            return;
        }
    };
    if let Some(sess) = inner.sessions.get_mut(owner) {
        if let Some(agent) = sess.agents.iter_mut().find(|a| a.id == agent_id) {
            agent.prompt_tokens = done.prompt_tokens;
            agent.completion_tokens = done.completion_tokens;
            agent.status = AgentStatus::Done;
            agent.push_assistant(done.text.clone());
        }
    }
    emit(Event::agent_done(owner, agent_id, deliver, &done));
    if let Some(dest) = inner.sessions.get_mut(deliver) {
        dest.ledger
            .record(done.prompt_tokens, done.completion_tokens, 0.0);
        let tagged = format!("{tag} {}", dest.track_question(&done.text));
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
    }
    pump_agent_inbox(inner, emit, agent_id);
    let text = done.text.clone();
    fire_agent_links(inner, emit, agent_id, &text);
}

/// Réponse d'agent en erreur : statut, note sur le chat de livraison,
/// événement, puis file (les asks en attente partent quand même).
fn drain_agent_error(
    inner: &mut CoreInner,
    emit: Emit,
    owner: usize,
    agent_id: usize,
    deliver: usize,
    tag: &str,
    err: anyhow::Error,
) {
    fail_agent(inner, owner, agent_id);
    let error = format!("{err:#}");
    emit(Event::agent_failed(owner, agent_id, deliver, &error));
    if let Some(dest) = inner.sessions.get_mut(deliver) {
        dest.chat
            .push(Role::System, format!("{tag} error: {error}"));
    }
    pump_agent_inbox(inner, emit, agent_id);
}

/// Marque un agent échoué sur son chat propriétaire (réponse en erreur).
fn fail_agent(inner: &mut CoreInner, owner: usize, agent_id: usize) {
    if let Some(sess) = inner.sessions.get_mut(owner) {
        if let Some(agent) = sess.agents.iter_mut().find(|a| a.id == agent_id) {
            agent.status = AgentStatus::Failed;
        }
    }
}

/// Résumé reçu : historique remplacé (pinné + résumé + 4 derniers).
/// En cas d'échec, l'historique reste intact (erreur système seule).
/// Rend `(before, after)` si le chat existe toujours.
pub fn drain_compact(
    inner: &mut CoreInner,
    emit: Emit,
    id: usize,
    reply: Result<Completion>,
) -> Option<(usize, usize)> {
    let sess = inner.sessions.get_mut(id)?;
    sess.waiting = false;
    match reply {
        Ok(done) => {
            sess.ledger
                .record(done.prompt_tokens, done.completion_tokens, 0.0);
            let before = sess.history.len();
            let compacted = compacted_history(&sess.history, &done.text, sess.pinned_system);
            sess.history = compacted;
            let after = sess.history.len();
            sess.chat
                .push(Role::System, format!("compacted: {before}→{after}"));
            emit(Event::compacted(id, before, after));
            Some((before, after))
        }
        Err(err) => {
            sess.chat
                .push(Role::System, format!("compaction failed: {err:#}"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::LinkEnd;
    use crate::core::tests::{done, test_core};
    use crate::drain_bus::deliver_tell;
    use crate::provider::Provider;

    /// Texte du dernier message affiché du chat `idx`.
    fn last_text(core: &crate::core::Core, idx: usize) -> String {
        core.lock()
            .sessions
            .get(idx)
            .unwrap()
            .chat
            .messages()
            .last()
            .map(|m| m.text.clone())
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn drain_chat_pompe_la_file_en_groupe() {
        let (core, _channels, dir) = test_core("pump");
        // Provider factice branché (sinon la pompe garde la file).
        let cfg = crate::provider::ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        core.lock().provider = Some(std::sync::Arc::new(Provider::new(cfg).unwrap()));
        core.add_chat(); // courant = Chat 2
        core.lock().sessions.select(0);
        core.lock().sessions.get_mut(1).unwrap().sending = true;
        let emit = |event: Event| core.emit(event);
        {
            let mut inner = core.lock();
            deliver_tell(&mut inner, &emit, 1, String::from("[t] premier"), 0);
            deliver_tell(&mut inner, &emit, 1, String::from("[t] second"), 0);
        }
        {
            let mut inner = core.lock();
            drain_chat(&mut inner, &emit, 1, 0, Ok(done("ok")));
        }
        let inner = core.lock();
        let dest = inner.sessions.get(1).unwrap();
        assert!(dest.sending); // réponse groupée relancée aussitôt
        assert!(dest.inbox.is_empty());
        let users = dest
            .history
            .iter()
            .filter(|(role, _)| role == "user")
            .count();
        assert_eq!(users, 2);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn lien_chat_vers_chat_retransmet_et_declenche() {
        let (core, _channels, dir) = test_core("link");
        let cfg = crate::provider::ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        core.lock().provider = Some(std::sync::Arc::new(Provider::new(cfg).unwrap()));
        core.add_chat();
        core.lock()
            .bus
            .link(LinkEnd::Chat(0), LinkEnd::Chat(1))
            .unwrap();
        let emit = |event: Event| core.emit(event);
        {
            let mut inner = core.lock();
            drain_chat(&mut inner, &emit, 0, 0, Ok(done("salut")));
        }
        assert!(core.lock().sessions.get(1).unwrap().sending);
        assert_eq!(last_text(&core, 1), "[auto Chat 1] salut");
        assert_eq!(core.lock().bus.forwarded, 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn plafond_relais_coupe_les_boucles() {
        let (core, _channels, dir) = test_core("cap");
        core.add_chat();
        {
            let mut inner = core.lock();
            inner.bus.link(LinkEnd::Chat(0), LinkEnd::Chat(1)).unwrap();
            inner.bus.link(LinkEnd::Chat(1), LinkEnd::Chat(0)).unwrap();
            inner.sessions.get_mut(0).unwrap().auto_hops = 8;
        }
        let emit = |event: Event| core.emit(event);
        {
            let mut inner = core.lock();
            drain_chat(&mut inner, &emit, 0, 0, Ok(done("x")));
        }
        {
            let inner = core.lock();
            assert_eq!(inner.bus.capped, 1);
            assert_eq!(inner.bus.forwarded, 0);
            assert!(!inner.sessions.get(1).unwrap().sending);
        }
        // `last_text` reverrouille : garde relâchée avant.
        assert!(last_text(&core, 0).starts_with("link cut"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn epoque_revolue_ignoree_et_toolcalls_remontes() {
        let (core, _channels, dir) = test_core("epoch");
        let emit = |event: Event| core.emit(event);
        {
            let mut inner = core.lock();
            inner.sessions.get_mut(0).unwrap().sending = true;
            inner.sessions.get_mut(0).unwrap().epoch = 3;
            let out = drain_chat(&mut inner, &emit, 0, 2, Ok(done("tard")));
            assert!(matches!(out, DrainOutcome::Ignored));
        }
        {
            let mut inner = core.lock();
            let mut tooled = done("appel");
            tooled.tool_calls.push(crate::provider::ToolCall {
                id: String::from("c1"),
                name: String::from("shell_run"),
                args: String::from("{}"),
            });
            let out = drain_chat(&mut inner, &emit, 0, 3, Ok(tooled));
            assert!(matches!(out, DrainOutcome::ToolCalls { .. }));
            // Repli texte seul : rendu + événement comme un drain normal.
            let plain = done("appel");
            finish_chat_text(&mut inner, &emit, 0, &plain, 0.5);
            assert_eq!(inner.sessions.get(0).unwrap().ledger.total(), 2);
        }
        // `last_text` reverrouille : garde `inner` relâchée avant.
        assert_eq!(last_text(&core, 0), "appel");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn livraison_agent_verse_contexte_sans_declencher() {
        let (core, _channels, dir) = test_core("agent");
        core.add_chat();
        let history = vec![(String::from("user"), String::from("u"))];
        let id = core
            .lock()
            .sessions
            .add_agent(0, "prompt", history)
            .unwrap();
        let emit = |event: Event| core.emit(event);
        {
            let mut inner = core.lock();
            drain_agent(&mut inner, &emit, 0, id, 1, Ok(done("trouvaille")));
        }
        let tagged = "[agent#1 from Chat 1] trouvaille";
        {
            let inner = core.lock();
            let dest = inner.sessions.get(1).unwrap();
            assert!(!dest.sending); // FYI : pas de réponse déclenchée
            assert!(dest.history.iter().any(|(r, t)| r == "user" && t == tagged));
        }
        // `last_text` reverrouille : garde relâchée avant.
        assert_eq!(last_text(&core, 1), tagged);
        std::fs::remove_dir_all(dir).ok();
    }
}
