//! État BACK-TOOLS : tout ce que les commandes §4–§10 manipulent.
//!
//! [`ToolsBackend`] est géré par Tauri (`app.manage`, à côté de l'état
//! core) ; les commandes de cette tranche ne prennent que lui. Les
//! ponts vers la tranche core (`note_*`, `take_*`, `BackendOut`) sont
//! documentés un par un — l'intégrateur les câble en quelques lignes
//! (voir `README.md`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crate::almanac::Almanac;
use crate::apparition::{Apparition, Omen};
use crate::events_log::EventLog;
use crate::fx::Fx;
use crate::haunt::Haunt;
use crate::hub::{Hub, SyncView};
use crate::kiro::hook_run::{HookOut, HooksEngine};
use crate::kiro::mcp_pool::McpState;
use crate::permissions::{self, Mode, Rules};
use crate::phantom::Phantom;
use crate::provider::Llm;
use crate::spec_runner::SpecRunner;

pub use crate::outbox::BackendOut;
use crate::tools::flow::ToolFlow;
use crate::vps::VpsRuntime;

/// Tombée d'une critique (canal interne, exposée pour les tests).
pub struct CritiqueDone {
    pub text: Option<String>,
    pub prompt: u64,
    pub completion: u64,
}

/// État géré des commandes BACK-TOOLS (`tauri::State`).
pub struct ToolsBackend {
    /// Envoi outillé (`/tools on`, défaut on comme le TUI).
    pub tools_on: bool,
    /// Boucle outils en cours (une seule à la fois).
    pub flow: ToolFlow,
    /// Mode de permissions + règles fines.
    pub permissions: Mode,
    pub rules: Rules,
    /// Fichiers de persistance (`None` si `HOME` manquant).
    pub permissions_path: Option<PathBuf>,
    pub rules_path: Option<PathBuf>,
    /// Chemins Kiro résolus (projet + global + checkpoints).
    pub kiro_project: PathBuf,
    pub kiro_global: Option<PathBuf>,
    pub checkpoint_dir: Option<PathBuf>,
    /// Moteur hooks + parc MCP + runner specs.
    pub hooks: HooksEngine,
    pub mcp: McpState,
    pub specs: SpecRunner,
    /// Corps de skill armé (préfixé au prochain envoi, transitoire).
    pub skill_prefix: Option<String>,
    /// Connexion vue du core (gate `/spec run`, comme le TUI).
    pub connected: bool,
    /// LLM du core (synthèse `/web` seule ; `None` = résultats bruts).
    pub llm: Option<Arc<dyn Llm>>,
    /// Modèle + effort effectifs vus du core (synthèse `/web`).
    pub llm_model: String,
    pub llm_effort: Option<String>,
    /// Almanach des tokens (fichier défaut, comme `main.rs`).
    pub almanac: Almanac,
    /// Profils + statuts VPS.
    pub vps: VpsRuntime,
    /// Kiro en balade, apparitions, effets, hub, jeu.
    pub phantom: Phantom,
    pub apparition: Apparition,
    pub fx: Fx,
    /// Boutique (portefeuille de session + message, `shop.rs`).
    pub shop: crate::shop::Shop,
    pub hub: Hub,
    /// Dernière vue sessions connue (câblée par `note_sessions`).
    pub hub_view: SyncView,
    pub haunt: Haunt,
    /// Jeu ouvert (frames `haunt_frame` seulement si vrai).
    pub haunt_open: bool,
    /// Activité du chat courant (câblée par `note_chat_activity`).
    pub chat_sending: bool,
    pub chat_agents: usize,
    /// Index du chat courant (0-based, câblé par `note_current_chat` :
    /// les annonces sans chat — skill, remember, bestiaire — y vont).
    pub current_chat: usize,
    /// Dernier présage (`error`/`compact`, 6 s comme le TUI).
    pub last_omen: Option<(String, Instant)>,
    /// Journal JSONL (`--events-log` / `KH_EVENTS_LOG`).
    pub events: EventLog,
    /// File des sorties vers la tranche core (`take_outbox`).
    pub outbox: Vec<BackendOut>,
    critique_tx: tokio::sync::mpsc::UnboundedSender<CritiqueDone>,
    critique_rx: tokio::sync::mpsc::UnboundedReceiver<CritiqueDone>,
}

impl ToolsBackend {
    /// Backend prod (chemins réels, comme le TUI au boot).
    pub fn new() -> Self {
        let kiro_project = crate::kiro::project_dir();
        let kiro_global = crate::kiro::global_dir();
        let checkpoint_dir = crate::kiro::checkpoint::dir();
        let mut specs = SpecRunner::new(kiro_project.clone(), checkpoint_dir.clone());
        let permissions = permissions::load();
        specs.set_mode(permissions);
        let (critique_tx, critique_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            tools_on: true,
            flow: ToolFlow::new(),
            permissions,
            rules: permissions::rules_path()
                .map(|p| permissions::load_rules_from(&p))
                .unwrap_or_default(),
            permissions_path: permissions::path(),
            rules_path: permissions::rules_path(),
            hooks: HooksEngine::new(kiro_project.clone(), kiro_global.clone()),
            mcp: McpState::new(),
            specs,
            skill_prefix: None,
            connected: false,
            llm: None,
            llm_model: String::new(),
            llm_effort: None,
            almanac: Almanac::open_default(),
            vps: VpsRuntime::load(),
            phantom: Phantom::new(0),
            apparition: Apparition::new(),
            fx: Fx::new(),
            shop: crate::shop::Shop::new(),
            hub: Hub::new(),
            hub_view: SyncView::default(),
            haunt: Haunt::new(),
            haunt_open: false,
            chat_sending: false,
            chat_agents: 0,
            current_chat: 0,
            last_omen: None,
            events: EventLog::resolve(None),
            outbox: Vec::new(),
            kiro_project,
            kiro_global,
            checkpoint_dir,
            critique_tx,
            critique_rx,
        }
    }

    /// Backend hermétique (tests + `--smoke` : projet tmp, almanach mémoire).
    pub fn new_in(dir: &std::path::Path) -> Self {
        let mut backend = Self::new();
        // Hermétique : ni config user réelle ni HOME (défauts TUI).
        backend.permissions = crate::permissions::Mode::Auto;
        backend.rules = crate::permissions::Rules::default();
        backend.kiro_project = dir.to_path_buf();
        backend.kiro_global = None;
        backend.checkpoint_dir = Some(dir.join("ckpt"));
        backend.hooks = HooksEngine::new(dir.to_path_buf(), None);
        backend.mcp = McpState::for_config(crate::kiro::mcp::Config::default());
        backend.specs = SpecRunner::new(dir.to_path_buf(), Some(dir.join("ckpt")));
        backend.specs.set_mode(backend.permissions);
        backend.almanac = Almanac::memory();
        backend.vps = VpsRuntime::default();
        backend.events = EventLog::disabled();
        backend.permissions_path = None;
        backend.rules_path = None;
        backend
    }

    /// Enregistre le LLM du core (synthèse `/web`).
    pub fn set_llm(&mut self, llm: Arc<dyn Llm>) {
        self.llm = Some(llm);
    }

    /// Mémorise modèle + effort effectifs (synthèse `/web`).
    /// L'intégrateur appelle à chaque changement (model/thinking).
    pub fn note_model(&mut self, model: &str, effort: Option<&str>) {
        self.llm_model = model.to_string();
        self.llm_effort = effort.map(str::to_string);
    }

    /// Suit la connexion vue du core (gate `/spec run`).
    pub fn note_connected(&mut self, connected: bool) {
        self.connected = connected;
        self.specs.set_connected(connected);
    }

    /// Change le mode (permissions + runner specs) et persiste.
    pub fn set_mode(&mut self, mode: Mode) {
        self.permissions = mode;
        self.specs.set_mode(mode);
        if let Some(path) = &self.permissions_path.clone() {
            let _ = permissions::save_to(path, mode);
        }
    }

    /// Mémorise l'activité du chat courant (panneau apparition).
    /// L'intégrateur appelle à chaque envoi/réponse/fin d'agent.
    pub fn note_chat_activity(&mut self, sending: bool, agents: usize) {
        self.chat_sending = sending;
        self.chat_agents = agents;
    }

    /// Mémorise l'index du chat courant (0-based).
    /// L'intégrateur appelle à chaque `chat_select`.
    pub fn note_current_chat(&mut self, index: usize) {
        self.current_chat = index;
    }

    /// Mémorise la vue sessions (sync du hub au prochain tick).
    /// L'intégrateur appelle à chaque changement (chats/agents).
    pub fn note_sessions(&mut self, view: SyncView) {
        self.hub_view = view;
    }

    /// Signale un présage (`error` ou `compact`, 6 s comme le TUI).
    pub fn note_omen(&mut self, kind: &str) {
        let omen = match kind {
            "compact" => Omen::Compact,
            _ => Omen::Error,
        };
        self.apparition.omen(omen);
        self.last_omen = Some((kind.to_string(), Instant::now()));
    }

    /// Effroi F5 : Kiro fuit, apparition jump scare, hub panique
    /// (identique à `App::spook` du TUI).
    pub fn spook(&mut self) {
        self.phantom.scare();
        self.apparition.scare();
        self.hub.scare();
    }

    /// Tick 10 Hz (même ordre que `App::tick` : phantom, apparition,
    /// hub sync+step, fx sur effroi) + tombées hooks/critiques.
    /// L'intégrateur appelle en boucle et route les sorties.
    pub fn tick_10hz(&mut self) -> Vec<BackendOut> {
        let mut outs = Vec::new();
        self.phantom.tick();
        self.apparition.tick();
        self.hub.sync(&self.hub_view);
        self.hub.step();
        self.fx.tick(self.apparition.scaring());
        if self
            .last_omen
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed().as_secs() >= 6)
        {
            self.last_omen = None;
        }
        for out in self.hooks.drain_outcomes() {
            let HookOut::ChatLine { chat, text } = out;
            outs.push(BackendOut::ChatLine { chat, text });
        }
        while let Ok(done) = self.critique_rx.try_recv() {
            if let Some(text) = self.critique_reply(done) {
                outs.push(BackendOut::Critique { text });
            }
        }
        outs
    }

    /// Envoi réel : Kiro juge `prompt` avec `model` (une critique à la
    /// fois ; rien si `/critic off`, sans runtime ou sans LLM).
    /// L'intégrateur appelle à chaque envoi user (miroir `critique()`).
    pub fn note_user_send(&mut self, prompt: &str, model: &str) {
        if !self.phantom.wants_critique() {
            return;
        }
        let (Some(llm), Ok(_)) = (self.llm.clone(), tokio::runtime::Handle::try_current()) else {
            return;
        };
        let messages = crate::phantom::critique_messages(prompt);
        let model = model.to_string();
        let tx = self.critique_tx.clone();
        self.phantom.begin_critique();
        tokio::spawn(async move {
            let reply = llm.complete_full(&messages, &model, None).await;
            let done = match reply {
                Ok(done) => CritiqueDone {
                    text: crate::phantom::clean_critique(&done.text),
                    prompt: done.prompt_tokens,
                    completion: done.completion_tokens,
                },
                Err(_) => CritiqueDone {
                    text: None,
                    prompt: 0,
                    completion: 0,
                },
            };
            let _ = tx.send(done);
        });
    }

    /// Critique arrivée : bulle (erreur = ronchonne, sans détail),
    /// tokens à l'almanach (comme le TUI). Rend le texte dit, le cas
    /// échéant.
    pub fn critique_reply(&mut self, done: CritiqueDone) -> Option<String> {
        self.almanac.record(done.prompt, done.completion);
        match done.text {
            Some(text) => {
                self.phantom
                    .critique_done(Some(text), done.prompt + done.completion);
            }
            None => self.phantom.critique_failed(),
        }
        self.phantom.bubble_text()
    }

    /// Corps d'un skill (erreur `unknown skill` comme le TUI).
    pub fn skill_body(&self, name: &str) -> Result<String, String> {
        let skills =
            crate::kiro::skills::load_all_in(&self.kiro_project, self.kiro_global.as_deref());
        crate::kiro::skills::find(&skills, name)
            .map(|skill| skill.body.clone())
            .ok_or_else(|| format!("unknown skill: {name} — /skills to list"))
    }

    /// Prend le préfixe de skill armé (l'intégrateur le préfixe à la
    /// saisie courante, transitoire comme le TUI).
    pub fn take_skill_prefix(&mut self) -> Option<String> {
        self.skill_prefix.take()
    }

    /// Compteurs VPS pour `vps_list` (statuts affichables).
    pub fn vps_statuses(&self) -> HashMap<String, String> {
        self.vps
            .profiles
            .iter()
            .map(|p| {
                (
                    p.name.clone(),
                    super::cmd_vps::status_text(&self.vps.status_of(&p.name)),
                )
            })
            .collect()
    }
}

impl Default for ToolsBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Verrouille le backend géré (erreur = verrou empoisonné, jamais de
/// panique ; partagé par toutes les commandes, zéro duplication).
pub fn lock<'a>(
    backend: &'a tauri::State<'_, std::sync::Mutex<ToolsBackend>>,
) -> Result<std::sync::MutexGuard<'a, ToolsBackend>, String> {
    backend.lock().map_err(|_| String::from("backend busy"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend(name: &str) -> (ToolsBackend, PathBuf) {
        let dir = std::env::temp_dir().join(format!("bt-state-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        (ToolsBackend::new_in(&dir), dir)
    }

    #[test]
    fn tick_spook_et_omen() {
        let (mut backend, dir) = backend("tick");
        assert!(backend.tick_10hz().is_empty());
        backend.note_omen("error");
        assert_eq!(backend.last_omen.as_ref().unwrap().0, "error");
        backend.spook();
        assert!(backend.apparition.scaring());
        for _ in 0..5 {
            backend.tick_10hz();
        }
        assert!(backend.fx.fear() > 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sync_hub_via_vue_injectee() {
        use crate::hub::{AgentStatus, AgentSync, ChatSync};
        let (mut backend, dir) = backend("hub");
        backend.note_sessions(SyncView {
            chats: vec![ChatSync {
                sending: true,
                agents: vec![AgentSync {
                    id: 1,
                    status: AgentStatus::Running,
                }],
            }],
        });
        backend.tick_10hz();
        assert_eq!(backend.hub.spectres.len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }
}
