//! État global + boucle TUI (crossterm + tokio, tick 100 ms).
//! Les constructeurs vivent dans `app/ctors.rs`, le rendu dans `app/ui.rs`
//! (mise en page de la crypte), le panneau hub dans `app/esprits.rs`.

mod critic;
mod ctors;
mod esprits;
mod feedback;
mod haunt;
mod keys;
mod model;
mod picker;
mod send;
mod space;
mod ui;

pub use self::ui::Screen;

use std::time::{Duration, Instant};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::Result;
use crossterm::event::{self, Event};
use ratatui::{layout::Rect, DefaultTerminal};
use tokio::sync::mpsc;

use crate::almanac::Almanac;
use crate::apparition::Apparition;
use crate::bus::Bus;
use crate::connect::ConnectFlow;
use crate::events::EventLog;
use crate::fx::Fx;
use crate::gfx::ImageBank;
use crate::ghostty;

use crate::haunt::Haunt;
use crate::hub::Hub;
use crate::kiro::hook_exec::HookOutcome;
use crate::kiro::hook_run::HookState;
use crate::kiro::mcp_pool::McpState;
use crate::kiro::spec_run::{SpecGen, SpecRun};
use crate::permissions::Mode;
use crate::phantom::Phantom;
use crate::pool::SpawnPool;
use crate::provider::{Completion, Provider};
use crate::sessions::Sessions;
use crate::settings::Settings;
use crate::shop::Shop;
use crate::tools::flow::{Delivery, ToolRun};
use crate::web::WebAnswer;

/// Message async d'une tâche provider : chat (`idx`, époque du vol),
/// agent (`propriétaire`, id, `livraison`), compaction (`idx`, voir
/// [`crate::agents::compacted_history`]), recherche web (`idx`, [`WebAnswer`]),
/// outils (`idx`, livraisons d'exécution, voir [`crate::tools::flow`]),
/// génération spec (`chat`, état chaîné, voir [`crate::kiro::spec_run`]),
/// MCP (`/mcp refresh` terminé, statut à afficher), hook (commande
/// terminée, voir [`crate::kiro::hook_run`]), critique de Kiro (voir
/// [`crate::phantom`]), cloud (voir [`crate::kiro::cloud`]).
pub(crate) enum Incoming {
    Chat(usize, u64, Result<Completion>),
    Agent(usize, usize, usize, Result<Completion>),
    Compact(usize, Result<Completion>),
    Web(usize, Result<WebAnswer>),
    ToolDone(usize, Vec<Delivery>),
    Critique(Result<Completion>),
    Vps(crate::vps::VpsOutcome),
    SpecNew(usize, SpecGen, Result<Completion>),
    McpText(String),
    HookDone(HookOutcome),
    Cloud(usize, usize, crate::kiro::cloud::CloudMsg),
}

/// État global de l'application.
pub struct App {
    /// Kiro en balade : juge les prompts, se fait frapper (`/critic`).
    phantom: Phantom,
    pub(crate) fx: Fx,
    bank: ImageBank,
    pub(crate) apparition: Apparition,
    pub(crate) hub: Hub,
    /// Pont d'événements vers le fond 3D (gratuit quand la vue est coupée).
    pub(crate) scene: crate::space::scene::SceneLog,
    /// Fond 3D « project space » (F8) ; `None` = coupé.
    pub(crate) space_view: Option<crate::space::view::SpaceView>,
    /// Une séquence Kitty d'effacement est en attente (juste après F8).
    pub(crate) space_delete: bool,
    /// Diff du disque pour la 3D (`None` quand la vue est coupée).
    pub(crate) space_watch: Option<crate::space::watch::Watcher>,
    pub(crate) shop: Shop,
    /// Jeu Haunt (F9, pause hors écran) et son clavier (`app/haunt.rs`).
    pub(crate) haunt: Haunt,
    pub(crate) keyboard: haunt::Keyboard,
    tick: u64,
    busy: bool,
    /// Jauge contexte mise en cache (tokens estimés + fenêtre modèle),
    /// rafraîchie 1×/s : le calcul (BPE + clones) est trop coûteux à
    /// chaque frame (10 Hz). Voir `refresh_ctx_cache`.
    ctx_used: u64,
    ctx_limit: u64,
    pub(crate) sessions: Sessions,
    /// Tokens par jour (affichés par `/usage`) ; mémoire seule tant que
    /// `main` n'ouvre pas le fichier.
    pub(crate) almanac: Almanac,
    pub(crate) bus: Bus,
    /// Providers + API keys (`/settings`): encrypted vault, masked input.
    pub(crate) settings: Settings,
    pub(crate) provider: Option<Arc<Provider>>,
    pub(crate) connect_flow: Option<ConnectFlow>,
    pub(crate) agents_md: Option<String>,
    /// Fichier `--agents-file` (`None` = lookup cwd), lu au premier envoi.
    pub(crate) agents_file: Option<String>,
    /// Lecture agents.md déjà tentée (une seule lecture, jamais relue).
    pub(crate) agents_tried: bool,
    pub(crate) screen: Screen,
    /// Ajustement manuel de la colonne droite (Ctrl+←/→, 0 = auto).
    pub(crate) right_delta: i16,
    /// Ajustement manuel de la sidebar (drag souris, 0 = auto).
    pub(crate) side_delta: i16,
    /// Rectangle de la sidebar (mémorisé au rendu, `None` hors chat).
    pub(crate) side_area: Option<Rect>,
    /// Largeur auto de la sidebar au dernier rendu (base du delta).
    pub(crate) side_auto: u16,
    /// Drag sidebar en cours : décalage souris → bord droit (anti-saut).
    pub(crate) resizing_side: Option<i16>,
    /// Poignée sidebar survolée (mise en relief).
    pub(crate) side_hot: bool,
    /// Dernière sélection copiée (repli si le presse-papiers système est illisible).
    pub(crate) clip_last: String,
    /// Rectangle du bouton [+] (mémorisé au rendu, `None` hors sidebar).
    pub(crate) plus_btn: Option<Rect>,
    /// Bloc des lignes de chats de la sidebar (mémorisé au rendu, une
    /// ligne par chat, `None` hors écran chat) : clic = choisir le chat.
    pub(crate) side_rows: Option<Rect>,
    /// Boutons [×] des chats (mémorisés au rendu, un par chat, `None` si
    /// masqué) : clic = fermer le chat (vide hors écran chat).
    pub(crate) close_btns: Vec<Option<Rect>>,
    /// Onglets de l'en-tête (mémorisés au rendu, écran + rectangle) :
    /// clic = aller à l'écran (chat, boutique).
    pub(crate) tab_hits: Vec<(Screen, Rect)>,
    /// Lignes des articles de la boutique (mémorisées au rendu) :
    /// clic = acheter (vide hors écran boutique).
    pub(crate) shop_rows: Vec<Rect>,
    pub(crate) thinking: Option<String>,
    pub(crate) permissions: Mode,
    /// Fichier de persistance du mode (`None` si `HOME` manquant).
    pub(crate) permissions_path: Option<PathBuf>,
    /// État T5 : règles fines + chemins Kiro résolus à la construction.
    pub(crate) t5: crate::permissions::T5State,
    /// Envois outillés (`/tools on`, défaut) ; boucle agent dans `tool_run`.
    pub(crate) tools_on: bool,
    /// Boucle agent en cours (`None` = aucun envoi outillé actif).
    pub(crate) tool_run: Option<ToolRun>,
    /// Runs `/spec run` par agent porteur (retiré à sa tombée).
    pub(crate) spec_runs: HashMap<usize, SpecRun>,
    /// Run en attente de `y/n` (mode `auto`, un seul à la fois).
    pub(crate) spec_waiting: Option<SpecRun>,
    /// Parc MCP (connexions paresseuses, `/mcp`, arrêt à la sortie).
    pub(crate) mcp: McpState,
    /// Moteur hooks Kiro (file `agent`, récents, gardes, `/hooks`).
    pub(crate) hooks: HookState,
    /// Fichier de persistance des chats (`None` = off : tests, smoke, demo).
    pub(crate) chats_path: Option<PathBuf>,
    /// Journal d'événements temps réel (`--events-log`, désactivé sinon).
    pub(crate) events: EventLog,
    /// VPS : profils nommés + statuts distants (`/vps`).
    pub(crate) vps: crate::vps::VpsRuntime,
    pub(crate) cloud: crate::kiro::cloud::CloudJobs,
    pub(crate) tx: mpsc::Sender<Incoming>,
    pub(crate) rx: mpsc::Receiver<Incoming>,
    /// Pool borné d'appels LLM (plafond `CRYPTE_MAX_FLIGHT`, défaut 4).
    pub(crate) pool: SpawnPool,
}

impl App {
    /// Branche le vrai terminal (après `ratatui::init`) : protocole
    /// d'images (Kitty dans Ghostty) + habillage de la fenêtre.
    pub fn attach_terminal(&mut self) -> Result<()> {
        self.bank = ImageBank::detect();
        self.keyboard.live = true;
        ghostty::enter()?;
        Ok(())
    }

    /// Boucle principale : scrute les touches et les réponses toutes les
    /// 10 ms (routage quasi instantané), ne rend que si besoin (touche,
    /// réponse, tick 100 ms pour les animations). Sur l'écran Haunt, le
    /// jeu tourne à 240 Hz et seules ses frames déclenchent le rendu.
    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let mut last_tick = Instant::now();
        let mut last_pump = Instant::now();
        let mut last_space = Instant::now();
        let mut dirty = true;
        'main: loop {
            if dirty {
                ghostty::draw_synced(terminal, |frame| self.render_frame(frame))?;
                dirty = false;
            }
            dirty |= self.drain_replies();
            self.sync_progress()?;
            if event::poll(self.poll_wait())? {
                // Rafale : vide la file d'attente (frappe rapide sans retard).
                loop {
                    match event::read()? {
                        Event::Key(key) => {
                            if !self.on_key(key) {
                                break 'main;
                            }
                        }
                        Event::Mouse(mev) => self.on_mouse(mev),
                        Event::Paste(text) => self.on_paste(text),
                        _ => {}
                    }
                    dirty |= self.screen != Screen::Haunt;
                    if !event::poll(Duration::ZERO)? {
                        break;
                    }
                }
            }
            if last_tick.elapsed() >= Duration::from_millis(100) {
                last_tick = Instant::now();
                self.tick();
                dirty |= self.screen != Screen::Haunt;
            }
            dirty |= self.pump_haunt(&mut last_pump);
            dirty |= self.pump_space(&mut last_space);
        }
        self.release_keys();
        self.mcp.shutdown().await;
        self.save_now();
        Ok(())
    }

    /// F5 : une créature surgit (jump scare + éclair), le hub panique et
    /// Kiro (en balade) prend peur.
    pub(crate) fn spook(&mut self) {
        self.phantom.scare();
        self.apparition.scare();
        self.hub.scare();
    }

    /// Exécute `f` puis rend true (touche gérée, ne quitte pas).
    pub(crate) fn cmd(&mut self, f: impl FnOnce(&mut Self)) -> bool {
        f(self);
        true
    }

    /// Réponses arrivées, routées vers leur chat (non bloquant, id invalide
    /// ignoré). Rend vrai si au moins un message a été routé (rendu sale).
    fn drain_replies(&mut self) -> bool {
        let mut routed = false;
        while let Ok(msg) = self.rx.try_recv() {
            routed = true;
            if let Some((prompt, completion)) = msg.usage() {
                self.almanac.record(prompt, completion);
            }
            match msg {
                Incoming::Chat(id, epoch, reply) => self.drain_chat(id, epoch, reply),
                Incoming::Agent(owner, agent_id, deliver, reply) => {
                    self.drain_agent(owner, agent_id, deliver, reply);
                }
                Incoming::Compact(id, reply) => self.drain_compact(id, reply),
                Incoming::Web(id, reply) => self.drain_web(id, reply),
                Incoming::ToolDone(id, deliveries) => self.drain_tool_done(id, deliveries),
                Incoming::Critique(reply) => self.drain_critique(reply),
                Incoming::Vps(outcome) => self.drain_vps(outcome),
                Incoming::SpecNew(chat, gen, reply) => self.drain_spec_new(chat, gen, reply),
                Incoming::McpText(text) => self.drain_mcp_text(text),
                Incoming::HookDone(out) => self.drain_hook_done(out),
                Incoming::Cloud(chat, job, msg) => self.drain_cloud(chat, job, msg),
            }
        }
        routed
    }

    /// Fait avancer les animations (apparitions, hub, effets).
    pub(crate) fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.autosave_if_due(self.tick);
        self.phantom.tick();
        self.apparition.tick();
        self.hub.sync(&self.sessions);
        self.hub.step();
        self.fx.tick(self.apparition.scaring());
        if self.tick % 10 == 0 {
            self.refresh_ctx_cache();
        }
        if self.tick % 20 == 0 {
            self.poll_space_watch();
        }
        crate::mbox::eveil::tour(&self.events);
    }

    /// Rafraîchit la jauge contexte en cache (1×/s + 1× construction).
    pub(crate) fn refresh_ctx_cache(&mut self) {
        self.ctx_used = self.context_used();
        self.ctx_limit = self.context_limit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands;
    use crate::provider::ProviderConfig;
    use crate::sessions::AgentStatus;
    use ratatui::{backend::TestBackend, Terminal};

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
    fn ctrl_fleches_redimensionnent_la_colonne() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = test_app();
        let key = |c| KeyEvent::new(c, KeyModifiers::CONTROL);
        assert!(app.on_key(key(KeyCode::Left)));
        assert_eq!(app.right_delta, 2);
        assert!(app.on_key(key(KeyCode::Right)));
        assert_eq!(app.right_delta, 0);
    }

    #[test]
    fn ctrl_w_ferme_le_chat_courant() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = test_app();
        app.add_chat();
        let key = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert!(app.on_key(key));
        assert_eq!(app.sessions.count(), 1);
        assert_eq!(app.sessions.current().name, "Chat 1");
    }

    #[test]
    fn resize_colonne_borne_le_delta() {
        let mut app = test_app();
        app.resize_right(200);
        assert_eq!(app.right_delta, 64);
        app.resize_right(-200);
        assert_eq!(app.right_delta, -64);
    }

    #[test]
    fn every_billed_reply_counts_in_the_almanac() {
        let mut app = test_app();
        let done = |p, c| Completion {
            text: String::from("ok"),
            tool_calls: Vec::new(),
            prompt_tokens: p,
            completion_tokens: c,
        };
        let raw_web = WebAnswer {
            text: String::from("raw"),
            backend: "ddg",
            prompt_tokens: 0,
            completion_tokens: 0,
        };
        let tx = app.tx.clone();
        tx.try_send(Incoming::Chat(0, 0, Ok(done(10, 5)))).unwrap();
        tx.try_send(Incoming::Compact(7, Ok(done(3, 2)))).unwrap();
        tx.try_send(Incoming::Chat(0, 0, Err(anyhow::anyhow!("boom"))))
            .unwrap();
        tx.try_send(Incoming::Web(0, Ok(raw_web))).unwrap();
        app.drain_replies();
        let tonight = app.almanac.day(crate::almanac::today());
        assert_eq!((tonight.total(), tonight.calls), (20, 2));
    }

    #[test]
    fn bestiary_montre_une_creature_au_hasard() {
        let mut app = test_app();
        app.run_command("/bestiary");
        let shown = app.apparition.current(false, 0);
        assert_ne!(shown.slug, "fantome");
        assert_eq!(
            last_text(&app, 0),
            format!("{} — {}", shown.name, shown.lore)
        );
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
            tool_calls: Vec::new(),
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
    fn status_bar_shows_no_render_mode() {
        let mut app = App::offline().unwrap();
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer();
        let foot: String = (0..80).map(|x| buf[(x, 23)].symbol()).collect();
        for word in ["kitty", "halfblocks", "sixel", "iterm2", "◆"] {
            assert!(!foot.contains(word), "mode visible : {foot:?}");
        }
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
