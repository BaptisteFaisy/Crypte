//! État global + boucle TUI (crossterm + tokio, tick 100 ms).
//! Le rendu vit dans `app/ui.rs` (mise en page de la crypte), le hub des
//! esprits dans `app/esprits.rs`.

mod esprits;
mod keys;
mod ui;

pub use self::ui::Screen;

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{layout::Rect, DefaultTerminal};
use tokio::sync::mpsc;

use crate::agents;
use crate::apparition::Apparition;
use crate::bus::Bus;
use crate::chat::Role;
use crate::commands;
use crate::connect::{ConnectFlow, NOT_CONNECTED};
use crate::fx::Fx;
use crate::galerie::Galerie;
use crate::gfx::ImageBank;
use crate::ghostty;
use crate::history::{send_window, short_name};
use crate::hub::Hub;
use crate::mascot::{Mascot, MascotState};
use crate::provider::{Completion, Provider, ProviderConfig};
use crate::sessions::Sessions;

/// Message async d'une tâche provider : chat (`idx`), agent
/// (`propriétaire`, id, `livraison`), compaction (`idx`, voir
/// [`agents::compacted_history`]).
pub(crate) enum Incoming {
    Chat(usize, Result<Completion>),
    Agent(usize, usize, usize, Result<Completion>),
    Compact(usize, Result<Completion>),
}

/// État global de l'application.
pub struct App {
    mascot: Mascot,
    fx: Fx,
    bank: ImageBank,
    pub(crate) apparition: Apparition,
    pub(crate) hub: Hub,
    galerie: Galerie,
    tick: u64,
    busy: bool,
    pub(crate) sessions: Sessions,
    pub(crate) bus: Bus,
    pub(crate) provider: Option<Arc<Provider>>,
    pub(crate) connect_flow: Option<ConnectFlow>,
    pub(crate) agents_md: Option<String>,
    pub(crate) screen: Screen,
    /// Rectangle du bouton [+] (mémorisé au rendu, `None` hors sidebar).
    pub(crate) plus_btn: Option<Rect>,
    pub(crate) thinking: Option<String>,
    pub(crate) tx: mpsc::UnboundedSender<Incoming>,
    rx: mpsc::UnboundedReceiver<Incoming>,
}

impl App {
    /// Construit l'app (lit la config provider depuis l'environnement).
    #[allow(dead_code)]
    pub fn new() -> Result<Self> {
        Self::new_with_config(None)
    }

    /// Construit l'app ; si `config_path` est donné, la config TOML est lue
    /// depuis ce fichier (surchargée par l'environnement, voir `from_toml`).
    #[allow(dead_code)]
    pub fn new_with_config(config_path: Option<&str>) -> Result<Self> {
        let cfg = match config_path {
            Some(path) => ProviderConfig::from_toml(path)?,
            None => ProviderConfig::from_env()?,
        };
        Self::from_config(cfg)
    }

    /// Construit l'app depuis une config déjà résolue (sans agents.md).
    pub fn from_config(cfg: ProviderConfig) -> Result<Self> {
        Self::from_config_with_agents(cfg, None)
    }

    /// Construit l'app + pin agents.md dans le chat initial (si chargé),
    /// avec un message système d'annonce (lignes + ~tokens).
    pub fn from_config_with_agents(cfg: ProviderConfig, agents_md: Option<String>) -> Result<Self> {
        let provider = Provider::new(cfg)?;
        Ok(Self::with_provider(
            Some(Arc::new(provider)),
            agents_md,
            false,
        ))
    }

    /// Construit l'app sans provider (`/connect` ensuite pour se connecter).
    #[allow(dead_code)]
    pub fn disconnected() -> Self {
        Self::disconnected_with_agents(None)
    }

    /// Construit l'app déconnectée + pin agents.md (si chargé).
    pub fn disconnected_with_agents(agents_md: Option<String>) -> Self {
        Self::with_provider(None, agents_md, true)
    }

    /// Base commune : état + pin agents.md (+ hint `/connect` si déconnecté).
    fn with_provider(
        provider: Option<Arc<Provider>>,
        agents_md: Option<String>,
        hint_connect: bool,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut app = Self {
            mascot: Mascot::new(),
            fx: Fx::new(),
            bank: ImageBank::headless(),
            apparition: Apparition::new(),
            hub: Hub::new(),
            galerie: Galerie::new(),
            tick: 0,
            busy: false,
            sessions: Sessions::new(),
            bus: Bus::new(),
            provider,
            connect_flow: None,
            agents_md,
            screen: Screen::Chat,
            plus_btn: None,
            thinking: None,
            tx,
            rx,
        };
        app.pin_current();
        app.announce_agents_md();
        if hint_connect {
            app.push_system(String::from(NOT_CONNECTED));
        }
        app
    }

    /// Épingle agents.md en tête du chat courant (si chargé).
    fn pin_current(&mut self) {
        if let Some(content) = self.agents_md.clone() {
            self.sessions.current_mut().pin_system(&content);
        }
    }

    /// Annonce le chargement agents.md dans le chat courant (si chargé).
    fn announce_agents_md(&mut self) {
        if let Some(content) = &self.agents_md {
            let msg = agents::loaded_message(content);
            self.sessions.current_mut().chat.push(Role::System, msg);
        }
    }

    /// Nouveau chat + pin agents.md si chargé (Ctrl+N, `/new`).
    pub(crate) fn add_chat(&mut self) {
        self.sessions.add();
        self.pin_current();
    }

    /// App hors ligne (`--smoke`, `--apercu`) : aucune clé, aucun appel.
    pub fn offline() -> Result<Self> {
        Self::from_config(ProviderConfig {
            base_url: String::from("http://127.0.0.1:9"),
            api_key: String::new(),
            model: String::from("crypt-offline"),
        })
    }

    /// Branche le vrai terminal (après `ratatui::init`) : protocole
    /// d'images (Kitty dans Ghostty) + habillage de la fenêtre.
    pub fn attach_terminal(&mut self) -> Result<()> {
        self.bank = ImageBank::detect();
        ghostty::enter()?;
        Ok(())
    }

    /// Boucle principale : scrute les touches et les réponses toutes les
    /// 10 ms (routage quasi instantané), ne rend que si besoin (touche,
    /// réponse, tick 100 ms pour les animations).
    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let mut last_tick = Instant::now();
        let mut dirty = true;
        loop {
            if dirty {
                ghostty::draw_synced(terminal, |frame| self.render_frame(frame))?;
                dirty = false;
            }
            dirty |= self.drain_replies();
            self.sync_progress()?;
            if event::poll(Duration::from_millis(10))? {
                match event::read()? {
                    Event::Key(key) => {
                        if key.kind == KeyEventKind::Press && !self.on_key(key) {
                            break;
                        }
                    }
                    Event::Mouse(mev) => self.on_mouse(mev),
                    _ => {}
                }
                dirty = true;
            }
            if last_tick.elapsed() >= Duration::from_millis(100) {
                last_tick = Instant::now();
                self.tick();
                dirty = true;
            }
        }
        Ok(())
    }

    /// Barre de progression Ghostty : suit l'attente du chat courant.
    fn sync_progress(&mut self) -> Result<()> {
        let busy = self.sessions.current().sending;
        if busy != self.busy {
            self.busy = busy;
            ghostty::progress(busy)?;
        }
        Ok(())
    }

    /// Traite une touche ; renvoie false pour quitter.
    /// Flow `/connect` actif : touches dédiées (pas de quit direct).
    fn on_key(&mut self, key: KeyEvent) -> bool {
        if self.connect_flow.is_some() {
            return self.on_flow_key(key);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let empty = self.sessions.current().chat.input.is_empty();
        match self.screen {
            Screen::Bestiary => return self.on_bestiary_key(key.code),
            Screen::Hub => return self.on_hub_key(key.code),
            _ => {}
        }
        match key.code {
            KeyCode::Esc => false,
            KeyCode::Char('q') if !ctrl && empty => false,
            KeyCode::Char('n' | 'N') if ctrl => self.cmd(|s| s.add_chat()),
            KeyCode::Char(c) if !ctrl => self.cmd(|s| s.sessions.current_mut().chat.input_push(c)),
            KeyCode::Backspace => self.cmd(|s| s.sessions.current_mut().chat.input_pop()),
            KeyCode::Enter => self.cmd(|s| s.send()),
            KeyCode::F(2) => self.cmd(|s| s.screen = s.screen.toggle(Screen::Tokens)),
            KeyCode::F(3) => self.cmd(|s| s.sessions.prev()),
            KeyCode::F(4) => self.cmd(|s| s.sessions.next()),
            KeyCode::F(5) => self.cmd(|s| s.spook()),
            KeyCode::F(6) => self.cmd(|s| s.screen = s.screen.toggle(Screen::Bestiary)),
            KeyCode::F(7) => self.cmd(|s| s.open_hub()),
            _ => true,
        }
    }

    /// F5 : Kiro s'effraie et une créature surgit (jump scare + éclair).
    pub(crate) fn spook(&mut self) {
        self.mascot.spook();
        self.apparition.scare();
        self.hub.scare();
    }

    /// Exécute `f` puis rend true (touche gérée, ne quitte pas).
    pub(crate) fn cmd(&mut self, f: impl FnOnce(&mut Self)) -> bool {
        f(self);
        true
    }

    /// Enfile l'input : commande locale ou complétion en tâche de fond.
    /// Un chat sans message user prend le nom du premier message
    /// (ignore le system pinné : historique non vide mais sans user).
    fn send(&mut self) {
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
        if let Some(used) = self.context_over_limit(&text) {
            self.push_system(format!(
                "context full ({}), send refused: /compact to continue",
                crate::tokens::fmt_context(used, self.context_limit())
            ));
            return;
        }
        let model = self.effective_model();
        let effort = self.thinking.clone();
        let tx = self.tx.clone();
        let sess = self.sessions.current_mut();
        sess.chat.take_input();
        if !sess.history.iter().any(|(role, _)| role == "user") {
            sess.name = short_name(&text);
        }
        sess.chat.push(Role::User, text.clone());
        sess.history.push(("user".to_string(), text));
        let history = send_window(&sess.history, sess.pinned_system);
        sess.sending = true;
        sess.auto_hops = 0;
        sess.sending_since = Some(Instant::now());
        tokio::spawn(async move {
            let reply = provider
                .complete_full(&history, &model, effort.as_deref())
                .await;
            let _ = tx.send(Incoming::Chat(id, reply));
        });
    }

    /// Réponses arrivées, routées vers leur chat (non bloquant, id invalide
    /// ignoré). Rend vrai si au moins un message a été routé (rendu sale).
    fn drain_replies(&mut self) -> bool {
        let mut routed = false;
        while let Ok(msg) = self.rx.try_recv() {
            routed = true;
            match msg {
                Incoming::Chat(id, reply) => self.drain_chat(id, reply),
                Incoming::Agent(owner, agent_id, deliver, reply) => {
                    self.drain_agent(owner, agent_id, deliver, reply);
                }
                Incoming::Compact(id, reply) => self.drain_compact(id, reply),
            }
        }
        routed
    }

    /// Modèle effectif : celui du chat courant, sinon le défaut provider.
    pub(crate) fn effective_model(&self) -> String {
        self.effective_model_for(self.sessions.index())
    }

    /// Modèle effectif du chat `idx` (le sien, sinon le défaut provider).
    pub(crate) fn effective_model_for(&self, idx: usize) -> String {
        if let Some(model) = self.sessions.get(idx).and_then(|c| c.model.clone()) {
            return model;
        }
        self.provider
            .as_ref()
            .map(|p| p.config().model.clone())
            .unwrap_or_else(|| String::from("—"))
    }

    /// Thinking affiché (`None` → "default").
    pub(crate) fn thinking_text(&self) -> String {
        self.thinking
            .clone()
            .unwrap_or_else(|| String::from("default"))
    }

    /// Ajoute un message système au chat courant.
    pub(crate) fn push_system(&mut self, msg: String) {
        self.sessions.current_mut().chat.push(Role::System, msg);
    }

    /// Fait avancer les animations (mascotte, apparitions, effets).
    pub(crate) fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.mascot.tick();
        self.apparition.tick();
        self.hub.sync(&self.sessions);
        self.hub.step();
        self.fx.tick(self.mascot.state() == MascotState::Spook);
    }
}

/// Nom court de modèle : après le dernier '/', max 14 caractères.
pub fn short_model(model: &str) -> String {
    model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .chars()
        .take(14)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderConfig;
    use crate::sessions::AgentStatus;

    /// App de test (clé factice, aucun appel réseau dans ces tests).
    fn test_app() -> App {
        let cfg = ProviderConfig {
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

    #[test]
    fn tell_rejette_invalide_enfile_si_occupe() {
        let mut app = test_app();
        app.run_command("/tell 9 hello");
        assert_eq!(last_text(&app, 0), "chat 9 not found");
        app.run_command("/tell 1 hello");
        assert_eq!(
            last_text(&app, 0),
            "this is the current chat — type directly"
        );
        app.sessions.get_mut(0).unwrap().sending = true;
        app.add_chat(); // courant = chat 2
        app.run_command("/tell 1 hello");
        assert_eq!(last_text(&app, 1), "Chat 1 busy — queued");
        // Enfilé : affiché aussitôt, versé à l'historique au déclenchement.
        let dest = app.sessions.get(0).unwrap();
        assert_eq!(dest.inbox.len(), 1);
        assert!(dest.history.is_empty());
        assert_eq!(last_text(&app, 0), "[from Chat 2] hello");
    }

    #[test]
    fn ask_rejette_agent_inconnu() {
        let mut app = test_app();
        app.run_command("/ask #42 suite");
        assert_eq!(last_text(&app, 0), "agent#42 unknown");
    }

    #[test]
    fn agent_rejette_livraison_inexistante_sans_creer() {
        let mut app = test_app();
        app.run_command("/agent @9 fais ça");
        assert_eq!(last_text(&app, 0), "chat 9 not found");
        assert!(app.sessions.current().agents.is_empty());
    }

    #[test]
    fn hint_affiche_usage() {
        let mut app = test_app();
        app.run_command("/tell");
        assert_eq!(last_text(&app, 0), commands::TELL_USAGE);
        app.run_command("/ask #1");
        assert_eq!(last_text(&app, 0), commands::ASK_USAGE);
    }

    #[test]
    fn drain_agent_livre_ailleurs_statut_chez_proprietaire() {
        let mut app = test_app();
        app.add_chat();
        let history = vec![(String::from("user"), String::from("u"))];
        let id = app.sessions.add_agent(0, "prompt", history).unwrap();
        let done = Completion {
            text: String::from("réponse"),
            prompt_tokens: 10,
            completion_tokens: 5,
        };
        app.drain_agent(0, id, 1, Ok(done));
        // Statut + échange chez le propriétaire, pas de tokens ici.
        let owner = app.sessions.get(0).unwrap();
        assert_eq!(owner.agents[0].status, AgentStatus::Done);
        assert_eq!(owner.agents[0].history.len(), 2);
        assert_eq!(owner.ledger.total(), 0);
        // Ledger + message tagué sur la livraison.
        assert_eq!(app.sessions.get(1).unwrap().ledger.total(), 15);
        assert_eq!(last_text(&app, 1), "[agent#1 from Chat 1] réponse");
    }

    #[test]
    fn drain_agent_erreur_marquee_chez_proprietaire() {
        let mut app = test_app();
        let history = vec![(String::from("user"), String::from("u"))];
        let id = app.sessions.add_agent(0, "prompt", history).unwrap();
        app.drain_agent(0, id, 0, Err(anyhow::anyhow!("boom")));
        assert_eq!(
            app.sessions.get(0).unwrap().agents[0].status,
            AgentStatus::Failed
        );
        assert_eq!(last_text(&app, 0), "[agent#1] error: boom");
    }
}
