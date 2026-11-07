//! État global + boucle TUI (crossterm + tokio, tick 100 ms).
//! Les constructeurs vivent dans `app/ctors.rs`, le rendu dans `app/ui.rs`
//! (mise en page de la crypte), le hub des esprits dans `app/esprits.rs`.

mod ctors;
mod esprits;
mod keys;
mod ui;

pub use self::ui::Screen;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{layout::Rect, DefaultTerminal};
use tokio::sync::mpsc;

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
use crate::permissions::Mode;
use crate::provider::{Completion, Provider};
use crate::sessions::Sessions;
use crate::web::WebAnswer;

/// Message async d'une tâche provider : chat (`idx`), agent
/// (`propriétaire`, id, `livraison`), compaction (`idx`, voir
/// [`crate::agents::compacted_history`]), recherche web (`idx`, [`WebAnswer`]).
pub(crate) enum Incoming {
    Chat(usize, Result<Completion>),
    Agent(usize, usize, usize, Result<Completion>),
    Compact(usize, Result<Completion>),
    Web(usize, Result<WebAnswer>),
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
    /// Jauge contexte mise en cache (tokens estimés + fenêtre modèle),
    /// rafraîchie 1×/s : le calcul (BPE + clones) est trop coûteux à
    /// chaque frame (10 Hz). Voir `refresh_ctx_cache`.
    ctx_used: u64,
    ctx_limit: u64,
    pub(crate) sessions: Sessions,
    pub(crate) bus: Bus,
    pub(crate) provider: Option<Arc<Provider>>,
    pub(crate) connect_flow: Option<ConnectFlow>,
    pub(crate) agents_md: Option<String>,
    pub(crate) screen: Screen,
    /// Rectangle du bouton [+] (mémorisé au rendu, `None` hors sidebar).
    pub(crate) plus_btn: Option<Rect>,
    pub(crate) thinking: Option<String>,
    pub(crate) permissions: Mode,
    /// Fichier de persistance du mode (`None` si `HOME` manquant).
    pub(crate) permissions_path: Option<PathBuf>,
    pub(crate) tx: mpsc::UnboundedSender<Incoming>,
    rx: mpsc::UnboundedReceiver<Incoming>,
}

impl App {
    /// Nouveau chat + pin agents.md si chargé (Ctrl+N, `/new`).
    pub(crate) fn add_chat(&mut self) {
        self.sessions.add();
        self.pin_current();
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
                Incoming::Web(id, reply) => self.drain_web(id, reply),
            }
        }
        routed
    }

    /// Modèle effectif : celui du chat courant, sinon le défaut provider.
    pub(crate) fn effective_model(&self) -> String {
        self.effective_model_ref().to_string()
    }

    /// Modèle effectif du chat `idx` (le sien, sinon le défaut provider).
    pub(crate) fn effective_model_for(&self, idx: usize) -> String {
        self.effective_model_ref_for(idx).to_string()
    }

    /// Modèle effectif du chat courant, emprunté (barre d'état, zéro alloc).
    pub(crate) fn effective_model_ref(&self) -> &str {
        self.effective_model_ref_for(self.sessions.index())
    }

    /// Modèle effectif du chat `idx`, emprunté : le sien, sinon le
    /// défaut provider, sinon `—`.
    fn effective_model_ref_for(&self, idx: usize) -> &str {
        if let Some(model) = self.sessions.get(idx).and_then(|c| c.model.as_deref()) {
            return model;
        }
        self.provider
            .as_ref()
            .map(|p| p.config().model.as_str())
            .unwrap_or("—")
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
        if self.tick % 10 == 0 {
            self.refresh_ctx_cache();
        }
    }

    /// Rafraîchit la jauge contexte mise en cache (1×/s depuis `tick`,
    /// + 1× à la construction) : la frame ne fait plus que lire.
    pub(crate) fn refresh_ctx_cache(&mut self) {
        self.ctx_used = self.context_used();
        self.ctx_limit = self.context_limit();
    }
}

/// Nom court de modèle : après le dernier '/', max 14 caractères.
/// Emprunté quand rien n'est tronqué (cas courant, zéro alloc).
pub fn short_model(model: &str) -> Cow<'_, str> {
    let base = model.rsplit('/').next().unwrap_or(model);
    match base.char_indices().map(|(i, _)| i).nth(14) {
        Some(end) => Cow::Owned(base[..end].to_string()),
        None => Cow::Borrowed(base),
    }
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
    fn permissions_affiche_change_persiste_et_rejette() {
        let mut app = test_app();
        app.permissions = Mode::Auto;
        let dir = std::env::temp_dir().join(format!("kh-app-{}-perms", std::process::id()));
        app.permissions_path = Some(dir.join("permissions"));
        app.run_command("/permissions");
        assert!(last_text(&app, 0).starts_with("permissions: auto (asks"));
        app.run_command("/permissions yolo");
        assert!(last_text(&app, 0).starts_with("permissions: yolo (runs"));
        assert_eq!(app.permissions, Mode::Yolo);
        assert_eq!(
            crate::permissions::load_from(&dir.join("permissions")),
            Mode::Yolo
        );
        app.run_command("/permissions turbo");
        assert!(last_text(&app, 0).starts_with("invalid mode: turbo"));
        assert_eq!(app.permissions, Mode::Yolo);
        std::fs::remove_dir_all(&dir).ok();
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
