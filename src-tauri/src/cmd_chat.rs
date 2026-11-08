//! Commandes chat §1 (`commands.md`) : liste,CRUD, envoi, stop, état.
//!
//! Méthodes `Core` (verrou interne, droppé avant tout `await`) ; les
//! adaptateurs `#[tauri::command]` vivent en `tauri_chat.rs`.
//! Insertion kiro-tools documentée sur `chat_send` (branche outils,
//! hooks `PromptSubmit`, critique Kiro — voir README).

use crate::chat_state::Role;
use crate::core::{Core, NOT_CONNECTED};
use crate::dto::{ChatStateDto, ChatSummary, SendAck};
use crate::events::Event;
use crate::history::{send_window, short_name, window_estimate};
use crate::sessions::CloseOutcome;
use crate::slash::is_command;
use crate::tokens::fmt_context;

impl Core {
    /// `chat_list` : résumés des chats ouverts, dans l'ordre.
    pub fn chat_list(&self) -> Vec<ChatSummary> {
        let inner = self.lock();
        inner
            .sessions
            .iter()
            .enumerate()
            .map(|(i, c)| ChatSummary::of(i, c))
            .collect()
    }

    /// `chat_new` : nouveau chat (courant, pinné si agents.md), événement émis.
    pub fn chat_new(&self) -> ChatSummary {
        let at = self.add_chat();
        let inner = self.lock();
        ChatSummary::of(at, inner.sessions.get(at).expect("chat créé ci-dessus"))
    }

    /// `chat_select` : rend le chat `index` courant + son état complet.
    pub fn chat_select(&self, index: usize) -> Result<ChatStateDto, String> {
        let mut inner = self.lock();
        let Some(chat) = inner.sessions.get(index) else {
            return Err(format!("chat {} not found", index + 1));
        };
        let state = ChatStateDto::of(index, chat);
        inner.sessions.select(index);
        Ok(state)
    }

    /// `chat_close` : ferme le chat `index` (liens re-pointés, événement
    /// émis) puis rend la liste ; refus motivé si vol ou inconnu.
    pub fn chat_close(&self, index: usize) -> Result<Vec<ChatSummary>, String> {
        match self.close_chat(index) {
            CloseOutcome::Closed => Ok(self.chat_list()),
            CloseOutcome::Busy => {
                let msg = String::from("cannot close: a reply is in flight");
                self.lock().push_system(msg.clone());
                Err(msg)
            }
            CloseOutcome::Unknown => {
                let msg = format!("chat {} not found", index + 1);
                self.lock().push_system(msg.clone());
                Err(msg)
            }
        }
    }

    /// `chat_clear` : vide la conversation du chat `index` (pinné gardé) ;
    /// refusé si une réponse est en cours (elle retomberait dedans).
    pub fn chat_clear(&self, index: usize) -> Result<ChatStateDto, String> {
        let mut inner = self.lock();
        let Some(sess) = inner.sessions.get_mut(index) else {
            return Err(format!("chat {} not found", index + 1));
        };
        if sess.sending {
            let msg = String::from("reply in flight — retry");
            sess.chat.push(Role::System, msg.clone());
            return Err(msg);
        }
        sess.clear();
        sess.chat
            .push(Role::System, String::from("conversation cleared"));
        Ok(ChatStateDto::of(index, sess))
    }

    /// `chat_send` : enfile le texte (chiffre → réponse à la question IA,
    /// nommage au premier message) et spawne la complétion ; ACK immédiat,
    /// résultat par `crypte-event`. Ne prend jamais les commandes slash.
    pub async fn chat_send(&self, index: usize, text: String) -> SendAck {
        self.chat_send_tooled(index, text, SendPrep::plain()).await
    }

    /// `chat_send` préparé par le pont : préfixes transitoires steering /
    /// powers (comme le TUI) + schémas d'outils quand `tools_on`.
    pub async fn chat_send_tooled(&self, index: usize, text: String, prep: SendPrep) -> SendAck {
        let text = text.trim().to_string();
        if text.is_empty() {
            return SendAck::refused("empty message");
        }
        if is_command(&text) {
            return SendAck::refused("slash commands are not chat text (use the UI actions)");
        }
        // Verrou : gardes + push + capture du plan d'envoi.
        let plan = {
            let mut inner = self.lock();
            let provider = match guard_chat_send(&mut inner, index, &text) {
                Ok(provider) => provider,
                Err(ack) => return ack,
            };
            let pushed = push_user_message(&mut inner, index, text);
            let plan = SendPlan::capture(&inner, index, provider, pushed, prep);
            drop(inner);
            self.emit(Event::user_msg(index, &plan.text));
            plan
        };
        plan.spawn();
        SendAck::accepted()
    }

    /// `chat_stop` : stoppe le vol du chat `index` (la réponse tardive
    /// sera ignorée) ; faux si rien n'était en vol ou chat inconnu.
    /// (La tranche kiro-tools annule aussi la boucle outil du chat.)
    pub fn chat_stop(&self, index: usize) -> bool {
        self.lock()
            .sessions
            .get_mut(index)
            .is_some_and(|sess| sess.stop_sending())
    }

    /// `chat_state` : état complet du chat `index` (refresh après event).
    pub fn chat_state(&self, index: usize) -> Result<ChatStateDto, String> {
        let inner = self.lock();
        inner
            .sessions
            .get(index)
            .map(|chat| ChatStateDto::of(index, chat))
            .ok_or_else(|| format!("chat {} not found", index + 1))
    }
}

/// Message poussé : fenêtre d'envoi + époque du vol + texte résolu.
struct Pushed {
    history: Vec<(String, String)>,
    epoch: u64,
    text: String,
}

/// Préparation d'envoi (calculée par le pont `bridge`, appliquée ici) :
/// blocs steering/powers transitoires + schémas d'outils éventuels.
pub struct SendPrep {
    pub steer: Vec<(String, String)>,
    pub powers: Vec<(String, String)>,
    pub defs: Option<Vec<serde_json::Value>>,
}

impl SendPrep {
    /// Envoi plein (tests, sans pont).
    pub(crate) fn plain() -> Self {
        Self {
            steer: Vec::new(),
            powers: Vec::new(),
            defs: None,
        }
    }
}

/// Plan d'envoi capturé sous verrou, spawné après (sans verrou).
struct SendPlan {
    pool: crate::pool::SpawnPool,
    provider: std::sync::Arc<crate::provider::Provider>,
    history: Vec<(String, String)>,
    tools: Option<Vec<serde_json::Value>>,
    model: String,
    effort: Option<String>,
    tx: tokio::sync::mpsc::Sender<crate::core::Incoming>,
    index: usize,
    epoch: u64,
    text: String,
}

impl SendPlan {
    /// Capture le plan (provider déjà validé, message déjà poussé).
    /// Les préfixes s'insèrent après le run système de tête (TUI).
    fn capture(
        inner: &crate::core::CoreInner,
        index: usize,
        provider: std::sync::Arc<crate::provider::Provider>,
        pushed: Pushed,
        prep: SendPrep,
    ) -> Self {
        let mut history = pushed.history;
        crate::kiro::steering::prefix_tagged(&mut history, "steering", &prep.steer);
        crate::kiro::steering::prefix_tagged(&mut history, "power", &prep.powers);
        Self {
            pool: inner.pool.clone(),
            provider,
            history,
            tools: prep.defs,
            model: inner.effective_model_for(index),
            effort: inner.thinking.clone(),
            tx: inner.tx.clone(),
            index,
            epoch: pushed.epoch,
            text: pushed.text,
        }
    }

    /// Spawne la complétion (réponse routée en `Incoming::Chat`) :
    /// outillée (guide + schémas) si des schémas sont fournis, pleine sinon.
    fn spawn(self) {
        let Self {
            pool,
            provider,
            history,
            tools,
            model,
            effort,
            tx,
            index,
            epoch,
            text: _,
        } = self;
        match tools {
            None => pool.spawn(report(tx, index, epoch, async move {
                provider
                    .complete_full(&history, &model, effort.as_deref())
                    .await
            })),
            Some(defs) => {
                let wire = crate::tools::flow::tooled_wire(history);
                let wire_json: Vec<serde_json::Value> = wire
                    .iter()
                    .map(|msg| serde_json::to_value(msg).unwrap_or(serde_json::Value::Null))
                    .collect();
                pool.spawn(report(tx, index, epoch, async move {
                    provider
                        .complete_json(&wire_json, &defs, &model, effort.as_deref())
                        .await
                }))
            }
        };
    }
}

/// Termine un vol : attend la réponse, la route en `Incoming::Chat`.
async fn report(
    tx: tokio::sync::mpsc::Sender<crate::core::Incoming>,
    index: usize,
    epoch: u64,
    reply: impl std::future::Future<Output = anyhow::Result<crate::provider::Completion>> + Send,
) {
    let reply = reply.await;
    let _ = tx
        .send(crate::core::Incoming::Chat(index, epoch, reply))
        .await;
}

/// Gardes d'envoi : chat présent, libre, contexte tient, connecté.
/// Rend le provider, ou l'ACK de refus (note poussée dans le chat).
fn guard_chat_send(
    inner: &mut crate::core::CoreInner,
    index: usize,
    text: &str,
) -> Result<std::sync::Arc<crate::provider::Provider>, SendAck> {
    if inner.sessions.count() == 0 {
        return Err(SendAck::refused("no chat"));
    }
    let Some(sess) = inner.sessions.get(index) else {
        return Err(SendAck::refused(format!("chat {} not found", index + 1)));
    };
    if sess.sending {
        return Err(SendAck::refused("reply in flight"));
    }
    let limit = inner.context_limit_for(index);
    let used = window_estimate(&sess.history, sess.pinned_system, Some(text));
    if used > limit {
        let msg = format!(
            "context full ({}), send refused: /compact to continue",
            fmt_context(used, limit)
        );
        inner
            .sessions
            .get_mut(index)
            .unwrap()
            .chat
            .push(Role::System, msg.clone());
        return Err(SendAck::refused(msg));
    }
    inner.provider.clone().ok_or_else(|| {
        inner
            .sessions
            .get_mut(index)
            .unwrap()
            .chat
            .push(Role::System, String::from(NOT_CONNECTED));
        SendAck::refused(NOT_CONNECTED)
    })
}

/// Pousse le message user : chiffre → réponse à la question IA (les deux
/// effacent l'attente), nommage au premier message, flags de vol posés.
fn push_user_message(inner: &mut crate::core::CoreInner, index: usize, text: String) -> Pushed {
    let pending = inner.sessions.get_mut(index).unwrap().pending.take();
    let text = pending
        .as_ref()
        .and_then(|q| crate::question::resolve_answer(q, &text))
        .unwrap_or(text);
    let sess = inner.sessions.get_mut(index).unwrap();
    sess.chat.take_input();
    if !sess.history.iter().any(|(role, _)| role == "user") {
        sess.name = short_name(&text);
    }
    sess.chat.push(Role::User, text.clone());
    sess.push_history("user", text.clone());
    let history = send_window(&sess.history, sess.pinned_system);
    sess.sending = true;
    sess.auto_hops = 0;
    sess.sending_since = Some(std::time::Instant::now());
    let epoch = sess.epoch;
    Pushed {
        history,
        epoch,
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tests::test_core;
    use crate::provider::ProviderConfig;

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

    #[test]
    fn select_close_clear_et_etats() {
        let (core, _channels, dir) = test_core("crud");
        let created = core.chat_new();
        assert_eq!(created.index, 1);
        assert_eq!(core.chat_list().len(), 2);
        assert!(core.chat_select(9).is_err());
        assert_eq!(core.chat_state(1).unwrap().name, "Chat 2");
        let cleared = core.chat_clear(1).unwrap();
        assert_eq!(cleared.messages.len(), 1);
        assert_eq!(cleared.messages[0].text, "conversation cleared");
        core.lock().sessions.get_mut(1).unwrap().sending = true;
        assert!(core.chat_clear(1).is_err());
        assert!(core.chat_close(9).is_err());
        assert!(core.chat_close(1).is_err(), "vol en cours : refusé");
        core.lock().sessions.get_mut(1).unwrap().sending = false;
        assert_eq!(core.chat_close(1).unwrap().len(), 1);
        assert!(!core.chat_stop(0));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn send_valide_pousse_et_spawne() {
        let (core, mut channels, dir) = wired("send");
        let ack = core.chat_send(0, "  bonsoir  ".to_string()).await;
        assert!(ack.accepted);
        {
            let inner = core.lock();
            let sess = inner.sessions.get(0).unwrap();
            assert!(sess.sending);
            assert_eq!(sess.name, "bonsoir");
            assert_eq!(sess.history.last().unwrap().0, "user");
        }
        let event = channels.events.try_recv().unwrap();
        let v = serde_json::to_value(&event).unwrap();
        assert_eq!(v["type"], "user_msg");
        assert!(core.chat_stop(0));
        assert!(!core.lock().sessions.get(0).unwrap().sending);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn send_refuse_vide_slash_inconnu_occupe_deconnecte() {
        let (core, _channels, dir) = wired("refus");
        assert!(!core.chat_send(0, "   ".to_string()).await.accepted);
        let slash = core.chat_send(0, "/help".to_string()).await;
        assert!(!slash.accepted);
        assert!(slash.reason.unwrap().contains("slash"));
        assert!(!core.chat_send(9, "x".to_string()).await.accepted);
        core.lock().sessions.get_mut(0).unwrap().sending = true;
        let busy = core.chat_send(0, "x".to_string()).await;
        assert_eq!(busy.reason.as_deref(), Some("reply in flight"));
        core.lock().sessions.get_mut(0).unwrap().sending = false;
        core.lock().provider = None;
        let down = core.chat_send(0, "x".to_string()).await;
        assert_eq!(down.reason.as_deref(), Some(NOT_CONNECTED));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn send_chiffre_repond_a_la_question() {
        let (core, _channels, dir) = wired("digit");
        {
            let mut inner = core.lock();
            let sess = inner.sessions.get_mut(0).unwrap();
            sess.track_question("```question\nOn continue ?\n1. Oui\n2. Non\n```");
            assert!(sess.pending.is_some());
        }
        let ack = core.chat_send(0, "2".to_string()).await;
        assert!(ack.accepted);
        let inner = core.lock();
        let sess = inner.sessions.get(0).unwrap();
        assert!(sess.pending.is_none());
        assert_eq!(
            sess.history.last(),
            Some(&(String::from("user"), String::from("Non")))
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
