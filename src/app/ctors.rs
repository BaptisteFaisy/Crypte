//! Constructeurs de [`App`] (config env/TOML, déconnecté, hors ligne)
//! + pin agents.md dans le chat initial.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc;

use super::{App, Screen};
use crate::agents;
use crate::apparition::Apparition;
use crate::bus::Bus;
use crate::chat::Role;
use crate::connect::NOT_CONNECTED;
use crate::fx::Fx;
use crate::galerie::Galerie;
use crate::gfx::ImageBank;
use crate::hub::Hub;
use crate::mascot::Mascot;
use crate::provider::{Provider, ProviderConfig};
use crate::sessions::Sessions;

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
            ctx_used: 0,
            ctx_limit: 0,
            sessions: Sessions::new(),
            bus: Bus::new(),
            provider,
            connect_flow: None,
            agents_md,
            screen: Screen::Chat,
            plus_btn: None,
            thinking: None,
            permissions: crate::permissions::load(),
            permissions_path: crate::permissions::path(),
            tx,
            rx,
        };
        app.pin_current();
        app.announce_agents_md();
        if hint_connect {
            app.push_system(String::from(NOT_CONNECTED));
        }
        app.refresh_ctx_cache();
        app
    }

    /// Épingle agents.md en tête du chat courant (si chargé).
    pub(super) fn pin_current(&mut self) {
        if let Some(content) = &self.agents_md {
            self.sessions.current_mut().pin_system(content);
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
