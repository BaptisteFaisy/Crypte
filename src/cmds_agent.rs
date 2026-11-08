//! Handler `/agent` (extrait de `cmds.rs`, limite 500 lignes).
//!
//! `impl App` déporté : lit/écrit l'état via les accès `pub(crate)` de [`App`].

use crate::app::{App, Incoming};
use crate::connect::NOT_CONNECTED;
use crate::events::Event;

impl App {
    /// `/agent [@<n>] <texte>` : sous-agent (agents.md préfixé si chargé,
    /// sans historique) ; réponse livrée au chat n si ciblé, sinon ici.
    /// `<nom-custom>` en tête (voir `agents_md`) = prompt custom en système.
    /// Déconnecté → hint `/connect`, sans créer d'agent.
    pub(crate) fn cmd_agent(&mut self, target: Option<usize>, prompt: String) {
        let Some(provider) = self.provider.clone() else {
            self.push_system(String::from(NOT_CONNECTED));
            return;
        };
        let id = self.sessions.index();
        let Some(deliver) = self.resolve_agent_deliver(target, id) else {
            return;
        };
        // Premier envoi réel : lecture unique d'agents.md (pin + annonce).
        self.ensure_agents_loaded();
        let spawn =
            crate::kiro::agents_md::spawn_messages(self, self.agents_md.as_deref(), &prompt);
        let (mut messages, prompt, filter) = (spawn.messages, spawn.prompt, spawn.filter);
        // Consigne des questions IA devant le prompt (l'estimation suit, honnête).
        let at = messages.len().saturating_sub(1);
        messages.insert(
            at,
            (
                String::from("system"),
                crate::question::QUESTION_SYSTEM.to_string(),
            ),
        );
        let custom_model = spawn.model;
        let used = crate::tokens::context_estimate(&messages);
        if used > self.context_limit() {
            self.push_system(format!(
                "agent context full ({}), agent not started",
                crate::tokens::fmt_context(used, self.context_limit())
            ));
            return;
        }
        let exchange = vec![(String::from("user"), prompt.clone())];
        let Some(agent_id) = self.sessions.add_agent(id, &prompt, exchange) else {
            return;
        };
        self.sessions
            .attach_workspace(agent_id, crate::workspace::base().as_deref());
        if let Some((_, agent)) = self.sessions.find_agent_mut(agent_id) {
            agent.tool_filter = filter.clone();
        }
        self.events
            .emit(&Event::agent_spawned(agent_id, id, deliver, &prompt));
        self.hooks_fire_agent_spawn(id, &prompt);
        let line = if deliver == id {
            crate::agent_view::spawn_line(agent_id, None, &prompt)
        } else {
            crate::agent_view::spawn_line(agent_id, Some(deliver + 1), &prompt)
        };
        self.push_system(line);
        let model = custom_model.unwrap_or_else(|| self.effective_model());
        let effort = self.thinking.clone();
        let tx = self.tx.clone();
        let rules = self.t5.rules.clone();
        let mode = self.permissions;
        let mcp = self.mcp.clone();
        let tooled = self.tools_on;
        self.pool.spawn(async move {
            let ctx = crate::tools::agent::Ctx {
                provider: &provider,
                model: &model,
                effort: effort.as_deref(),
                rules: &rules,
                mode,
                mcp: &mcp,
                chat: id,
            };
            let reply = crate::tools::agent::agent_reply(ctx, messages, &filter, tooled).await;
            let _ = tx.send(Incoming::Agent(id, agent_id, deliver, reply)).await;
        });
    }

    /// Chat de livraison `/agent` (`None` + note si cible inconnue).
    fn resolve_agent_deliver(&mut self, target: Option<usize>, id: usize) -> Option<usize> {
        match target {
            None => Some(id),
            Some(n) if self.sessions.get(n - 1).is_some() => Some(n - 1),
            Some(n) => {
                self.push_system(format!("chat {n} not found"));
                None
            }
        }
    }
}
