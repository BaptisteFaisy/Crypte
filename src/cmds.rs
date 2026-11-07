//! Handlers des commandes slash (`/usage`, `/new`, `/model`, …).
//!
//! `impl App` déporté hors `app.rs` (limite 500 lignes) : chaque handler
//! lit/écrit l'état via les accès `pub(crate)` de [`App`].

use crate::agents;
use crate::app::{App, Incoming};
use crate::bus::{LinkEnd, LinkError, QueuedAsk, QueuedTell, Target};
use crate::chat::Role;
use crate::commands::{self, Command, MsgTarget};
use crate::connect::NOT_CONNECTED;
use crate::history::send_window;
use crate::hub::Signal;
use crate::provider::{provider_label, ProviderConfig};
use crate::sessions::AskError;

/// Historique minimal (messages) pour lancer un compactage.
const COMPACT_MIN: usize = 6;
/// Instruction de résumé : FR, <20 lignes, faits/décisions/contexte.
const COMPACT_INSTRUCTION: &str = "Summarize this conversation in under 20 lines: established facts, decisions made, useful context for what follows. Reply with only the summary, no introduction.";

impl App {
    /// Exécute une commande locale (`/usage`, `/new`, `/model`, …).
    pub(crate) fn run_command(&mut self, text: &str) {
        match commands::parse(text) {
            Some(Command::Usage) => self.cmd_usage(),
            Some(Command::New) => self.add_chat(),
            Some(Command::Help) => self.push_system(commands::help_text().to_string()),
            Some(Command::Model(arg)) => self.cmd_model(arg),
            Some(Command::Thinking(arg)) => self.cmd_thinking(arg),
            Some(Command::Agent { target, prompt }) => self.cmd_agent(target, prompt),
            Some(Command::Agents { all }) => self.cmd_agents(all),
            Some(Command::Ask { agent, text }) => self.cmd_ask(agent, text),
            Some(Command::Tell { target, text }) => self.cmd_tell(target, text),
            Some(Command::Msg { to, text }) => self.cmd_msg(to, text),
            Some(Command::Link { from, to }) => self.cmd_link(from, to),
            Some(Command::Links) => self.push_system(self.bus.list_text()),
            Some(Command::Unlink { from, to }) => self.cmd_unlink(from, to),
            Some(Command::Compact) => self.cmd_compact(),
            Some(Command::Hint(msg)) => self.push_system(msg.to_string()),
            Some(Command::Connect(arg)) => self.cmd_connect(arg),
            Some(Command::Permissions(arg)) => self.cmd_permissions(arg),
            Some(Command::Web(query)) => self.cmd_web(query),
            Some(Command::Unknown(word)) => {
                self.push_system(format!("unknown command: {word} — /help for help"));
            }
            None => self.push_system(String::from("unknown command — /help for help")),
        }
    }

    /// `/usage` : tokens du chat courant + total session (+ modèle/thinking/agents.md).
    fn cmd_usage(&mut self) {
        let total = self.sessions.total_ledger();
        let model = self.effective_model();
        let thinking = self.thinking_text();
        let mut msg =
            commands::usage_text(&self.sessions.current().ledger, &total, &model, &thinking);
        msg.push('\n');
        msg.push_str(&agents::usage_line(self.agents_md.as_deref()));
        let used = {
            let sess = self.sessions.current();
            let window = sess.history.clone();
            let window = send_window(&window, sess.pinned_system);
            crate::tokens::context_estimate(&window)
        };
        msg.push('\n');
        msg.push_str(&format!(
            "context: {}",
            crate::tokens::fmt_context(used, self.context_limit())
        ));
        self.push_system(msg);
    }

    /// `/model` : affiche le modèle effectif, ou change celui du chat.
    fn cmd_model(&mut self, arg: Option<String>) {
        match arg {
            None => self.push_system(format!(
                "model: {} (context window: {})",
                self.effective_model(),
                self.context_limit()
            )),
            Some(name) if name == "reset" || name == "default" => {
                self.sessions.current_mut().model = None;
                self.push_system(format!(
                    "model: {} (default, context window: {})",
                    self.effective_model(),
                    self.context_limit()
                ));
            }
            Some(name) => {
                self.sessions.current_mut().model = Some(name.clone());
                self.push_system(format!(
                    "model: {name} (context window: {})",
                    self.context_limit()
                ));
            }
        }
    }

    /// `/thinking` : affiche ou change l'effort de raisonnement global.
    fn cmd_thinking(&mut self, arg: Option<String>) {
        match arg {
            None => self.push_system(format!("thinking: {}", self.thinking_text())),
            Some(v) if v == "default" => {
                self.thinking = None;
                self.push_system(String::from("thinking: default"));
            }
            Some(v) if commands::valid_effort(&v) => {
                self.thinking = Some(v.clone());
                self.push_system(format!("thinking: {v}"));
            }
            Some(v) => {
                let mut vals = effort_values();
                vals.push("default");
                self.push_system(format!("invalid effort: {v} — values: {}", vals.join(", ")));
            }
        }
    }

    /// `/agent [@<n>] <texte>` : sous-agent (agents.md préfixé si chargé,
    /// sans historique) ; réponse livrée au chat n si ciblé, sinon ici.
    /// Déconnecté → hint `/connect`, sans créer d'agent.
    fn cmd_agent(&mut self, target: Option<usize>, prompt: String) {
        let Some(provider) = self.provider.clone() else {
            self.push_system(String::from(NOT_CONNECTED));
            return;
        };
        let id = self.sessions.index();
        let deliver = match target {
            None => id,
            Some(n) if self.sessions.get(n - 1).is_some() => n - 1,
            Some(n) => {
                self.push_system(format!("chat {n} not found"));
                return;
            }
        };
        let mut messages = Vec::with_capacity(2);
        if let Some(content) = &self.agents_md {
            messages.push((String::from("system"), content.clone()));
        }
        messages.push(("user".to_string(), prompt.clone()));
        let used = crate::tokens::context_estimate(&messages);
        if used > self.context_limit() {
            self.push_system(format!(
                "agent context full ({}), agent not started",
                crate::tokens::fmt_context(used, self.context_limit())
            ));
            return;
        }
        let Some(agent_id) = self.sessions.add_agent(id, &prompt, messages.clone()) else {
            return;
        };
        if deliver == id {
            self.push_system(format!("agent#{agent_id} started"));
        } else {
            self.push_system(format!("agent#{agent_id} started → Chat {}", deliver + 1));
        }
        let model = self.effective_model();
        let effort = self.thinking.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let reply = provider
                .complete_full(&messages, &model, effort.as_deref())
                .await;
            let _ = tx.send(Incoming::Agent(id, agent_id, deliver, reply));
        });
    }

    /// `/agents [all]` : sous-agents du chat courant, ou de tous les
    /// chats (`all`, préfixés `Chat k`) ; file d'attente entre crochets.
    fn cmd_agents(&mut self, all: bool) {
        let here = self.sessions.index();
        let mut rows = Vec::new();
        for (i, sess) in self.sessions.iter().enumerate() {
            if !all && i != here {
                continue;
            }
            let scope = if all {
                format!("Chat {} ", i + 1)
            } else {
                String::new()
            };
            for a in sess.agents.iter() {
                let queued = if a.inbox.is_empty() {
                    String::new()
                } else {
                    format!(" [queued={}]", a.inbox.len())
                };
                rows.push(format!(
                    "{scope}#{id} {status} in={pin} out={pout} {prompt}{queued}",
                    scope = scope,
                    id = a.id,
                    status = a.status.label(),
                    pin = a.prompt_tokens,
                    pout = a.completion_tokens,
                    prompt = a.prompt,
                    queued = queued
                ));
            }
        }
        if rows.is_empty() {
            self.push_system(String::from("no agents"));
        } else {
            self.push_system(rows.join("\n"));
        }
    }

    /// `/ask #<id> <texte>` : suite de conversation avec l'agent (cherché
    /// dans tous les chats, réponse livrée ici) ; enfilé s'il est en cours
    /// (déclenché dès qu'il est libre, zéro réessaie).
    fn cmd_ask(&mut self, agent_id: usize, text: String) {
        if self.provider.is_none() {
            self.push_system(String::from(NOT_CONNECTED));
            return;
        }
        let here = self.sessions.index();
        match self.sessions.ask_agent(agent_id, text.clone()) {
            Err(AskError::Unknown) => {
                self.push_system(format!("agent#{agent_id} unknown"));
            }
            Err(AskError::Busy) => {
                let pos = self.enqueue_ask(agent_id, here, text);
                self.push_system(format!("agent#{agent_id} busy — queued (position {pos})"));
            }
            Ok((owner, history)) => {
                if let Some((_, agent)) = self.sessions.find_agent_mut(agent_id) {
                    agent.auto_hops = 0;
                }
                self.launch_agent(owner, agent_id, here, history);
            }
        }
    }

    /// Enfile un ask sur l'agent (connu : `Busy` le prouve) ; rend la position.
    fn enqueue_ask(&mut self, agent_id: usize, deliver: usize, text: String) -> usize {
        let (_, agent) = self
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

    /// `/tell <n> <texte>` : injecte le texte dans le chat n (préfixé
    /// `[de Chat k]`, affiché aussitôt) et déclenche sa réponse ; s'il est
    /// occupé, enfilé (déclenché dès qu'il est libre, zéro réessaie).
    fn cmd_tell(&mut self, target: usize, text: String) {
        if self.provider.is_none() {
            self.push_system(String::from(NOT_CONNECTED));
            return;
        }
        let here = self.sessions.index();
        let idx = target - 1;
        if idx == here {
            self.push_system(String::from("this is the current chat — type directly"));
            return;
        }
        if self.sessions.get(idx).is_none() {
            self.push_system(format!("chat {target} not found"));
            return;
        }
        self.hub.signal(Signal::Tell {
            from: here,
            to: idx,
        });
        let envelope = format!("[from Chat {}] {text}", here + 1);
        if self.deliver_tell(idx, envelope, 0) {
            self.push_system(format!("Chat {target} busy — queued"));
        } else {
            self.push_system(format!("sent to Chat {target}"));
        }
    }

    /// Dépose un tell (user ou auto) dans le chat `idx` : affiché aussitôt,
    /// déclenché si libre, enfilé sinon. Rend vrai si enfilé.
    pub(crate) fn deliver_tell(&mut self, idx: usize, envelope: String, hops: u8) -> bool {
        let dest = self.sessions.get_mut(idx).expect("chat validé ci-dessus");
        dest.chat.push(Role::User, envelope.clone());
        if dest.sending {
            dest.inbox.push_back(QueuedTell {
                envelope,
                hops,
                trigger: true,
            });
            return true;
        }
        dest.history.push((String::from("user"), envelope));
        dest.auto_hops = hops;
        self.launch_chat(idx);
        false
    }

    /// `/msg <cible|all> <texte>` : parle à un chat (`2`, `@2`), un agent
    /// (`#3`, livré ici) ou tous les autres chats (`all`, en éventail).
    /// Enfile si occupé, jamais refusé.
    fn cmd_msg(&mut self, to: MsgTarget, text: String) {
        if self.provider.is_none() {
            self.push_system(String::from(NOT_CONNECTED));
            return;
        }
        match to {
            MsgTarget::One(Target::Chat(n)) => self.cmd_tell(n, text),
            MsgTarget::One(Target::Agent(id)) => self.cmd_ask(id, text),
            MsgTarget::All => self.cmd_broadcast(text),
        }
    }

    /// `/msg all` : dépose le texte dans tous les autres chats (affiché
    /// aussitôt partout, chaque libre déclenché en parallèle, sinon enfilé).
    fn cmd_broadcast(&mut self, text: String) {
        let here = self.sessions.index();
        let others: Vec<usize> = (0..self.sessions.count()).filter(|i| *i != here).collect();
        if others.is_empty() {
            self.push_system(String::from("no other chat — /new to open one"));
            return;
        }
        let mut queued = 0usize;
        for idx in &others {
            self.hub.signal(Signal::Tell {
                from: here,
                to: *idx,
            });
            let envelope = format!("[from Chat {}] {text}", here + 1);
            if self.deliver_tell(*idx, envelope, 0) {
                queued += 1;
            }
        }
        let fired = others.len() - queued;
        self.push_system(format!(
            "broadcast to {} chats ({fired} triggered, {queued} queued)",
            others.len()
        ));
    }

    /// `/link <a> <b>` : chaque réponse de `a` est retransmise à `b`
    /// (déclenche sa réponse ; 8 relais max, voir `/links`). Extrémités
    /// validées (chat existant, agent connu).
    fn cmd_link(&mut self, from: LinkEnd, to: LinkEnd) {
        if let Err(msg) = self.check_link_end(from) {
            self.push_system(msg);
            return;
        }
        if let Err(msg) = self.check_link_end(to) {
            self.push_system(msg);
            return;
        }
        match self.bus.link(from, to) {
            Ok(()) => self.push_system(format!("linked: {} → {}", from.label(), to.label())),
            Err(LinkError::Same) => {
                self.push_system(String::from("a link to itself is useless"));
            }
            Err(LinkError::Exists) => self.push_system(String::from("link already present")),
        }
    }

    /// `/unlink <a> <b>` : retire le lien (note s'il n'existait pas).
    fn cmd_unlink(&mut self, from: LinkEnd, to: LinkEnd) {
        if self.bus.unlink(from, to) {
            self.push_system(format!("unlinked: {} → {}", from.label(), to.label()));
        } else {
            self.push_system(String::from("no link in that direction"));
        }
    }

    /// Valide une extrémité de lien (chat existant, agent connu).
    fn check_link_end(&self, end: LinkEnd) -> Result<(), String> {
        match end {
            LinkEnd::Chat(idx) if self.sessions.get(idx).is_some() => Ok(()),
            LinkEnd::Chat(idx) => Err(format!("chat {} not found", idx + 1)),
            LinkEnd::Agent(id) if self.sessions.has_agent(id) => Ok(()),
            LinkEnd::Agent(id) => Err(format!("agent#{id} unknown")),
        }
    }

    /// Limite effective du chat courant : fenêtre du modèle, plafonnée à 1M.
    pub(crate) fn context_limit(&self) -> u64 {
        crate::tokens::context_window(&self.effective_model()).min(crate::tokens::CONTEXT_LIMIT)
    }

    /// Contexte estimé du chat courant : fenêtre d'envoi + saisie en cours.
    pub(crate) fn context_used(&self) -> u64 {
        let sess = self.sessions.current();
        let mut window = sess.history.clone();
        if !sess.chat.input.trim().is_empty() {
            window.push(("user".to_string(), sess.chat.input.clone()));
        }
        let window = send_window(&window, sess.pinned_system);
        crate::tokens::context_estimate(&window)
    }

    /// Fenêtre à envoyer si elle dépasse la limite (tokens estimés), sinon `None`.
    pub(crate) fn context_over_limit(&self, text: &str) -> Option<u64> {
        let sess = self.sessions.current();
        let mut window = sess.history.clone();
        window.push(("user".to_string(), text.to_string()));
        let window = send_window(&window, sess.pinned_system);
        let used = crate::tokens::context_estimate(&window);
        (used > self.context_limit()).then_some(used)
    }

    /// `/compact` : résume l'historique (agents.md exclu du résumé méta,
    /// mais conservé en tête après compaction — gardé : pinné? + résumé + 4).
    /// Déconnecté → hint `/connect`.
    fn cmd_compact(&mut self) {
        let Some(provider) = self.provider.clone() else {
            self.push_system(String::from(NOT_CONNECTED));
            return;
        };
        let id = self.sessions.index();
        let sess = self.sessions.current();
        let pinned = sess.pinned_system;
        let history = sess.history.clone();
        if history.len() < COMPACT_MIN {
            self.push_system(String::from("nothing to compact"));
            return;
        }
        let model = self.effective_model();
        let effort = self.thinking.clone();
        let tx = self.tx.clone();
        let mut messages = vec![(String::from("system"), String::from(COMPACT_INSTRUCTION))];
        messages.extend(agents::without_pinned(&history, pinned).iter().cloned());
        tokio::spawn(async move {
            let reply = provider
                .complete_full(&messages, &model, effort.as_deref())
                .await;
            let _ = tx.send(Incoming::Compact(id, reply));
        });
    }

    /// `/connect [openai|muse]` : statut (sans arg), store Muse ou flow clavier.
    /// Inconnu → erreur + valeurs (validation dans `start_connect_flow`).
    fn cmd_connect(&mut self, arg: Option<String>) {
        match arg {
            None => self.cmd_connect_status(),
            Some(name) if name == "muse" => self.cmd_connect_muse(),
            Some(name) => self.start_connect_flow(&name),
        }
    }

    /// `/connect` sans arg : provider + modèle, ou non connecté + usage.
    fn cmd_connect_status(&mut self) {
        match self.provider.as_ref() {
            Some(provider) => {
                let cfg = provider.config();
                self.push_system(format!(
                    "connected to {} ({})\nusage: /connect openai|muse",
                    provider_label(&cfg.base_url),
                    cfg.model
                ));
            }
            None => self.push_system(String::from(NOT_CONNECTED)),
        }
    }

    /// `/connect muse` : store Muse si lisible, sinon saisie manuelle.
    fn cmd_connect_muse(&mut self) {
        let from_store = ProviderConfig::muse_auth_path().and_then(ProviderConfig::from_muse_auth);
        match from_store {
            Ok(cfg) => self.apply_connected(cfg, "muse"),
            Err(_) => {
                self.push_system(String::from("Muse store unreadable, manual input"));
                self.start_connect_flow("muse");
            }
        }
    }

    /// `/web <query>` : web search + LLM synthesis with cited sources.
    /// Async (`Incoming::Web`) ; works disconnected too (raw results).
    fn cmd_web(&mut self, query: String) {
        let id = self.sessions.index();
        let provider = self.provider.clone();
        let model = self.effective_model();
        let effort = self.thinking.clone();
        let tx = self.tx.clone();
        self.push_system(format!("web search: {query}…"));
        tokio::spawn(async move {
            let reply =
                crate::web::answer(query, provider.as_deref(), &model, effort.as_deref()).await;
            let _ = tx.send(Incoming::Web(id, reply));
        });
    }
}

/// Valeurs d'effort reconnues (sondées via `valid_effort`, source unique).
fn effort_values() -> Vec<&'static str> {
    [
        "minimal", "none", "low", "medium", "high", "xhigh", "max", "auto",
    ]
    .into_iter()
    .filter(|v| commands::valid_effort(v))
    .collect()
}
