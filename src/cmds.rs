//! Handlers des commandes slash (`/usage`, `/new`, `/model`, …).
//!
//! `impl App` déporté hors `app.rs` (limite 500 lignes) : chaque handler
//! lit/écrit l'état via les accès `pub(crate)` de [`App`].

use std::sync::Arc;

use crate::agents;
use crate::app::{App, Incoming};
use crate::commands::{self, Command};

/// Historique minimal (messages) pour lancer un compactage.
const COMPACT_MIN: usize = 6;
/// Instruction de résumé : FR, <20 lignes, faits/décisions/contexte.
const COMPACT_INSTRUCTION: &str = "Résume cette conversation en moins de 20 lignes : faits établis, décisions prises, contexte utile pour la suite. Réponds uniquement avec le résumé, sans introduction.";

impl App {
    /// Exécute une commande locale (`/usage`, `/new`, `/model`, …).
    pub(crate) fn run_command(&mut self, text: &str) {
        match commands::parse(text) {
            Some(Command::Usage) => self.cmd_usage(),
            Some(Command::New) => self.add_chat(),
            Some(Command::Help) => self.push_system(commands::help_text().to_string()),
            Some(Command::Model(arg)) => self.cmd_model(arg),
            Some(Command::Thinking(arg)) => self.cmd_thinking(arg),
            Some(Command::Agent(prompt)) => self.cmd_agent(prompt),
            Some(Command::Agents) => self.cmd_agents(),
            Some(Command::Compact) => self.cmd_compact(),
            Some(Command::Unknown(word)) => {
                self.push_system(format!("commande inconnue : {word} — /help pour l'aide"));
            }
            None => self.push_system(String::from("commande inconnue — /help pour l'aide")),
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
        self.push_system(msg);
    }

    /// `/model` : affiche le modèle effectif, ou change celui du chat.
    fn cmd_model(&mut self, arg: Option<String>) {
        match arg {
            None => self.push_system(format!("modèle : {}", self.effective_model())),
            Some(name) if name == "reset" || name == "default" => {
                self.sessions.current_mut().model = None;
                self.push_system(format!("modèle : {} (défaut)", self.effective_model()));
            }
            Some(name) => {
                self.sessions.current_mut().model = Some(name.clone());
                self.push_system(format!("modèle : {name}"));
            }
        }
    }

    /// `/thinking` : affiche ou change l'effort de raisonnement global.
    fn cmd_thinking(&mut self, arg: Option<String>) {
        match arg {
            None => self.push_system(format!("thinking : {}", self.thinking_text())),
            Some(v) if v == "default" => {
                self.thinking = None;
                self.push_system(String::from("thinking : default"));
            }
            Some(v) if commands::valid_effort(&v) => {
                self.thinking = Some(v.clone());
                self.push_system(format!("thinking : {v}"));
            }
            Some(v) => {
                let mut vals = effort_values();
                vals.push("default");
                self.push_system(format!(
                    "effort invalide : {v} — valeurs : {}",
                    vals.join(", ")
                ));
            }
        }
    }

    /// `/agent <texte>` : sous-agent (agents.md préfixé si chargé, sans historique).
    fn cmd_agent(&mut self, prompt: String) {
        let id = self.sessions.index();
        let Some(agent_id) = self.sessions.add_agent(id, &prompt) else {
            return;
        };
        self.push_system(format!("agent#{agent_id} lancé"));
        let model = self.effective_model();
        let effort = self.thinking.clone();
        let provider = Arc::clone(&self.provider);
        let tx = self.tx.clone();
        let mut messages = Vec::with_capacity(2);
        if let Some(content) = &self.agents_md {
            messages.push((String::from("system"), content.clone()));
        }
        messages.push(("user".to_string(), prompt));
        tokio::spawn(async move {
            let reply = provider
                .complete_full(&messages, &model, effort.as_deref())
                .await;
            let _ = tx.send(Incoming::Agent(id, agent_id, reply));
        });
    }

    /// `/agents` : liste les sous-agents du chat courant.
    fn cmd_agents(&mut self) {
        let sess = self.sessions.current();
        let text = if sess.agents.is_empty() {
            String::from("aucun agent")
        } else {
            sess.agents
                .iter()
                .map(|a| {
                    format!(
                        "#{} {} in={} out={} {}",
                        a.id,
                        a.status.label(),
                        a.prompt_tokens,
                        a.completion_tokens,
                        a.prompt
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        self.push_system(text);
    }

    /// `/compact` : résume l'historique (agents.md exclu du résumé méta,
    /// mais conservé en tête après compaction — gardé : pinné? + résumé + 4).
    fn cmd_compact(&mut self) {
        let id = self.sessions.index();
        let sess = self.sessions.current();
        let pinned = sess.pinned_system;
        let history = sess.history.clone();
        if history.len() < COMPACT_MIN {
            self.push_system(String::from("rien à compacter"));
            return;
        }
        let model = self.effective_model();
        let effort = self.thinking.clone();
        let provider = Arc::clone(&self.provider);
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
