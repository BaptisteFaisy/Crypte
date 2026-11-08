//! Déclenchements et bus : lancements, files, liens auto.
//!
//! Compagnon de `drain.rs` (budget 500 lignes) : `launch_*` (tâches
//! async), `deliver_tell`/`enqueue_ask` (dépôts), pompes des files et
//! retransmissions le long des liens.

use std::time::Instant;

use crate::bus::{LinkEnd, QueuedAsk, QueuedTell, MAX_AUTO_HOPS};
use crate::chat_state::Role;
use crate::core::CoreInner;
use crate::events::Event;
use crate::history::send_window;
use crate::sessions::AgentStatus;

/// Émission d'un fait vers le JSONL + `crypte-event`.
type Emit<'a> = &'a dyn Fn(Event);

/// Déclenche la réponse du chat `idx` (historique déjà à jour,
/// `auto_hops` déjà posé) : fenêtre d'envoi + tâche async.
/// Sans provider, ou chat inconnu : rien.
pub fn launch_chat(inner: &mut CoreInner, idx: usize) {
    let Some(provider) = inner.provider.clone() else {
        return;
    };
    let (history, epoch) = {
        let Some(sess) = inner.sessions.get_mut(idx) else {
            return;
        };
        sess.sending = true;
        sess.sending_since = Some(Instant::now());
        (send_window(&sess.history, sess.pinned_system), sess.epoch)
    };
    let model = inner.effective_model_for(idx);
    let effort = inner.thinking.clone();
    let tx = inner.tx.clone();
    inner.pool.spawn(async move {
        let reply = provider
            .complete_full(&history, &model, effort.as_deref())
            .await;
        let _ = tx
            .send(crate::core::Incoming::Chat(idx, epoch, reply))
            .await;
    });
}

/// Appel agent (échange déjà à jour) : tâche async sous permis,
/// réponse livrée à `deliver`. Le contexte système agents.md est
/// re-préfixé ici (une seule copie, en vol) : il n'est jamais
/// stocké dans l'agent. Sans provider : rien.
pub fn launch_agent(
    inner: &mut CoreInner,
    emit: Emit,
    owner: usize,
    agent_id: usize,
    deliver: usize,
    mut history: Vec<(String, String)>,
) {
    let Some(provider) = inner.provider.clone() else {
        return;
    };
    if let Some(md) = &inner.agents_md {
        history.insert(0, (String::from("system"), md.clone()));
    }
    crate::question::prefix(&mut history);
    emit(Event::agent_ask(owner, agent_id, deliver, &history));
    let model = inner.effective_model_for(owner);
    let effort = inner.thinking.clone();
    let tx = inner.tx.clone();
    inner.pool.spawn(async move {
        let reply = provider
            .complete_full(&history, &model, effort.as_deref())
            .await;
        let _ = tx
            .send(crate::core::Incoming::Agent(
                owner, agent_id, deliver, reply,
            ))
            .await;
    });
}

/// Dépose un tell (user ou auto) dans le chat `idx` : affiché aussitôt,
/// déclenché si libre, enfilé sinon. Rend vrai si enfilé.
pub fn deliver_tell(
    inner: &mut CoreInner,
    emit: Emit,
    idx: usize,
    envelope: String,
    hops: u8,
) -> bool {
    let dest = inner.sessions.get_mut(idx).expect("chat validé ci-dessus");
    dest.chat.push(Role::User, envelope.clone());
    emit(Event::user_msg(idx, &envelope));
    if dest.sending {
        dest.inbox.push_back(QueuedTell {
            envelope,
            hops,
            trigger: true,
        });
        return true;
    }
    dest.push_history("user", envelope);
    dest.auto_hops = hops;
    launch_chat(inner, idx);
    false
}

/// Enfile un ask sur l'agent (connu) ; rend la position dans sa file.
pub fn enqueue_ask(inner: &mut CoreInner, agent_id: usize, deliver: usize, text: String) -> usize {
    let (_, agent) = inner
        .sessions
        .find_agent_mut(agent_id)
        .expect("agent connu (Busy renvoyé par ask_agent)");
    agent.inbox.push_back(QueuedAsk {
        deliver,
        text,
        hops: 0,
    });
    agent.inbox.len()
}

/// File du chat `id` : verse tout à l'historique (déjà affichés à
/// l'enfilement) et déclenche UNE réponse groupée si au moins une
/// entrée l'exige (`trigger`, sinon simple versement contexte).
/// Rien si occupé, inconnu, file vide, ou déconnecté (file gardée).
pub(crate) fn pump_chat_inbox(inner: &mut CoreInner, id: usize) {
    if inner.provider.is_none() {
        return;
    }
    let Some(sess) = inner.sessions.get_mut(id) else {
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
    launch_chat(inner, id);
}

/// File de l'agent : déclenche le premier ask en attente (les autres
/// suivront à chaque tombée). Rien si inconnu, en cours, file vide,
/// ou déconnecté (file gardée).
pub(crate) fn pump_agent_inbox(inner: &mut CoreInner, emit: Emit, agent_id: usize) {
    if inner.provider.is_none() {
        return;
    }
    let ready = {
        let Some((owner, agent)) = inner.sessions.find_agent_mut(agent_id) else {
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
    if let Some((_, agent)) = inner.sessions.find_agent_mut(agent_id) {
        agent.deliver = deliver;
    }
    launch_agent(inner, emit, owner, agent_id, deliver, history);
}

/// Liens auto depuis le chat `id` : retransmet `text` à chaque cible
/// liée (relais hops+1, coupé au-delà de [`MAX_AUTO_HOPS`]).
pub(crate) fn fire_chat_links(inner: &mut CoreInner, emit: Emit, id: usize, text: &str) {
    let from = LinkEnd::Chat(id);
    let hops = inner.sessions.get(id).map(|s| s.auto_hops).unwrap_or(0);
    for to in inner.bus.targets(from) {
        forward_link(inner, emit, from, to, text, hops);
    }
}

/// Liens auto depuis l'agent : retransmet `text` à chaque cible liée.
pub(crate) fn fire_agent_links(inner: &mut CoreInner, emit: Emit, agent_id: usize, text: &str) {
    let from = LinkEnd::Agent(agent_id);
    let hops = match inner.sessions.find_agent_mut(agent_id) {
        Some((_, agent)) => agent.auto_hops,
        None => return,
    };
    for to in inner.bus.targets(from) {
        forward_link(inner, emit, from, to, text, hops);
    }
}

/// Retransmet une réponse le long d'un lien (relais hops+1) : vers
/// un chat (tell auto) ou un agent (ask auto). Plafonné (`capped` +
/// note à la source, voir [`MAX_AUTO_HOPS`]).
fn forward_link(
    inner: &mut CoreInner,
    emit: Emit,
    from: LinkEnd,
    to: LinkEnd,
    text: &str,
    hops: u8,
) {
    let next = hops.saturating_add(1);
    if next > MAX_AUTO_HOPS {
        inner.bus.capped += 1;
        note_link_capped(inner, from);
        return;
    }
    inner.bus.forwarded += 1;
    match to {
        LinkEnd::Chat(idx) => auto_tell(inner, emit, idx, from, text, next),
        LinkEnd::Agent(agent_id) => auto_ask(inner, emit, agent_id, text, next),
    }
}

/// Tell automatique (lien) : déposé aussitôt dans le chat cible
/// (déclenché ou enfilé, voir `deliver_tell`). Cible inconnue
/// (chat fermé entre-temps) : ignoré.
fn auto_tell(inner: &mut CoreInner, emit: Emit, idx: usize, from: LinkEnd, text: &str, hops: u8) {
    if inner.sessions.get(idx).is_none() {
        return;
    }
    let envelope = format!("[auto {}] {text}", from.label());
    deliver_tell(inner, emit, idx, envelope, hops);
}

/// Ask automatique (lien) vers l'agent : déclenché s'il est libre
/// (livré chez son propriétaire), enfilé sinon. Inconnu : ignoré.
fn auto_ask(inner: &mut CoreInner, emit: Emit, agent_id: usize, text: &str, hops: u8) {
    let Some((owner, agent)) = inner.sessions.find_agent_mut(agent_id) else {
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
    agent.deliver = owner;
    let history = agent.history.clone();
    launch_agent(inner, emit, owner, agent_id, owner, history);
}

/// Note « chaîne coupée » dans le chat source (ou propriétaire).
fn note_link_capped(inner: &mut CoreInner, from: LinkEnd) {
    let msg = format!("link cut: {MAX_AUTO_HOPS}-hop chain limit reached");
    match from {
        LinkEnd::Chat(idx) => {
            if let Some(sess) = inner.sessions.get_mut(idx) {
                sess.chat.push(Role::System, msg);
            }
        }
        LinkEnd::Agent(agent_id) => {
            let owner = inner
                .sessions
                .find_agent_mut(agent_id)
                .map(|(owner, _)| owner);
            if let Some(owner) = owner {
                if let Some(sess) = inner.sessions.get_mut(owner) {
                    sess.chat.push(Role::System, msg);
                }
            }
        }
    }
}

/// Étiquette de réponse agent : `[agent#id]` (+ ` de Chat k` si livré
/// sur un autre chat que son propriétaire).
pub(crate) fn agent_tag(owner: usize, deliver: usize, agent_id: usize) -> String {
    if owner == deliver {
        format!("[agent#{agent_id}]")
    } else {
        format!("[agent#{agent_id} from Chat {}]", owner + 1)
    }
}
