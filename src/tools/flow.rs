//! Boucle agent : appels d'outils, confirmations (y/n), relances LLM.
//!
//! Un envoi avec outils (`/tools on`) boucle : réponse → appels (file
//! séquentielle) → confirmations des modifications en mode `auto` →
//! exécution ([`super::exec`]) → résultats rejoués → nouvel appel, jusqu'à
//! la réponse finale (aucune limite de tours). La boucle vit dans [`App`]
//! (jamais persistée) ; l'historique provider ne reçoit que le texte final.

use std::collections::VecDeque;

use crossterm::event::{KeyCode, KeyEvent};

use super::exec::{self, Outcome};
use super::wire::{ToolCall, WireMsg};
use super::{defs, GUIDE};
use crate::app::{App, Incoming};
use crate::chat::Role;
use crate::history::send_window;
use crate::provider::truncate;

/// Boucle agent en cours (un seul envoi outillé à la fois).
pub struct ToolRun {
    /// Chat d'origine (l'affichage et le ledger y restent).
    pub chat: usize,
    /// Transcript filaire (historique + guide + appels + résultats).
    pub wire: Vec<WireMsg>,
    /// Appels décidés, exécutés dans l'ordre.
    pub queue: VecDeque<ToolCall>,
    /// Confirmation y/n attendue (touches capturées, voir `on_tool_key`).
    pub confirming: bool,
}

/// Livraison d'exécution (appel + résultat, affichée puis rejouée).
pub struct Delivery {
    pub call: ToolCall,
    pub ok: bool,
    pub text: String,
    pub ms: u64,
}

impl Delivery {
    /// Livraison depuis un [`Outcome`] d'exécution.
    fn from_outcome(call: ToolCall, out: Outcome, ms: u64) -> Self {
        Self {
            call,
            ok: out.ok,
            text: out.text,
            ms,
        }
    }
}

impl App {
    /// `/tools [on|off]` : affiche ou change l'envoi outillé (session seule,
    /// défaut on) ; une boucle en cours n'est pas interrompue.
    pub(crate) fn cmd_tools(&mut self, arg: Option<String>) {
        match arg.as_deref() {
            None => self.push_system(tools_line(self.tools_on)),
            Some("on") => {
                self.tools_on = true;
                self.push_system(tools_line(true));
            }
            Some("off") => {
                self.tools_on = false;
                self.push_system(tools_line(false));
            }
            Some(val) => self.push_system(format!("invalid: {val} — values: on, off")),
        }
    }

    /// Touches pendant une confirmation : y = exécute, n/Échap = refuse
    /// (le reste est ignoré, jamais de quit — comme le flow `/connect`).
    pub(crate) fn on_tool_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('y' | 'Y') => self.tool_resolve(true),
            KeyCode::Char('n' | 'N') | KeyCode::Esc => self.tool_resolve(false),
            _ => true,
        }
    }

    /// Réponse avec appels : affiche le texte (hors historique), prépare la
    /// boucle (transcript + file) et avance (confirme ou exécute).
    /// Le ledger du tour est déjà compté par `drain_chat`.
    pub(crate) fn drain_tool_calls(&mut self, id: usize, done: crate::provider::Completion) {
        let Some(sess) = self.sessions.get_mut(id) else {
            self.tool_run = None;
            return;
        };
        sess.sending = true;
        sess.sending_since = Some(std::time::Instant::now());
        if !done.text.trim().is_empty() {
            sess.chat.push(Role::Assistant, done.text.clone());
        }
        if self.tool_run.is_none() {
            self.tool_run = Some(self.new_run(id));
        }
        let turn = WireMsg::assistant_calls(&done.text, &done.tool_calls);
        if let Some(run) = self.tool_run.as_mut() {
            run.wire.push(turn);
            run.queue.extend(done.tool_calls);
        }
        self.tool_advance();
    }

    /// Exécution(s) terminée(s) : affiche, rejoue au transcript, avance.
    /// Boucle absente ou autre chat : livraison périmée, ignorée.
    pub(crate) fn drain_tool_done(&mut self, id: usize, deliveries: Vec<Delivery>) {
        let current = self.tool_run.as_ref().map(|run| run.chat);
        if current != Some(id) {
            return;
        }
        for delivery in &deliveries {
            self.hooks_fire(
                crate::kiro::hooks::Trigger::PostToolUse,
                &delivery.call.name,
                id,
            );
        }
        self.tool_delivered(deliveries);
    }

    /// Transcript initial : fenêtre d'envoi + guide outils (transitoire).
    fn new_run(&self, id: usize) -> ToolRun {
        let mut wire: Vec<WireMsg> = self
            .sessions
            .get(id)
            .map(|sess| send_window(&sess.history, sess.pinned_system))
            .unwrap_or_default()
            .into_iter()
            .map(|(role, content)| WireMsg::text(role, content))
            .collect();
        wire.push(WireMsg::text(String::from("system"), String::from(GUIDE)));
        ToolRun {
            chat: id,
            wire,
            queue: VecDeque::new(),
            confirming: false,
        }
    }

    /// Avance la boucle : prochain appel (confirme ou exécute) ou relance LLM.
    fn tool_advance(&mut self) {
        let next = self
            .tool_run
            .as_ref()
            .and_then(|run| run.queue.front().cloned());
        let Some(call) = next else {
            self.tool_spawn_llm();
            return;
        };
        self.tool_gate(call);
    }

    /// Demande confirmation : saute au chat, affiche l'appel, prompt y/n.
    pub(crate) fn tool_ask(&mut self, call: &ToolCall) {
        let chat = self.tool_run.as_ref().map(|run| run.chat).unwrap_or(0);
        self.sessions.select(chat);
        if let Some(run) = self.tool_run.as_mut() {
            run.confirming = true;
        }
        if let Some(sess) = self.sessions.get_mut(chat) {
            sess.chat
                .push(Role::System, format!("allow {}? (y/n)", call_line(call)));
            sess.chat
                .set_prompt(Some(String::from("allow? (y/n)")), false);
            sess.chat.set_input_preview(Some(call_line(call)));
        }
    }

    /// Résolution y/n : exécute ou note le refus, puis avance.
    fn tool_resolve(&mut self, allow: bool) -> bool {
        let popped = self.tool_run.as_mut().and_then(|run| {
            run.confirming = false;
            run.queue.pop_front().map(|call| (run.chat, call))
        });
        let Some((chat, call)) = popped else {
            return true;
        };
        self.restore_tool_prompt(chat);
        if allow {
            self.tool_spawn_exec(chat, call);
        } else {
            self.push_tool_line(chat, format!("denied {}", call_line(&call)));
            let denied = Delivery {
                call: call.clone(),
                ok: false,
                text: String::from("denied by user"),
                ms: 0,
            };
            self.tool_delivered(vec![denied]);
        }
        true
    }

    /// Relance LLM avec le transcript outillé (tour suivant, sans limite).
    /// Sans provider (impossible en pratique) : boucle avortée proprement.
    fn tool_spawn_llm(&mut self) {
        let Some(provider) = self.provider.clone() else {
            self.tool_abort("no provider — loop aborted");
            return;
        };
        let (chat, wire) = match self.tool_run.as_ref() {
            Some(run) => (run.chat, run.wire.clone()),
            None => return,
        };
        let model = self.effective_model_for(chat);
        let effort = self.thinking.clone();
        let epoch = self.sessions.get(chat).map(|s| s.epoch).unwrap_or(0);
        let tx = self.tx.clone();
        let mcp = self.mcp.clone();
        self.pool.spawn(async move {
            let tools = mcp.tooled_defs().await;
            let reply = provider
                .complete_with_tools(&wire, &tools, &model, effort.as_deref())
                .await;
            let _ = tx.send(Incoming::Chat(chat, epoch, reply)).await;
        });
    }

    /// Exécute un appel (ligne `running`, tâche async, livraison `ToolDone`).
    /// Hors pool LLM : un exécutable local (jusqu'à 90 s) ne doit pas
    /// occuper un permis réseau ; le canal borné garde la backpressure.
    pub(crate) fn tool_spawn_exec(&mut self, chat: usize, call: ToolCall) {
        self.hooks_fire(crate::kiro::hooks::Trigger::PreToolUse, &call.name, chat);
        self.space_tool_events(&call);
        self.push_tool_line(chat, format!("[tool {}]", call_line(&call)));
        let tx = self.tx.clone();
        let mcp = self.mcp.clone();
        tokio::spawn(async move {
            let started = std::time::Instant::now();
            let out = if crate::kiro::mcp_client::is_mcp_name(&call.name) {
                mcp.call(chat, &call.name, &call.args).await
            } else {
                exec::run_tool(chat, &call.name, &call.args).await
            };
            let ms = started.elapsed().as_millis().min(9_999_999) as u64;
            let delivery = Delivery::from_outcome(call, out, ms);
            let _ = tx.send(Incoming::ToolDone(chat, vec![delivery])).await;
        });
    }

    /// Livraisons : affiche chaque résultat, rejoue au transcript, avance.
    pub(crate) fn tool_delivered(&mut self, deliveries: Vec<Delivery>) {
        let mut failed = false;
        let mut lines = Vec::with_capacity(deliveries.len());
        let mut results = Vec::with_capacity(deliveries.len());
        for delivery in &deliveries {
            failed |= !delivery.ok;
            lines.push(done_line(delivery));
            results.push(WireMsg::result(
                delivery.call.id.clone(),
                delivery.text.clone(),
            ));
        }
        if let Some(run) = self.tool_run.as_mut() {
            let chat = run.chat;
            run.wire.extend(results);
            if let Some(sess) = self.sessions.get_mut(chat) {
                for line in lines {
                    sess.chat.push(Role::System, line);
                }
            }
        }
        if failed {
            self.apparition.omen(crate::apparition::Omen::Error);
        }
        self.tool_advance();
    }

    /// Avorte la boucle (chat libéré + note système).
    fn tool_abort(&mut self, note: &str) {
        let chat = self.tool_run.as_ref().map(|run| run.chat);
        self.tool_run = None;
        if let Some(chat) = chat {
            if let Some(sess) = self.sessions.get_mut(chat) {
                sess.sending = false;
                sess.chat.push(Role::System, String::from(note));
            }
        }
    }

    /// Ligne système sur le chat `chat` (pas forcément le courant).
    fn push_tool_line(&mut self, chat: usize, line: String) {
        if let Some(sess) = self.sessions.get_mut(chat) {
            sess.chat.push(Role::System, line);
        }
    }

    /// Restaure le prompt de saisie après une confirmation.
    fn restore_tool_prompt(&mut self, chat: usize) {
        if let Some(sess) = self.sessions.get_mut(chat) {
            sess.chat.set_prompt(None, false);
            sess.chat.set_input_preview(None);
        }
    }
}

/// Fil initial outillé : historique + guide transitoire (non persisté).
pub fn tooled_wire(history: Vec<(String, String)>) -> Vec<WireMsg> {
    let mut wire: Vec<WireMsg> = history
        .into_iter()
        .map(|(role, content)| WireMsg::text(role, content))
        .collect();
    wire.push(WireMsg::text(String::from("system"), String::from(GUIDE)));
    wire
}

/// Une ligne d'appel : `nom {args…}` (args tronqués à 120 caractères).
fn call_line(call: &ToolCall) -> String {
    format!("{} {}", call.name, truncate(&call.args, 120))
}

/// Ligne de résultat : `nom → ok/FAILED, N chars, N ms` (+ extrait si échec).
fn done_line(delivery: &Delivery) -> String {
    let status = if delivery.ok { "ok" } else { "FAILED" };
    let mut line = format!(
        "[tool {} → {status}, {} chars, {} ms]",
        delivery.call.name,
        delivery.text.chars().count(),
        delivery.ms
    );
    if !delivery.ok {
        line.push_str(&format!(" {}", truncate(&delivery.text, 200)));
    }
    line
}

/// Ligne d'état `/tools` : on/off + nombre d'outils et familles.
fn tools_line(on: bool) -> String {
    if on {
        format!(
            "tools: on ({} tools: browser, desktop, shell)",
            defs().len()
        )
    } else {
        String::from("tools: off")
    }
}

#[cfg(test)]
mod tests {
    use super::super::{class, needs_confirm, Class};
    use super::*;
    use crate::permissions::Mode;

    #[test]
    fn lignes_appel_resultat_etat() {
        let text: String = "x".repeat(200);
        let call = ToolCall {
            id: String::from("c1"),
            name: String::from("shell_run"),
            args: text,
        };
        assert!(call_line(&call).len() < 140);
        let done = Delivery {
            call: call.clone(),
            ok: false,
            text: String::from("boom"),
            ms: 12,
        };
        let line = done_line(&done);
        assert!(line.contains("FAILED") && line.contains("boom"));
        assert!(tools_line(true).contains("tools: on (14 tools"));
        assert_eq!(tools_line(false), "tools: off");
    }

    /// App de test (provider fictif : `localhost:9` refuse tout).
    fn test_app() -> App {
        let cfg = crate::provider::ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        App::from_config(cfg).unwrap()
    }

    /// Texte du dernier message affiché du chat `idx`.
    fn last_text(app: &App, idx: usize) -> String {
        app.sessions
            .get(idx)
            .unwrap()
            .chat
            .messages()
            .last()
            .map(|m| m.text.clone())
            .unwrap_or_default()
    }

    /// Prochain `ToolDone` (scrute 2 s max, panique si timeout).
    async fn recv_done(app: &mut App) -> Vec<Delivery> {
        for _ in 0..100 {
            if let Ok(Incoming::ToolDone(id, got)) = app.rx.try_recv() {
                assert_eq!(id, 0);
                return got;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("ToolDone arrivé");
    }

    /// Prochaine réponse `Chat` en erreur (refus `localhost:9`).
    async fn recv_chat_err(app: &mut App) {
        for _ in 0..100 {
            if let Ok(Incoming::Chat(id, epoch, reply)) = app.rx.try_recv() {
                assert_eq!(id, 0);
                assert!(reply.is_err());
                app.drain_chat(id, epoch, reply);
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("Chat arrivé");
    }

    #[tokio::test]
    async fn boucle_shell_execute_livre_et_libere() {
        let mut app = test_app();
        // Config vide injectée : hermétique au .kiro/ réel du dépôt (pas de spawn MCP).
        let vide = std::env::temp_dir().join(format!("kh-flow-{}", std::process::id()));
        std::fs::create_dir_all(&vide).unwrap();
        app.t5.kiro_project = vide.clone();
        app.t5.kiro_global = None;
        app.mcp = crate::kiro::mcp_pool::McpState::for_config(crate::kiro::mcp::Config::default());
        app.permissions = Mode::Yolo;
        app.sessions
            .current_mut()
            .history
            .push((String::from("user"), String::from("hi")));
        let done = crate::provider::Completion {
            text: String::new(),
            tool_calls: vec![ToolCall {
                id: String::from("c1"),
                name: String::from("shell_run"),
                args: String::from(r#"{"cmd":"echo hi"}"#),
            }],
            prompt_tokens: 1,
            completion_tokens: 1,
        };
        app.drain_tool_calls(0, done);
        assert!(app.sessions.get(0).unwrap().sending);
        let deliveries = recv_done(&mut app).await;
        assert_eq!(deliveries.len(), 1);
        assert!(deliveries[0].ok);
        assert_eq!(deliveries[0].text, "hi\n");
        app.drain_tool_done(0, deliveries);
        assert!(last_text(&app, 0).contains("[tool shell_run → ok"));
        recv_chat_err(&mut app).await;
        assert!(app.tool_run.is_none());
        assert!(!app.sessions.get(0).unwrap().sending);
        assert!(last_text(&app, 0).starts_with("error:"));
        std::fs::remove_dir_all(&vide).ok();
    }

    #[test]
    fn confirmations_lectures_libres_modifs_en_auto() {
        assert_eq!(class("browser_snapshot"), Class::Read);
        assert_eq!(class("desktop_shot"), Class::Read);
        assert_eq!(class("browser_click"), Class::Change);
        assert_eq!(class("shell_run"), Class::Change);
        assert_eq!(class("nope"), Class::Change);
        assert!(!needs_confirm(Mode::Auto, "browser_read"));
        assert!(needs_confirm(Mode::Auto, "shell_run"));
        assert!(!needs_confirm(Mode::Yolo, "shell_run"));
    }
}
