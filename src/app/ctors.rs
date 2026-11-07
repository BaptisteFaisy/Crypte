//! Constructeurs de [`App`] (config env/TOML, déconnecté, hors ligne)
//! + pin agents.md dans le chat initial.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc;

use super::{App, Screen};
use crate::agents;
use crate::almanac::Almanac;
use crate::apparition::Apparition;
use crate::bus::Bus;
use crate::chat::Role;
use crate::connect::NOT_CONNECTED;
use crate::events::EventLog;
use crate::fx::Fx;
use crate::gfx::ImageBank;
use crate::hub::Hub;
use crate::mascot::Mascot;
use crate::pool::{SpawnPool, INCOMING_CAP};
use crate::provider::{Provider, ProviderConfig};
use crate::sessions::{CloseOutcome, Sessions};

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

    /// Construit l'app + pin agents.md dans le chat initial (si fourni),
    /// avec un message système d'annonce (lignes + ~tokens).
    pub fn from_config_with_agents(cfg: ProviderConfig, agents_md: Option<String>) -> Result<Self> {
        let provider = Provider::new(cfg)?;
        Ok(Self::with_provider(
            Some(Arc::new(provider)),
            agents_md,
            None,
            false,
            false,
        ))
    }

    /// Construit l'app, agents.md lu au premier envoi (`None` = lookup cwd).
    pub fn from_config_with_agents_file(
        cfg: ProviderConfig,
        agents_file: Option<String>,
    ) -> Result<Self> {
        let provider = Provider::new(cfg)?;
        Ok(Self::with_provider(
            Some(Arc::new(provider)),
            None,
            agents_file,
            true,
            false,
        ))
    }

    /// Construit l'app sans provider (`/connect` ensuite pour se connecter).
    #[allow(dead_code)]
    pub fn disconnected() -> Self {
        Self::disconnected_with_agents(None)
    }

    /// Construit l'app déconnectée + pin agents.md (si fourni).
    pub fn disconnected_with_agents(agents_md: Option<String>) -> Self {
        Self::with_provider(None, agents_md, None, false, true)
    }

    /// Construit l'app déconnectée, agents.md lu au premier envoi.
    pub fn disconnected_with_agents_file(agents_file: Option<String>) -> Self {
        Self::with_provider(None, None, agents_file, true, true)
    }

    /// Base commune : état + pin agents.md (+ hint `/connect` si déconnecté).
    /// `lazy` = contenu non fourni, lecture reportée au premier envoi.
    fn with_provider(
        provider: Option<Arc<Provider>>,
        agents_md: Option<String>,
        agents_file: Option<String>,
        lazy: bool,
        hint_connect: bool,
    ) -> Self {
        let (tx, rx) = mpsc::channel(INCOMING_CAP);
        let mut app = Self {
            mascot: Mascot::new(),
            fx: Fx::new(),
            bank: ImageBank::headless(),
            apparition: Apparition::new(),
            hub: Hub::new(),
            tick: 0,
            busy: false,
            ctx_used: 0,
            ctx_limit: 0,
            sessions: Sessions::new(),
            almanac: Almanac::memory(),
            bus: Bus::new(),
            provider,
            connect_flow: None,
            agents_md,
            agents_file,
            agents_tried: !lazy,
            screen: Screen::Chat,
            right_delta: 0,
            plus_btn: None,
            side_rows: None,
            close_btns: Vec::new(),
            tab_hits: Vec::new(),
            thinking: None,
            permissions: crate::permissions::load(),
            permissions_path: crate::permissions::path(),
            tools_on: true,
            tool_run: None,
            chats_path: None,
            events: EventLog::disabled(),
            tx,
            rx,
            pool: SpawnPool::new(),
        };
        app.pin_current();
        app.announce_agents_md();
        if hint_connect {
            app.push_system(String::from(NOT_CONNECTED));
        }
        app.refresh_ctx_cache();
        app
    }

    /// Lit agents.md + steering Kiro au premier envoi (une seule tentative),
    /// épingle les chats vides et annonce dans le chat courant. Absent = silencieux.
    pub(crate) fn ensure_agents_loaded(&mut self) {
        if self.agents_tried {
            return;
        }
        self.agents_tried = true;
        let merged = agents::load_merged(self.agents_file.as_deref());
        if merged.text.is_none() {
            return;
        }
        let msg = agents::merged_message(&merged);
        self.agents_md = merged.text;
        self.pin_all_empty();
        if let Some(msg) = msg {
            self.sessions.current_mut().chat.push(Role::System, msg);
        }
    }

    /// Ferme le chat `idx` (clic ×, Ctrl+W) : liens bus re-pointés,
    /// événement émis, refus motivé sinon (dernier, réponse en vol).
    pub(crate) fn close_chat(&mut self, idx: usize) {
        let name = self.sessions.get(idx).map(|c| c.name.clone());
        match self.sessions.close(idx) {
            CloseOutcome::Closed => {
                crate::sessions::shift_bus_links(&mut self.bus, idx);
                if let Some(name) = name {
                    self.events
                        .emit(&crate::events::Event::chat_closed(idx, &name));
                }
            }
            CloseOutcome::LastChat => {
                self.push_system(String::from("cannot close the last chat"));
            }
            CloseOutcome::Busy => {
                self.push_system(String::from("cannot close: a reply is in flight"));
            }
            CloseOutcome::Unknown => {
                self.push_system(format!("chat {} not found", idx + 1));
            }
        }
    }

    /// Nouveau chat + pin agents.md si chargé (Ctrl+N, `/new`).
    pub(crate) fn add_chat(&mut self) {
        self.sessions.add();
        self.pin_current();
        let at = self.sessions.index();
        let name = self.sessions.current().name.clone();
        self.events
            .emit(&crate::events::Event::chat_created(at, &name));
    }

    /// Épingle agents.md en tête du chat courant (si chargé).
    pub(super) fn pin_current(&mut self) {
        if let Some(content) = &self.agents_md {
            self.sessions.current_mut().pin_system(content);
        }
    }

    /// Épingle agents.md dans les chats vides non pinnés (premier envoi).
    fn pin_all_empty(&mut self) {
        if let Some(content) = self.agents_md.clone() {
            for i in 0..self.sessions.count() {
                let empty = self
                    .sessions
                    .get(i)
                    .is_some_and(|c| !c.pinned_system && c.history.is_empty());
                if empty {
                    self.sessions
                        .get_mut(i)
                        .expect("chat compté ci-dessus")
                        .pin_system(&content);
                }
            }
        }
    }

    /// Annonce le chargement agents.md dans le chat courant (si chargé).
    fn announce_agents_md(&mut self) {
        if let Some(content) = &self.agents_md {
            let msg = agents::loaded_message(content);
            self.sessions.current_mut().chat.push(Role::System, msg);
        }
    }

    /// App hors ligne (`--smoke`, `--apercu`) : aucune clé, aucun appel.
    pub fn offline() -> Result<Self> {
        Self::from_config(ProviderConfig {
            base_url: String::from("http://127.0.0.1:9"),
            api_key: String::new(),
            model: String::from("crypt-offline"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fichier temporaire unique par test (contenu écrit, à nettoyer).
    fn tmp_file(name: &str, content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("kh-lazy-{}-{name}", std::process::id()));
        std::fs::write(&path, content).unwrap();
        path
    }

    /// Texte du dernier message affiché du chat courant.
    fn last_text(app: &App) -> String {
        app.sessions
            .current()
            .chat
            .messages()
            .last()
            .map(|m| m.text.clone())
            .unwrap_or_default()
    }

    #[test]
    fn lazy_ne_lit_rien_avant_le_premier_envoi() {
        let path = tmp_file("avant", "contexte v1");
        let app = App::disconnected_with_agents_file(Some(path.to_string_lossy().into_owned()));
        assert!(app.agents_md.is_none());
        assert!(!app.sessions.current().pinned_system);
        assert!(app.sessions.current().history.is_empty());
        // Seul le hint /connect, pas d'annonce agents.md.
        assert_eq!(app.sessions.current().chat.messages().len(), 1);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn lazy_charge_epingle_et_annonce_une_seule_fois() {
        // Neutralise le steering Kiro réel (déterministe, voir KH_KIRO_*).
        let saved_project = std::env::var("KH_KIRO_PROJECT").ok();
        let saved_global = std::env::var("KH_KIRO_GLOBAL").ok();
        let empty = std::env::temp_dir().join(format!("kh-lazy-{}-empty", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        std::env::set_var("KH_KIRO_PROJECT", &empty);
        std::env::set_var("KH_KIRO_GLOBAL", &empty);
        let path = tmp_file("unique", "contexte v1");
        let mut app = App::disconnected_with_agents_file(Some(path.to_string_lossy().into_owned()));
        app.ensure_agents_loaded();
        assert_eq!(app.agents_md.as_deref(), Some("contexte v1"));
        assert!(app.sessions.current().pinned_system);
        assert_eq!(last_text(&app), agents::loaded_message("contexte v1"));
        // Le fichier change après : pas de relecture, contenu gardé.
        std::fs::write(&path, "contexte v2").unwrap();
        app.ensure_agents_loaded();
        assert_eq!(app.agents_md.as_deref(), Some("contexte v1"));
        // Nouveau chat après chargement : pinné comme avant.
        app.add_chat();
        assert!(app.sessions.current().pinned_system);
        std::fs::remove_file(&path).ok();
        match saved_project {
            Some(v) => std::env::set_var("KH_KIRO_PROJECT", v),
            None => std::env::remove_var("KH_KIRO_PROJECT"),
        }
        match saved_global {
            Some(v) => std::env::set_var("KH_KIRO_GLOBAL", v),
            None => std::env::remove_var("KH_KIRO_GLOBAL"),
        }
        std::fs::remove_dir_all(&empty).ok();
    }

    #[test]
    fn lazy_epingle_les_chats_vides_existants() {
        let path = tmp_file("multi", "contexte");
        let mut app = App::disconnected_with_agents_file(Some(path.to_string_lossy().into_owned()));
        app.add_chat();
        assert_eq!(app.sessions.count(), 2);
        app.ensure_agents_loaded();
        assert!(app.sessions.get(0).unwrap().pinned_system);
        assert!(app.sessions.get(1).unwrap().pinned_system);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn lazy_absent_sans_bruit_ni_reessai() {
        let path = std::env::temp_dir().join(format!("kh-lazy-{}-absent", std::process::id()));
        let mut app = App::disconnected_with_agents_file(Some(path.to_string_lossy().into_owned()));
        app.ensure_agents_loaded();
        assert!(app.agents_md.is_none());
        assert!(!app.sessions.current().pinned_system);
        assert_eq!(app.sessions.current().chat.messages().len(), 1);
        // Créé après la tentative : pas de relecture.
        std::fs::write(&path, "trop tard").unwrap();
        app.ensure_agents_loaded();
        assert!(app.agents_md.is_none());
        std::fs::remove_file(&path).ok();
    }
}
