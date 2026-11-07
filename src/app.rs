//! État global + boucle TUI (crossterm + tokio, tick 100 ms).

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::widgets::Paragraph;
use ratatui::{DefaultTerminal, Frame};
use tokio::sync::mpsc;

use crate::agents;
use crate::chat::Role;
use crate::commands;
use crate::fx::Fx;
use crate::mascot::{Mascot, MascotState};
use crate::provider::{Completion, Provider, ProviderConfig};
use crate::sessions::Sessions;
use crate::tokens::fmt_tps;

/// Écran affiché dans la zone principale.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Chat,
    Tokens,
}

impl Screen {
    fn toggle(self) -> Self {
        match self {
            Screen::Chat => Screen::Tokens,
            Screen::Tokens => Screen::Chat,
        }
    }
}

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
    pub(crate) sessions: Sessions,
    pub(crate) provider: Arc<Provider>,
    pub(crate) agents_md: Option<String>,
    screen: Screen,
    pub(crate) thinking: Option<String>,
    pub(crate) tx: mpsc::UnboundedSender<Incoming>,
    rx: mpsc::UnboundedReceiver<Incoming>,
}

/// Tours de conversation max envoyés au provider (borne le prompt).
const MAX_HISTORY: usize = 40;
/// Caractères du premier message repris comme nom du chat.
const NAME_LEN: usize = 18;
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
        let (tx, rx) = mpsc::unbounded_channel();
        let mut app = Self {
            mascot: Mascot::new(),
            fx: Fx::new(),
            sessions: Sessions::new(),
            provider: Arc::new(provider),
            agents_md,
            screen: Screen::Chat,
            thinking: None,
            tx,
            rx,
        };
        app.pin_current();
        app.announce_agents_md();
        Ok(app)
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

    /// Boucle principale : rend à chaque tour, tick 100 ms même sans entrée.
    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        loop {
            terminal.draw(|frame| self.draw(frame))?;
            self.drain_replies();
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press && !self.on_key(key) {
                        break;
                    }
                }
            }
            self.tick();
        }
        Ok(())
    }

    /// Traite une touche ; renvoie false pour quitter.
    fn on_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let empty = self.sessions.current().chat.input.is_empty();
        match key.code {
            KeyCode::Esc => false,
            KeyCode::Char('q') if !ctrl && empty => false,
            KeyCode::Char('n' | 'N') if ctrl => self.cmd(|s| s.add_chat()),
            KeyCode::Char(c) if !ctrl => self.cmd(|s| s.sessions.current_mut().chat.input_push(c)),
            KeyCode::Backspace => self.cmd(|s| s.sessions.current_mut().chat.input_pop()),
            KeyCode::Enter => self.cmd(|s| s.send()),
            KeyCode::F(2) => self.cmd(|s| s.screen = s.screen.toggle()),
            KeyCode::F(3) => self.cmd(|s| s.sessions.prev()),
            KeyCode::F(4) => self.cmd(|s| s.sessions.next()),
            KeyCode::F(5) => self.cmd(|s| s.mascot.spook()),
            _ => true,
        }
    }

    /// Exécute `f` puis rend true (touche gérée, ne quitte pas).
    fn cmd(&mut self, f: impl FnOnce(&mut Self)) -> bool {
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
        if self.sessions.current().sending {
            return;
        }
        let model = self.effective_model();
        let effort = self.thinking.clone();
        let provider = Arc::clone(&self.provider);
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
        sess.sending_since = Some(Instant::now());
        tokio::spawn(async move {
            let reply = provider
                .complete_full(&history, &model, effort.as_deref())
                .await;
            let _ = tx.send(Incoming::Chat(id, reply));
        });
    }

    /// Réponses arrivées, routées vers leur chat (non bloquant, id invalide ignoré).
    fn drain_replies(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Incoming::Chat(id, reply) => self.drain_chat(id, reply),
                Incoming::Agent(owner, agent_id, deliver, reply) => {
                    self.drain_agent(owner, agent_id, deliver, reply);
                }
                Incoming::Compact(id, reply) => self.drain_compact(id, reply),
            }
        }
    }

    /// Modèle effectif : celui du chat courant, sinon le défaut provider.
    pub(crate) fn effective_model(&self) -> String {
        self.effective_model_for(self.sessions.index())
    }

    /// Modèle effectif du chat `idx` (le sien, sinon le défaut provider).
    pub(crate) fn effective_model_for(&self, idx: usize) -> String {
        self.sessions
            .get(idx)
            .and_then(|c| c.model.clone())
            .unwrap_or_else(|| self.provider.config().model.clone())
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

    /// Fait avancer les animations (mascotte + effets).
    fn tick(&mut self) {
        self.mascot.tick();
        self.fx.tick(self.mascot.state() == MascotState::Spook);
    }

    /// Dessine tout l'écran selon l'écran actif.
    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        if area.width < 80 || area.height < 24 {
            let msg = "Terminal trop petit : 80x24 requis (Echap pour quitter)";
            frame.render_widget(Paragraph::new(msg), area);
            return;
        }
        // Saisie rendue par `chat.draw` (anti double saisie : pas de rangée ici).
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(10),
                Constraint::Length(1),
            ])
            .split(area);
        let title = "CRYPT-CHAT  (q:quit F2:tokens F3/F4:chat Ctrl+N:new F5:spook)";
        frame.render_widget(Paragraph::new(title), rows[0]);
        match self.screen {
            Screen::Chat => self.draw_chat(frame, rows[1]),
            Screen::Tokens => self.sessions.current().ledger.draw(frame, rows[1]),
        }
        frame.render_widget(Paragraph::new(self.status_text()), rows[2]);
        self.fx.draw(frame.area(), frame.buffer_mut());
    }

    /// Corps chat : sidebar + (discussion avec saisie + mascotte).
    fn draw_chat(&mut self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(20), Constraint::Min(10)])
            .split(area);
        self.sessions.draw_sidebar(frame, cols[0]);
        let rest = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(cols[1]);
        self.sessions.current().chat.draw(frame, rest[0]);
        self.mascot.draw(frame, rest[1]);
    }

    /// Barre de statut : `chat i/n • modèle • X tok • Y` (`—` si tps nul).
    fn status_text(&self) -> String {
        let sess = self.sessions.current();
        format!(
            "chat {}/{} • {} • {} tok • {}",
            self.sessions.index() + 1,
            self.sessions.count(),
            short_model(&self.effective_model()),
            sess.ledger.total(),
            fmt_tps(sess.ledger.last_tps())
        )
    }
}

/// Fenêtre d'historique envoyée au provider : 40 derniers, tête
/// `system` pinnée gardée (partagée par `send` et `/tell`).
pub(crate) fn send_window(history: &[(String, String)], pinned: bool) -> Vec<(String, String)> {
    let mut window = history.to_vec();
    if window.len() > MAX_HISTORY {
        let drop = window.len() - MAX_HISTORY;
        if pinned && !window.is_empty() {
            window.drain(1..1 + drop);
        } else {
            window.drain(..drop);
        }
    }
    window
}

/// Nom de chat depuis le premier message : 18 premiers caractères trimmés.
fn short_name(text: &str) -> String {
    text.chars()
        .take(NAME_LEN)
        .collect::<String>()
        .trim()
        .to_string()
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
    fn tell_rejette_cible_invalide_ou_occupee() {
        let mut app = test_app();
        app.run_command("/tell 9 hello");
        assert_eq!(last_text(&app, 0), "chat 9 inexistant");
        app.run_command("/tell 1 hello");
        assert_eq!(last_text(&app, 0), "c'est ce chat-ci — écris directement");
        app.sessions.get_mut(0).unwrap().sending = true;
        app.add_chat(); // courant = chat 2
        app.run_command("/tell 1 hello");
        assert_eq!(last_text(&app, 1), "Chat 1 occupé, réessaie");
        // Refus = rien injecté nulle part.
        assert!(app.sessions.get(0).unwrap().history.is_empty());
    }

    #[test]
    fn ask_rejette_agent_inconnu() {
        let mut app = test_app();
        app.run_command("/ask #42 suite");
        assert_eq!(last_text(&app, 0), "agent#42 inconnu");
    }

    #[test]
    fn agent_rejette_livraison_inexistante_sans_creer() {
        let mut app = test_app();
        app.run_command("/agent @9 fais ça");
        assert_eq!(last_text(&app, 0), "chat 9 inexistant");
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
        assert_eq!(last_text(&app, 1), "[agent#1 de Chat 1] réponse");
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
        assert_eq!(last_text(&app, 0), "[agent#1] erreur: boom");
    }
}
