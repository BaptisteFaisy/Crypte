//! État partagé back-core : sessions, bus, réglages, provider, almanach.
//!
//! `Core` est la valeur Tauri `State` (`Send + Sync`, intérieur sous
//! `Mutex`) ; `CoreInner` porte l'état mutable. Les réponses async du
//! provider arrivent en `Incoming` (canal borné `INCOMING_CAP`) et sont
//! routées par `route` ; chaque fait émet un `Event` vers le JSONL
//! (`EventLog`, KEEP) **et** vers le canal `crypte-event` (l'intégrateur
//! le relaie en `app.emit("crypte-event", …)`).
//!
//! Les commandes (`cmd_*`) sont des méthodes `Core` qui verrouillent,
//! mutent, puis droppent le verrou avant tout `await`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Result;
use tokio::sync::mpsc;

use crate::almanac::Almanac;
use crate::bus::Bus;
use crate::drain::{self, DrainOutcome};
use crate::events::{Event, EventLog};
use crate::history::window_estimate;
use crate::persist;
use crate::pool::{SpawnPool, INCOMING_CAP};
use crate::provider::{Completion, Provider};
use crate::sessions::{CloseOutcome, Sessions};
use crate::settings::Settings;

/// Message quand un envoi exige une connexion absente.
pub const NOT_CONNECTED: &str = "not connected: /connect <provider> or /settings";

/// Réponse async du provider, en attente de routage.
pub enum Incoming {
    /// Réponse chat (`idx` 0-based, époque du vol).
    Chat(usize, u64, Result<Completion>),
    /// Réponse agent (`owner`, `agent_id`, `deliver`).
    Agent(usize, usize, usize, Result<Completion>),
    /// Résumé de compaction (`idx` 0-based).
    Compact(usize, Result<Completion>),
}

/// Canaux rendus avec le `Core` : l'intégrateur boucle sur `incoming`
/// (`route`) et relaie `events` vers le frontend.
pub struct CoreChannels {
    pub incoming: mpsc::Receiver<Incoming>,
    pub events: mpsc::UnboundedReceiver<Event>,
}

/// État mutable (sous `Mutex` dans `Core`).
pub struct CoreInner {
    pub sessions: Sessions,
    pub bus: Bus,
    pub settings: Settings,
    pub provider: Option<Arc<Provider>>,
    pub provider_name: Option<String>,
    pub thinking: Option<String>,
    pub pool: SpawnPool,
    pub almanac: Almanac,
    /// Contexte agents.md chargé (posé par la tranche kiro ; `None` = absent).
    pub agents_md: Option<String>,
    /// Lecture agents.md tentée (une seule fois, comme le TUI).
    pub agents_tried: bool,
    pub tx: mpsc::Sender<Incoming>,
    pub chats_path: Option<PathBuf>,
}

impl CoreInner {
    /// Ajoute un message système au chat courant (perdu si aucun chat).
    pub fn push_system(&mut self, msg: String) {
        let idx = self.sessions.index();
        let Some(sess) = self.sessions.get_mut(idx) else {
            return; // dernier chat fermé : rien où afficher
        };
        sess.chat.push(crate::chat_state::Role::System, msg);
    }

    /// Modèle effectif : celui du chat, sinon le défaut provider, sinon `—`.
    pub fn effective_model_for(&self, idx: usize) -> String {
        if let Some(model) = self.sessions.get(idx).and_then(|c| c.model.as_deref()) {
            return model.to_string();
        }
        self.provider
            .as_ref()
            .map(|p| p.config().model.clone())
            .unwrap_or_else(|| String::from("—"))
    }

    /// Thinking affiché (`None` → "default").
    pub fn thinking_text(&self) -> String {
        self.thinking
            .clone()
            .unwrap_or_else(|| String::from("default"))
    }

    /// Limite effective du chat `idx` : fenêtre du modèle, plafonnée à 1M.
    pub fn context_limit_for(&self, idx: usize) -> u64 {
        crate::tokens::context_window(&self.effective_model_for(idx))
            .min(crate::tokens::CONTEXT_LIMIT)
    }

    /// Contexte estimé du chat `idx` : fenêtre d'envoi + saisie connue.
    pub fn context_used_for(&self, idx: usize) -> u64 {
        let Some(sess) = self.sessions.get(idx) else {
            return 0; // aucun chat : jauge à zéro
        };
        let input = sess.chat.input.trim();
        let pending = if input.is_empty() { None } else { Some(input) };
        window_estimate(&sess.history, sess.pinned_system, pending)
    }
}

/// État partagé (`tauri::State`), canaux d'émission hors verrou.
pub struct Core {
    log: EventLog,
    emit_tx: mpsc::UnboundedSender<Event>,
    inner: Mutex<CoreInner>,
}

impl Core {
    /// Construit l'état + ses canaux (`incoming` borné, `events` non borné).
    pub fn new(settings: Settings, almanac: Almanac, log: EventLog) -> (Self, CoreChannels) {
        let (tx, incoming) = mpsc::channel(INCOMING_CAP);
        let (emit_tx, events) = mpsc::unbounded_channel();
        let core = Self {
            log,
            emit_tx,
            inner: Mutex::new(CoreInner {
                sessions: Sessions::new(),
                bus: Bus::new(),
                settings,
                provider: None,
                provider_name: None,
                thinking: None,
                pool: SpawnPool::new(),
                almanac,
                agents_md: None,
                agents_tried: false,
                tx,
                chats_path: None,
            }),
        };
        (core, CoreChannels { incoming, events })
    }

    /// Verrouille l'état (empoisonnement ignoré : un panic ne condamne
    /// pas les commandes suivantes).
    pub fn lock(&self) -> MutexGuard<'_, CoreInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Émet un fait : append JSONL + canal `crypte-event` (envoi
    /// infaillible : le relais frontend ne doit jamais casser le backend).
    pub fn emit(&self, event: Event) {
        self.log.emit(&event);
        let _ = self.emit_tx.send(event);
    }

    /// Route une réponse async : almanach (usage facturé) + drain vers
    /// les chats. `ToolCalls` remonte à l'intégrateur (tranche kiro-tools).
    pub fn route(&self, incoming: Incoming) -> DrainOutcome {
        if let Some((prompt, completion)) = usage_of(&incoming) {
            self.lock().almanac.record(prompt, completion);
        }
        let emit = |event: Event| self.emit(event);
        let mut inner = self.lock();
        match incoming {
            Incoming::Chat(id, epoch, reply) => {
                drain::drain_chat(&mut inner, &emit, id, epoch, reply)
            }
            Incoming::Agent(owner, agent, deliver, reply) => {
                drain::drain_agent(&mut inner, &emit, owner, agent, deliver, reply);
                DrainOutcome::Done
            }
            Incoming::Compact(id, reply) => {
                drain::drain_compact(&mut inner, &emit, id, reply);
                DrainOutcome::Done
            }
        }
    }

    /// Nouveau chat + pin agents.md si chargé. Rend son index.
    pub fn add_chat(&self) -> usize {
        let mut inner = self.lock();
        inner.sessions.add();
        if let Some(content) = inner.agents_md.clone() {
            inner.sessions.current_mut().pin_system(&content);
        }
        let at = inner.sessions.index();
        let name = inner.sessions.current().name.clone();
        drop(inner);
        self.emit(Event::chat_created(at, &name));
        at
    }

    /// Ferme le chat `idx` : liens bus re-pointés, événement émis,
    /// refus motivé sinon (vol, inconnu).
    pub fn close_chat(&self, idx: usize) -> CloseOutcome {
        let mut inner = self.lock();
        let name = inner.sessions.get(idx).map(|c| c.name.clone());
        let outcome = inner.sessions.close(idx);
        if outcome != CloseOutcome::Closed {
            return outcome;
        }
        crate::sessions::shift_bus_links(&mut inner.bus, idx);
        drop(inner);
        if let Some(name) = name {
            self.emit(Event::chat_closed(idx, &name));
        }
        outcome
    }

    /// Pose le contexte agents.md (tranche kiro) + pinne les chats vides.
    pub fn set_agents_md(&self, content: Option<String>) {
        let mut inner = self.lock();
        let Some(text) = content else {
            inner.agents_md = None;
            return;
        };
        inner.agents_md = Some(text.clone());
        for i in 0..inner.sessions.count() {
            let empty = inner
                .sessions
                .get(i)
                .is_some_and(|c| !c.pinned_system && c.history.is_empty());
            if empty {
                inner.sessions.get_mut(i).unwrap().pin_system(&text);
            }
        }
    }

    /// Active la persistance : restaure les chats (silencieux), re-pinne
    /// agents.md quand chargé. Appelé par le binaire seul — jamais les tests.
    pub fn enable_history(&self) {
        if let Some(path) = persist::path() {
            self.enable_history_at(&path);
        }
    }

    /// Idem, à un chemin explicite (tests).
    pub fn enable_history_at(&self, path: &std::path::Path) {
        let mut inner = self.lock();
        match persist::load_from(path) {
            persist::Load::Missing => {}
            persist::Load::Corrupt => inner.push_system(String::from(
                "chats.json unreadable, kept as chats.json.corrupt — fresh start",
            )),
            persist::Load::Restored(sessions) => {
                inner.sessions = sessions;
                if let Some(md) = inner.agents_md.clone() {
                    persist::repin(&mut inner.sessions, &md);
                }
            }
        }
        inner.chats_path = Some(path.to_path_buf());
    }

    /// Autosave quand `tick` touche la marque ~5 s (silencieux, sans chemin = off).
    pub fn autosave_if_due(&self, tick: u64) {
        if tick.is_multiple_of(persist::AUTOSAVE_TICKS) {
            self.save_now();
        }
    }

    /// Sauvegarde immédiate (sortie) ; silencieuse, no-op sans chemin d'état.
    pub fn save_now(&self) {
        let inner = self.lock();
        if let Some(path) = &inner.chats_path {
            let _ = persist::save_to(path, &inner.sessions);
        }
    }

    /// Ligne système sur le chat `idx` (pont : sorties outils/specs).
    /// `history` = aussi versée à la fenêtre d'envoi (rôle `system`).
    pub fn push_system_to(&self, idx: usize, text: String, history: bool) {
        let mut inner = self.lock();
        let Some(sess) = inner.sessions.get_mut(idx) else {
            return;
        };
        sess.chat
            .push(crate::chat_state::Role::System, text.clone());
        if history {
            sess.push_history("system", text);
        }
    }

    /// Drapeau d'envoi du chat `idx` (pont : boucle outils).
    pub fn set_sending(&self, idx: usize, sending: bool) {
        if let Some(sess) = self.lock().sessions.get_mut(idx) {
            sess.sending = sending;
            sess.sending_since = sending.then(std::time::Instant::now);
        }
    }

    /// Fenêtre d'envoi du chat `idx` (pont : transcript boucle outils).
    pub fn send_window_of(&self, idx: usize) -> Vec<(String, String)> {
        let inner = self.lock();
        inner
            .sessions
            .get(idx)
            .map(|sess| crate::history::send_window(&sess.history, sess.pinned_system))
            .unwrap_or_default()
    }

    /// Tokens au ledger du chat `idx` (pont : boucle outils, specs, web).
    pub fn ledger_add(&self, idx: usize, prompt: u64, completion: u64, elapsed: f64) {
        if let Some(sess) = self.lock().sessions.get_mut(idx) {
            sess.ledger.record(prompt, completion, elapsed);
        }
    }

    /// Tokens à l'almanach (pont : réponses hors `Incoming`, ex. specs).
    pub fn almanac_record(&self, prompt: u64, completion: u64) {
        self.lock().almanac.record(prompt, completion);
    }

    /// Époque du vol en cours du chat `idx` (pont : réinjection).
    pub fn epoch_of(&self, idx: usize) -> u64 {
        self.lock().sessions.get(idx).map(|s| s.epoch).unwrap_or(0)
    }
}

/// Tokens (in, out) facturés par la réponse ; `None` si erreur.
fn usage_of(incoming: &Incoming) -> Option<(u64, u64)> {
    let done = match incoming {
        Incoming::Chat(_, _, r) | Incoming::Compact(_, r) => r.as_ref().ok()?,
        Incoming::Agent(_, _, _, r) => r.as_ref().ok()?,
    };
    Some((done.prompt_tokens, done.completion_tokens))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::provider::Completion;
    use crate::settings::Store;

    /// Core de test : coffre jetable, almanach mémoire, JSONL désactivé.
    pub(crate) fn test_core(name: &str) -> (Core, CoreChannels, PathBuf) {
        let dir = std::env::temp_dir().join(format!("kh-core-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let settings = Settings::with_store(Store::new(dir.clone()));
        let (core, channels) = Core::new(settings, Almanac::memory(), EventLog::disabled());
        (core, channels, dir)
    }

    /// Réponse provider factice (tokens 1/1).
    pub(crate) fn done(text: &str) -> Completion {
        Completion {
            tool_calls: Vec::new(),
            text: text.to_string(),
            prompt_tokens: 1,
            completion_tokens: 1,
        }
    }

    #[test]
    fn route_enregistre_l_almanach_et_emet() {
        let (core, mut channels, dir) = test_core("route");
        core.lock().sessions.get_mut(0).unwrap().sending = true;
        let outcome = core.route(Incoming::Chat(0, 0, Ok(done("boo"))));
        assert!(matches!(outcome, DrainOutcome::Done));
        assert_eq!(core.lock().almanac.all_time().total(), 2);
        let event = channels.events.try_recv().unwrap();
        let v = serde_json::to_value(&event).unwrap();
        assert_eq!(v["type"], "assistant_msg");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn add_chat_pinne_et_close_chat_emet() {
        let (core, mut channels, dir) = test_core("chats");
        core.set_agents_md(Some(String::from("ctx")));
        let at = core.add_chat();
        assert_eq!(at, 1);
        assert!(core.lock().sessions.get(1).unwrap().pinned_system);
        assert_eq!(core.close_chat(0), CloseOutcome::Closed);
        let mut types = Vec::new();
        while let Ok(ev) = channels.events.try_recv() {
            types.push(serde_json::to_value(&ev).unwrap()["type"].to_string());
        }
        assert!(types.contains(&"\"chat_created\"".to_string()), "{types:?}");
        assert!(types.contains(&"\"chat_closed\"".to_string()), "{types:?}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn enable_history_at_restaure_et_autosave_marques() {
        let (core, _channels, dir) = test_core("history");
        let path = dir.join("chats.json");
        core.lock()
            .sessions
            .current_mut()
            .push_history("user", String::from("x"));
        core.enable_history_at(&path);
        core.autosave_if_due(1);
        assert!(!path.exists());
        core.autosave_if_due(50);
        assert!(path.exists());
        std::fs::remove_dir_all(dir).ok();
    }
}
