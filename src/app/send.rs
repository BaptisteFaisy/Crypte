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
        if self.sessions.count() == 0 {
            return; // aucun chat : [+] / Ctrl+N pour recommencer
        }
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
        let text = self.hook_prefixed_input(&text);
        self.hooks_fire(crate::kiro::hooks::Trigger::PromptSubmit, &text, id);
        if let Some(used) = self.context_over_limit(&text) {
            self.push_system(format!(
                "context full ({}), send refused: /compact to continue",
                crate::tokens::fmt_context(used, self.context_limit())
            ));
            return;
        }
        // Pending AI question: a digit picks the option, any other
        // text answers freely (both clear the question).
        let pending = self.sessions.current_mut().pending.take();
        let text = pending
            .as_ref()
            .and_then(|q| crate::question::resolve_answer(q, &text))
            .unwrap_or(text);
        let judged = text.clone();
        let auto = self.auto_steer_blocks(&text);
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
        let mut history = send_window(&sess.history, sess.pinned_system);
        sess.sending = true;
        sess.auto_hops = 0;
        sess.sending_since = Some(Instant::now());
        let epoch = sess.epoch;
        if !auto.is_empty() {
            crate::kiro::steering::prefix_auto(&mut history, &auto);
            let names = auto
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            self.push_system(format!("steering auto: {names}"));
        }
        if self.tools_on {
            let wire = crate::tools::flow::tooled_wire(history);
            let mcp = self.mcp.clone();
            self.pool.spawn(async move {
                let tools = mcp.tooled_defs().await;
                let reply = provider
                    .complete_with_tools(&wire, &tools, &model, effort.as_deref())
                    .await;
                let _ = tx.send(Incoming::Chat(id, epoch, reply)).await;
            });
        } else {
            self.pool.spawn(async move {
                let reply = provider
                    .complete_full(&history, &model, effort.as_deref())
                    .await;
                let _ = tx.send(Incoming::Chat(id, epoch, reply)).await;
            });
        }
        // Kiro juge le prompt en parallèle (même modèle, bulle), après
        // l'appel du chat : le permis du pool va d'abord à la réponse.
        self.critique(id, &judged);
    }

    /// Blocs steering `auto` matchant la demande (rechargés à chaque
    /// envoi, éphémères : jamais pinnés, jamais l'historique stocké).
    fn auto_steer_blocks(&self, prompt: &str) -> Vec<(String, String)> {
        let files = crate::kiro::steering::load_all_in(
            &self.t5.kiro_project,
            self.t5.kiro_global.as_deref(),
        );
        crate::kiro::steering::auto_for(&files, prompt)
            .into_iter()
            .map(|file| (file.name.clone(), file.body.clone()))
            .collect()
    }

    /// Ctrl+C : stoppe le vol du chat courant (la réponse tardive sera
    /// ignorée, voir `epoch`) ; la boucle outil du chat est annulée
    /// aussi (livraisons orphelines ignorées). Sans effet si rien
    /// n'est en vol, ne quitte jamais (`q` quitte).
    pub(super) fn stop_chat(&mut self) {
        let id = self.sessions.index();
        let stopped = self
            .sessions
            .get_mut(id)
            .is_some_and(|sess| sess.stop_sending());
        if stopped && self.tool_run.as_ref().is_some_and(|run| run.chat == id) {
            self.tool_run = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Completion, ProviderConfig};

    /// App de test (clé factice, aucun appel réseau attendu).
    fn test_app() -> App {
        App::from_config(ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        })
        .unwrap()
    }

    /// Aller-retour question IA : le drain mémorise + rend, `send("2")`
    /// envoie le libellé et efface l'attente (brut gardé pour le LLM).
    #[tokio::test]
    async fn question_drain_then_digit_send_round_trip() {
        let mut app = test_app();
        // Config vide injectée : hermétique au .kiro/ réel du dépôt (pas de spawn MCP).
        let vide = std::env::temp_dir().join(format!("kh-send-{}", std::process::id()));
        std::fs::create_dir_all(&vide).unwrap();
        app.t5.kiro_project = vide.clone();
        app.t5.kiro_global = None;
        app.mcp = crate::kiro::mcp_pool::McpState::for_config(crate::kiro::mcp::Config::default());
        let raw = "look:\n```question\nShall we go on?\n1. Yes\n2. No thanks\n```";
        app.drain_chat(
            0,
            0,
            Ok(Completion {
                text: raw.to_string(),
                tool_calls: Vec::new(),
                prompt_tokens: 1,
                completion_tokens: 2,
            }),
        );
        assert!(app.sessions.current().pending.is_some(), "asked");
        let shown = app
            .sessions
            .current()
            .chat
            .messages()
            .last()
            .unwrap()
            .text
            .clone();
        assert!(shown.contains("the crypt asks you"), "rendered: {shown:?}");
        assert!(!shown.contains("```"), "block replaced: {shown:?}");
        assert_eq!(app.sessions.current().history.last().unwrap().1, raw);
        app.sessions.current_mut().chat.input.push('2');
        app.send();
        let sess = app.sessions.current();
        assert!(sess.pending.is_none(), "answering clears");
        assert_eq!(
            sess.history.last(),
            Some(&(String::from("user"), String::from("No thanks"))),
            "digit sends the label"
        );
        assert!(sess.sending, "send goes out normally");
        std::fs::remove_dir_all(&vide).ok();
    }

    /// Dossier `.kiro/steering` : un fichier `auto` (règles Rust).
    fn steer_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("kh-send-steer-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kiro/steering")).unwrap();
        std::fs::write(
            dir.join(".kiro/steering/r.md"),
            "---\ninclusion: auto\ndescription: Règles du borrow-checker Rust.\n---\nCorps.",
        )
        .unwrap();
        dir
    }

    /// Blocs `auto` : matchés par la demande, vides sinon.
    #[test]
    fn auto_steer_blocks_matche_ou_vide() {
        let dir = steer_dir("blocs");
        let mut app = test_app();
        app.t5.kiro_project = dir.clone();
        app.t5.kiro_global = None;
        let matched = app.auto_steer_blocks("explique le borrow checker");
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].0, "r");
        assert!(app.auto_steer_blocks("bonjour").is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Envoi : la demande qui matche annonce `steering auto: r`.
    #[tokio::test]
    async fn send_annonce_steering_auto_matche() {
        let mut app = test_app();
        let dir = steer_dir("annonce");
        app.t5.kiro_project = dir.clone();
        app.t5.kiro_global = None;
        app.mcp = crate::kiro::mcp_pool::McpState::for_config(crate::kiro::mcp::Config::default());
        app.sessions
            .current_mut()
            .chat
            .input
            .push_str("explique le borrow checker");
        app.send();
        let texts: Vec<String> = app
            .sessions
            .current()
            .chat
            .messages()
            .iter()
            .map(|msg| msg.text.clone())
            .collect();
        assert!(
            texts.iter().any(|t| t.contains("steering auto: r")),
            "{texts:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
