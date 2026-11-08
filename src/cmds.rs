//! Handlers des commandes slash (`/usage`, `/new`, `/model`, …).
//!
//! `impl App` déporté hors `app.rs` (limite 500 lignes) : chaque handler
//! lit/écrit l'état via les accès `pub(crate)` de [`App`].

use crate::agents;
use crate::app::{App, Incoming};
use crate::bus::{QueuedAsk, QueuedTell, Target};
use crate::chat::Role;
use crate::commands::{self, Command, MsgTarget};
use crate::connect::NOT_CONNECTED;
use crate::events::Event;
use crate::history::window_estimate;
use crate::hub::Signal;
use crate::sessions::AskError;
use crate::texts;

/// Historique minimal (messages) pour lancer un compactage.
const COMPACT_MIN: usize = 6;
/// Instruction de résumé : FR, <20 lignes, faits/décisions/contexte.
const COMPACT_INSTRUCTION: &str = "Summarize this conversation in under 20 lines: established facts, decisions made, useful context for what follows. Reply with only the summary, no introduction.";

impl App {
    /// Exécute une commande locale (`/usage`, `/new`, `/model`, …).
    /// Sans chat, seul `/new` passe (les autres exigent un courant).
    pub(crate) fn run_command(&mut self, text: &str) {
        let cmd = commands::parse(text);
        if self.sessions.count() == 0 && !matches!(cmd, Some(Command::New)) {
            return; // aucun chat : [+] / Ctrl+N / /new pour recommencer
        }
        match cmd {
            Some(Command::Usage) => self.cmd_usage(),
            Some(Command::New) => self.add_chat(),
            Some(Command::Clear) => self.cmd_clear(),
            Some(Command::Bestiary) => self.cmd_bestiary(),
            Some(Command::Help) => self.push_system(texts::help_text().to_string()),
            Some(Command::Model(arg)) => self.cmd_model(arg),
            Some(Command::Thinking(arg)) => self.cmd_thinking(arg),
            Some(Command::Agent { target, prompt }) => self.cmd_agent(target, prompt),
            Some(Command::Agents { all, voir }) => self.cmd_agents(all, voir),
            Some(Command::Ask { agent, text }) => self.cmd_ask(agent, text),
            Some(Command::Tell { target, text }) => self.cmd_tell(target, text),
            Some(Command::Msg { to, text }) => self.cmd_msg(to, text),
            Some(Command::Link { from, to }) => self.cmd_link(from, to),
            Some(Command::Links) => self.push_system(self.bus.list_text()),
            Some(Command::Unlink { from, to }) => self.cmd_unlink(from, to),
            Some(Command::Compact) => self.cmd_compact(),
            Some(Command::Hint(msg)) => self.push_system(msg.to_string()),
            Some(Command::Connect(arg)) => self.cmd_connect(arg),
            Some(Command::Settings(arg)) => self.open_settings(arg),
            Some(Command::Permissions(arg)) => self.cmd_permissions(arg),
            Some(Command::Web(query)) => self.cmd_web(query),
            Some(Command::Tools(arg)) => self.cmd_tools(arg),
            Some(Command::Kiro) => self.cmd_kiro(),
            Some(Command::Critic(arg)) => self.cmd_critic(arg),
            Some(Command::Vps { args }) => self.cmd_vps(args),
            Some(Command::SpecNew { name, idea }) => self.cmd_spec_new(name, idea),
            Some(Command::SpecShow { name }) => self.cmd_spec_show(name),
            Some(Command::SpecRun { name }) => self.cmd_spec_run(name),
            Some(Command::Mcp(arg)) => self.cmd_mcp(arg),
            Some(Command::Hooks) => self.cmd_hooks(),
            Some(Command::Cloud(args)) => self.cmd_cloud(args),
            Some(Command::Workflow(args)) => self.cmd_workflow(args),
            Some(Command::Skills) => self.cmd_skills(),
            Some(Command::Steer(name)) => self.cmd_steer(name),
            Some(Command::Skill { name, text }) => self.cmd_skill(name, text),
            Some(Command::Checkpoints) => self.cmd_checkpoints(),
            Some(Command::Rewind { target }) => self.cmd_rewind(target),
            Some(Command::Remember(text)) => self.cmd_remember(text),
            Some(Command::Unknown(word)) => {
                self.push_system(format!("unknown command: {word} — /help for help"));
            }
            None => self.push_system(String::from("unknown command — /help for help")),
        }
    }

    /// `/clear` : vide la conversation du chat courant (pinné conservé) ;
    /// refusé si une réponse est en cours (elle retomberait dedans).
    fn cmd_clear(&mut self) {
        if self.sessions.current().sending {
            self.push_system(String::from("reply in flight — retry"));
            return;
        }
        self.sessions.current_mut().clear();
        self.push_system(String::from("conversation cleared"));
    }

    /// `/bestiary` : une créature au hasard dans le panneau droit
    /// (quelques secondes) + son nom et sa légende dans le chat.
    fn cmd_bestiary(&mut self) {
        let creature = self.apparition.show_random();
        self.push_system(format!("{} — {}", creature.name, creature.lore));
    }

    /// `/usage` : tokens du chat courant + total session (+ modèle/thinking/agents.md,
    /// almanach toutes sessions).
    fn cmd_usage(&mut self) {
        // D'autres cryptes ont pu écrire : relire avant d'afficher.
        self.almanac.refresh();
        let total = self.sessions.total_ledger();
        let model = self.effective_model();
        let thinking = self.thinking_text();
        let mut msg =
            crate::tokens::usage_text(&self.sessions.current().ledger, &total, &model, &thinking);
        msg.push('\n');
        msg.push_str(&agents::usage_line(self.agents_md.as_deref()));
        let sess = self.sessions.current();
        let used = window_estimate(&sess.history, sess.pinned_system, None);
        msg.push('\n');
        msg.push_str(&format!(
            "context: {} | heap (chat): {} o",
            crate::tokens::fmt_context(used, self.context_limit()),
            self.sessions.current().heap_bytes()
        ));
        let (flight, cap) = (self.pool.in_flight(), self.pool.cap());
        msg.push_str(&format!("\npool: {flight}/{cap} llm in flight"));
        let earned = self.sessions.grand_total();
        let wallet = self.shop.wallet();
        msg.push_str(&format!(
            "\nwallet: {} coins ({} earned, {} spent)",
            wallet.balance(earned),
            earned,
            wallet.spent()
        ));
        msg.push('\n');
        msg.push_str(&self.almanac.usage_block(crate::almanac::today()));
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
                let mut vals = commands::effort_values();
                vals.push("default");
                self.push_system(format!("invalid effort: {v} — values: {}", vals.join(", ")));
            }
        }
    }

    /// `/agents [all|#id]` : sous-agents du chat courant, ou de tous
    /// les chats (`all`, préfixés `Chat k`) ; `#id` affiche l'échange
    /// complet de l'agent (cherché dans tous les chats) ; file d'attente
    /// entre crochets ; customs `.kiro/agents/` listés à la suite
    /// (`custom <nom>`).
    fn cmd_agents(&mut self, all: bool, voir: Option<usize>) {
        if let Some(id) = voir {
            let text = match self.sessions.find_agent_mut(id) {
                Some((_, agent)) => agent.transcript(),
                None => format!("agent#{id} unknown"),
            };
            self.push_system(text);
            return;
        }
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
        for line in crate::kiro::agents_md::lines_of_app(self) {
            rows.push(line);
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
        // Premier envoi réel : lecture unique d'agents.md (pin + annonce).
        self.ensure_agents_loaded();
        let dest = self.sessions.get_mut(idx).expect("chat validé ci-dessus");
        dest.chat.push(Role::User, envelope.clone());
        self.events.emit(&Event::user_msg(idx, &envelope));
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

    /// Limite effective du chat courant : fenêtre du modèle, plafonnée à 1M.
    pub(crate) fn context_limit(&self) -> u64 {
        crate::tokens::context_window(&self.effective_model()).min(crate::tokens::CONTEXT_LIMIT)
    }

    /// Contexte estimé du chat courant : fenêtre d'envoi + saisie en
    /// cours, sans copie (même somme que la fenêtre réellement envoyée).
    pub(crate) fn context_used(&self) -> u64 {
        let idx = self.sessions.index();
        let Some(sess) = self.sessions.get(idx) else {
            return 0; // aucun chat : jauge à zéro
        };
        let input = sess.chat.input.trim();
        let pending = if input.is_empty() { None } else { Some(input) };
        window_estimate(&sess.history, sess.pinned_system, pending)
    }

    /// Fenêtre à envoyer si elle dépasse la limite (tokens estimés), sinon `None`.
    pub(crate) fn context_over_limit(&self, text: &str) -> Option<u64> {
        let sess = self.sessions.current();
        let used = window_estimate(&sess.history, sess.pinned_system, Some(text));
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
        if self.sessions.current().history.len() < COMPACT_MIN {
            self.push_system(String::from("nothing to compact"));
            return;
        }
        self.sessions.current_mut().waiting = true;
        let sess = self.sessions.current();
        let pinned = sess.pinned_system;
        let history = sess.history.clone();
        let model = self.effective_model();
        let effort = self.thinking.clone();
        let tx = self.tx.clone();
        let mut messages = vec![(String::from("system"), String::from(COMPACT_INSTRUCTION))];
        messages.extend(agents::without_pinned(&history, pinned).iter().cloned());
        self.pool.spawn(async move {
            let reply = provider
                .complete_full(&messages, &model, effort.as_deref())
                .await;
            let _ = tx.send(Incoming::Compact(id, reply)).await;
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderConfig;

    /// App de test (clé factice, aucun appel réseau).
    fn test_app() -> App {
        App::from_config(ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        })
        .unwrap()
    }

    #[test]
    fn clear_vide_le_chat_et_refuse_si_envoi() {
        let mut app = test_app();
        let current = app.sessions.current_mut();
        current.chat.push(Role::User, "salut");
        current
            .history
            .push((String::from("user"), String::from("salut")));
        app.run_command("/clear");
        let last = app.sessions.current().chat.messages().last().unwrap();
        assert_eq!(last.text, "conversation cleared");
        assert_eq!(app.sessions.current().chat.messages().len(), 1);
        assert!(app.sessions.current().history.is_empty());
        // Envoi en cours : refusé, rien n'est effacé.
        app.sessions.current_mut().sending = true;
        app.run_command("/clear");
        let last = app.sessions.current().chat.messages().last().unwrap();
        assert_eq!(last.text, "reply in flight — retry");
        assert_eq!(app.sessions.current().chat.messages().len(), 2);
    }

    #[test]
    fn sans_chat_seul_new_passe_les_autres_sont_ignores() {
        let mut app = test_app();
        app.close_chat(0);
        assert_eq!(app.sessions.count(), 0);
        app.run_command("/usage"); // exigent un courant : ignorés, sans panique
        app.run_command("/model spectre-1");
        app.run_command("/compact");
        app.run_command("/clear");
        assert_eq!(app.sessions.count(), 0);
        app.run_command("/new");
        assert_eq!(app.sessions.count(), 1);
        assert_eq!(app.sessions.current().name, "Chat 1");
    }

    #[test]
    fn usage_command_shows_almanac_block() {
        let mut app = test_app();
        app.almanac.note(crate::almanac::today(), 100, 50);
        app.run_command("/usage");
        let last = app.sessions.current().chat.messages().last().unwrap();
        assert!(last.text.contains("ALMANAC (all sessions)"), "{last:?}");
        assert!(
            last.text.contains("tonight: 150 (in=100 out=50 calls=1)"),
            "{last:?}"
        );
    }

    #[test]
    fn agents_diese_montre_echange_et_inconnu_signale() {
        let mut app = test_app();
        app.add_chat();
        let history = vec![(String::from("user"), String::from("u"))];
        app.sessions.add_agent(0, "loin", history).unwrap();
        // Agent du chat 1, vu depuis le chat 2 (recherche globale).
        app.run_command("/agents #1");
        let last = app.sessions.current().chat.messages().last().unwrap();
        assert!(last.text.starts_with("#1 running — loin"), "{}", last.text);
        assert!(last.text.contains("\nyou: u"), "{}", last.text);
        app.run_command("/agents #42");
        let last = app.sessions.current().chat.messages().last().unwrap();
        assert_eq!(last.text, "agent#42 unknown");
    }
}
