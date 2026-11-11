//! Commandes agents + bus §2 (`commands.md`) : spawn, ask, tell, msg, liens.
//!
//! Cibles `msg_send` : `2`/`@2` chat (1-based, comme `/tell`), `#3`
//! agent, `all` (insensible à la casse, comme le TUI) ; les autres
//! commandes prennent des index chats 0-based (contrat).

use crate::bus::{parse_target, LinkEnd, LinkError, Target};
use crate::core::{Core, NOT_CONNECTED};
use crate::drain_bus;
use crate::dto::{BusStateDto, LinkEndDto, SubAgentInfo};
use crate::events::Event;
use crate::sessions::AskError;
use crate::tokens::{context_estimate, fmt_context};

impl Core {
    /// `agent_spawn` : sous-agent (réponse livrée à `target_chat` ou ici) ;
    /// workspace disque attaché, événement émis, réponse par `crypte-event`.
    pub async fn agent_spawn(
        &self,
        prompt: String,
        target_chat: Option<usize>,
    ) -> Result<SubAgentInfo, String> {
        let prompt = prompt.trim().to_string();
        if prompt.is_empty() {
            return Err(String::from("empty prompt"));
        }
        // Verrou : gardes + insertion + capture du plan de spawn.
        let plan = {
            let mut inner = self.lock();
            let guarded = guard_agent_spawn(&mut inner, &prompt, target_chat)?;
            let (agent_id, info) =
                insert_agent(&mut inner, guarded.owner, guarded.deliver, &prompt)?;
            let plan = SpawnPlan::capture(&inner, guarded, agent_id, info);
            drop(inner);
            self.emit(Event::agent_spawned(
                plan.agent_id,
                plan.owner,
                plan.deliver,
                &prompt,
            ));
            plan
        };
        let info = plan.info.clone();
        plan.spawn();
        Ok(info)
    }

    /// `agent_ask` : suite de conversation avec l'agent (réponse livrée
    /// ici) ; enfilé s'il est en cours (déclenché dès qu'il est libre).
    pub fn agent_ask(&self, agent_id: usize, text: String) -> Result<(), String> {
        let mut inner = self.lock();
        if inner.provider.is_none() {
            inner.push_system(String::from(NOT_CONNECTED));
            return Err(String::from(NOT_CONNECTED));
        }
        let here = inner.sessions.index();
        match inner.sessions.ask_agent(agent_id, text.clone()) {
            Err(AskError::Unknown) => {
                let msg = format!("agent#{agent_id} unknown");
                inner.push_system(msg.clone());
                Err(msg)
            }
            Err(AskError::Busy) => {
                let pos = drain_bus::enqueue_ask(&mut inner, agent_id, here, text);
                inner.push_system(format!("agent#{agent_id} busy — queued (position {pos})"));
                Ok(())
            }
            Ok((owner, history)) => {
                if let Some((_, agent)) = inner.sessions.find_agent_mut(agent_id) {
                    agent.auto_hops = 0;
                    agent.deliver = here;
                }
                let emit = |event: Event| self.emit(event);
                drain_bus::launch_agent(&mut inner, &emit, owner, agent_id, here, history);
                Ok(())
            }
        }
    }

    /// `agents_list` : sous-agents du chat courant, ou de tous (`all`).
    pub fn agents_list(&self, all: bool) -> Vec<SubAgentInfo> {
        let inner = self.lock();
        let here = inner.sessions.index();
        let mut out = Vec::new();
        for (i, sess) in inner.sessions.iter().enumerate() {
            if !all && i != here {
                continue;
            }
            for agent in sess.agents.iter() {
                out.push(SubAgentInfo::of(i, agent));
            }
        }
        out
    }

    /// `tell_send` : injecte le texte dans le chat `target_chat` (préfixé
    /// `[from Chat k]`, affiché aussitôt) et déclenche sa réponse ; enfilé
    /// s'il est occupé.
    pub fn tell_send(&self, target_chat: usize, text: String) -> Result<(), String> {
        let mut inner = self.lock();
        if inner.provider.is_none() {
            inner.push_system(String::from(NOT_CONNECTED));
            return Err(String::from(NOT_CONNECTED));
        }
        let here = inner.sessions.index();
        if target_chat == here {
            let msg = String::from("this is the current chat — type directly");
            inner.push_system(msg.clone());
            return Err(msg);
        }
        if inner.sessions.get(target_chat).is_none() {
            let msg = format!("chat {} not found", target_chat + 1);
            inner.push_system(msg.clone());
            return Err(msg);
        }
        let emit = |event: Event| self.emit(event);
        let envelope = format!("[from Chat {}] {text}", here + 1);
        let queued = drain_bus::deliver_tell(&mut inner, &emit, target_chat, envelope, 0);
        if queued {
            inner.push_system(format!("Chat {} busy — queued", target_chat + 1));
        } else {
            inner.push_system(format!("sent to Chat {}", target_chat + 1));
        }
        Ok(())
    }

    /// `msg_send` : parle à un chat (`2`, `@2`), un agent (`#3`, livré ici)
    /// ou tous les autres chats (`all`, en éventail). Enfile si occupé.
    pub fn msg_send(&self, target: String, text: String) -> Result<(), String> {
        if target.trim().eq_ignore_ascii_case("all") {
            return self.broadcast(text);
        }
        match parse_target(&target) {
            Some(Target::Chat(n)) => self.tell_send(n - 1, text),
            Some(Target::Agent(id)) => self.agent_ask(id, text),
            None => Err(format!("invalid target: {target} (use <n|@n|#id|all>)")),
        }
    }

    /// `link_add` : chaque réponse de `from` est retransmise à `to`
    /// (8 relais max). Extrémités validées (chat existant, agent connu).
    pub fn link_add(&self, from: LinkEndDto, to: LinkEndDto) -> Result<(), String> {
        let mut inner = self.lock();
        let (from, to) = (LinkEnd::from(from), LinkEnd::from(to));
        if let Err(msg) = check_link_end(&inner, from) {
            inner.push_system(msg.clone());
            return Err(msg);
        }
        if let Err(msg) = check_link_end(&inner, to) {
            inner.push_system(msg.clone());
            return Err(msg);
        }
        match inner.bus.link(from, to) {
            Ok(()) => {
                inner.push_system(format!("linked: {} → {}", from.label(), to.label()));
                Ok(())
            }
            Err(LinkError::Same) => {
                let msg = String::from("a link to itself is useless");
                inner.push_system(msg.clone());
                Err(msg)
            }
            Err(LinkError::Exists) => {
                let msg = String::from("link already present");
                inner.push_system(msg.clone());
                Err(msg)
            }
        }
    }

    /// `link_remove` : retire le lien (faux s'il n'existait pas).
    pub fn link_remove(&self, from: LinkEndDto, to: LinkEndDto) -> bool {
        let mut inner = self.lock();
        let (from, to) = (LinkEnd::from(from), LinkEnd::from(to));
        if inner.bus.unlink(from, to) {
            inner.push_system(format!("unlinked: {} → {}", from.label(), to.label()));
            true
        } else {
            inner.push_system(String::from("no link in that direction"));
            false
        }
    }

    /// `links_list` : liens + compteurs (`forwarded`/`capped`).
    pub fn links_list(&self) -> BusStateDto {
        let inner = self.lock();
        BusStateDto {
            links: inner
                .bus
                .links()
                .iter()
                .map(crate::dto::BusLinkDto::from)
                .collect(),
            forwarded: inner.bus.forwarded,
            capped: inner.bus.capped,
        }
    }

    /// Éventail `all` : dépose le texte dans tous les autres chats (affiché
    /// aussitôt partout, chaque libre déclenché, sinon enfilé).
    fn broadcast(&self, text: String) -> Result<(), String> {
        let mut inner = self.lock();
        if inner.provider.is_none() {
            inner.push_system(String::from(NOT_CONNECTED));
            return Err(String::from(NOT_CONNECTED));
        }
        let here = inner.sessions.index();
        let others: Vec<usize> = (0..inner.sessions.count()).filter(|i| *i != here).collect();
        if others.is_empty() {
            let msg = String::from("no other chat — /new to open one");
            inner.push_system(msg.clone());
            return Err(msg);
        }
        let emit = |event: Event| self.emit(event);
        let mut queued = 0usize;
        for idx in &others {
            let envelope = format!("[from Chat {}] {text}", here + 1);
            if drain_bus::deliver_tell(&mut inner, &emit, *idx, envelope, 0) {
                queued += 1;
            }
        }
        let fired = others.len() - queued;
        inner.push_system(format!(
            "broadcast to {} chats ({fired} triggered, {queued} queued)",
            others.len()
        ));
        Ok(())
    }
}

/// Spawn validé : provider + routage + messages (contexte tient).
struct Guarded {
    provider: std::sync::Arc<crate::provider::Provider>,
    owner: usize,
    deliver: usize,
    messages: Vec<(String, String)>,
}

/// Plan de spawn capturé sous verrou, spawné après (sans verrou).
struct SpawnPlan {
    pool: crate::pool::SpawnPool,
    provider: std::sync::Arc<crate::provider::Provider>,
    messages: Vec<(String, String)>,
    model: String,
    effort: Option<String>,
    tx: tokio::sync::mpsc::Sender<crate::core::Incoming>,
    owner: usize,
    agent_id: usize,
    deliver: usize,
    info: SubAgentInfo,
}

impl SpawnPlan {
    /// Capture le plan (spawn validé, agent inséré).
    fn capture(
        inner: &crate::core::CoreInner,
        guarded: Guarded,
        agent_id: usize,
        info: SubAgentInfo,
    ) -> Self {
        Self {
            pool: inner.pool.clone(),
            provider: guarded.provider,
            messages: guarded.messages,
            model: inner.effective_model_for(guarded.owner),
            effort: inner.thinking.clone(),
            tx: inner.tx.clone(),
            owner: guarded.owner,
            agent_id,
            deliver: guarded.deliver,
            info,
        }
    }

    /// Spawne l'appel agent (réponse routée en `Incoming::Agent`).
    fn spawn(self) {
        self.pool.spawn(async move {
            let reply = self
                .provider
                .complete_full(&self.messages, &self.model, self.effort.as_deref())
                .await;
            let _ = self
                .tx
                .send(crate::core::Incoming::Agent(
                    self.owner,
                    self.agent_id,
                    self.deliver,
                    reply,
                ))
                .await;
        });
    }
}

/// Gardes de spawn : connecté, livraison connue, contexte tient.
/// Rend provider + routage + messages (notes poussées si refus).
fn guard_agent_spawn(
    inner: &mut crate::core::CoreInner,
    prompt: &str,
    target_chat: Option<usize>,
) -> Result<Guarded, String> {
    let Some(provider) = inner.provider.clone() else {
        inner.push_system(String::from(NOT_CONNECTED));
        return Err(String::from(NOT_CONNECTED));
    };
    let owner = inner.sessions.index();
    let deliver = target_chat.unwrap_or(owner);
    if inner.sessions.get(deliver).is_none() {
        let msg = format!("chat {} not found", deliver + 1);
        inner.push_system(msg.clone());
        return Err(msg);
    }
    // Messages : agents.md (si chargé) + consigne questions + prompt.
    let mut messages = Vec::new();
    if let Some(md) = &inner.agents_md {
        messages.push((String::from("system"), md.clone()));
    }
    messages.push((
        String::from("system"),
        crate::question::QUESTION_SYSTEM.to_string(),
    ));
    messages.push((String::from("user"), prompt.to_string()));
    let limit = inner.context_limit_for(owner);
    let used = context_estimate(&messages);
    if used > limit {
        let msg = format!(
            "agent context full ({}), agent not started",
            fmt_context(used, limit)
        );
        inner.push_system(msg.clone());
        return Err(msg);
    }
    Ok(Guarded {
        provider,
        owner,
        deliver,
        messages,
    })
}

/// Insère l'agent `Running` : échange, workspace disque, livraison, note.
/// Rend son id global + sa vue (`SubAgentInfo`).
fn insert_agent(
    inner: &mut crate::core::CoreInner,
    owner: usize,
    deliver: usize,
    prompt: &str,
) -> Result<(usize, SubAgentInfo), String> {
    let exchange = vec![(String::from("user"), prompt.to_string())];
    let Some(agent_id) = inner.sessions.add_agent(owner, prompt, exchange) else {
        return Err(String::from("no chat"));
    };
    inner
        .sessions
        .attach_workspace(agent_id, crate::workspace::base().as_deref());
    if let Some((_, agent)) = inner.sessions.find_agent_mut(agent_id) {
        agent.deliver = deliver;
    }
    let note = if deliver == owner {
        format!("agent#{agent_id} started")
    } else {
        format!("agent#{agent_id} started → Chat {}", deliver + 1)
    };
    inner.push_system(note);
    let (o, agent) = inner
        .sessions
        .find_agent_mut(agent_id)
        .expect("agent créé ci-dessus");
    Ok((agent_id, SubAgentInfo::of(o, agent)))
}

/// Valide une extrémité de lien (chat existant, agent connu).
fn check_link_end(inner: &crate::core::CoreInner, end: LinkEnd) -> Result<(), String> {
    match end {
        LinkEnd::Chat(idx) if inner.sessions.get(idx).is_some() => Ok(()),
        LinkEnd::Chat(idx) => Err(format!("chat {} not found", idx + 1)),
        LinkEnd::Agent(id) if inner.sessions.has_agent(id) => Ok(()),
        LinkEnd::Agent(id) => Err(format!("agent#{id} unknown")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tests::test_core;
    use crate::provider::ProviderConfig;
    use crate::subagents::user;

    /// Core branché sur un provider injoignable (spawn sans réseau).
    fn wired(name: &str) -> (Core, crate::core::CoreChannels, std::path::PathBuf) {
        let (core, channels, dir) = test_core(name);
        let cfg = ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        core.lock().provider = Some(std::sync::Arc::new(
            crate::provider::Provider::new(cfg).unwrap(),
        ));
        (core, channels, dir)
    }

    #[tokio::test]
    async fn spawn_valide_ask_queue_et_liste() {
        let (core, _channels, dir) = wired("agents");
        assert!(core.agent_spawn(String::from("  "), None).await.is_err());
        let info = core
            .agent_spawn(String::from("creuse"), None)
            .await
            .unwrap();
        assert_eq!((info.id, info.owner, info.deliver), (1, 0, 0));
        assert_eq!(info.status, "running");
        assert!(info.last_text.is_none());
        // Occupé → enfilé, position 1.
        core.agent_ask(1, String::from("suite")).unwrap();
        assert!(core.agent_ask(42, String::from("x")).is_err());
        let all = core.agents_list(true);
        assert_eq!(all.len(), 1);
        assert_eq!(core.agents_list(false).len(), 1);
        core.add_chat();
        assert_eq!(core.agents_list(false).len(), 0);
        assert_eq!(core.agents_list(true).len(), 1);
        let targeted = core
            .agent_spawn(String::from("loin"), Some(0))
            .await
            .unwrap();
        assert_eq!((targeted.owner, targeted.deliver), (1, 0));
        assert!(core.agent_spawn(String::from("x"), Some(9)).await.is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn tell_msg_et_broadcast() {
        let (core, _channels, dir) = wired("tell");
        core.add_chat();
        core.lock().sessions.select(0);
        core.tell_send(1, String::from("bonjour")).unwrap();
        assert!(core.lock().sessions.get(1).unwrap().sending);
        assert!(core.tell_send(0, String::from("moi")).is_err());
        assert!(core.tell_send(9, String::from("x")).is_err());
        core.msg_send(String::from("all"), String::from("go"))
            .unwrap();
        assert!(core
            .msg_send(String::from("zzz"), String::from("x"))
            .is_err());
        // Agent libre → ask direct via #id.
        let id = core.lock().sessions.add_agent(0, "p", user("u")).unwrap();
        core.lock().sessions.find_agent_mut(id).unwrap().1.status =
            crate::sessions::AgentStatus::Done;
        core.msg_send(format!("#{id}"), String::from("relance"))
            .unwrap();
        assert_eq!(
            core.lock().sessions.find_agent_mut(id).unwrap().1.status,
            crate::sessions::AgentStatus::Running
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn liens_valides_doublons_et_bus_state() {
        let (core, _channels, dir) = test_core("links");
        let a = LinkEndDto::Chat { id: 0 };
        core.add_chat();
        let b = LinkEndDto::Chat { id: 1 };
        assert!(core.link_add(a, a).is_err());
        core.link_add(a, b).unwrap();
        assert!(core.link_add(a, b).is_err());
        assert!(core.link_add(a, LinkEndDto::Chat { id: 9 }).is_err());
        assert!(core.link_add(a, LinkEndDto::Agent { id: 7 }).is_err());
        let state = core.links_list();
        assert_eq!(state.links.len(), 1);
        assert!(core.link_remove(a, b));
        assert!(!core.link_remove(a, b));
        assert!(core.links_list().links.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn deconnecte_refuse_proprement() {
        let (core, _channels, dir) = test_core("down");
        assert!(core.tell_send(0, String::from("x")).is_err());
        assert!(core.agent_ask(1, String::from("x")).is_err());
        assert!(core
            .msg_send(String::from("all"), String::from("x"))
            .is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
