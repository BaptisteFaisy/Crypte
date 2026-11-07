//! Flow de connexion interactive (`/connect openai|muse`).
//!
//! `impl App` déporté hors `app.rs` (limite 500 lignes) : handlers
//! `/connect` (statut, store Muse, flow clavier), état du flow (clé →
//! modèle), touches dédiées et installation du provider en fin de flow.
//!
//! La clé API transite uniquement en mémoire (buffer puis config) : saisie
//! masquée en `•`, jamais affichée, logguée ni poussée dans le chat — seuls
//! le provider et le modèle apparaissent dans les confirmations.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;
use crate::provider::{connect_defaults, provider_label, Provider, ProviderConfig};

/// Message quand un envoi exige une connexion absente.
pub(crate) const NOT_CONNECTED: &str = "not connected: /connect openai|muse";

/// Étape du flow de connexion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectStep {
    Key,
    Model,
}

/// État du flow : provider visé, clé saisie, buffer de l'étape courante.
pub(crate) struct ConnectFlow {
    step: ConnectStep,
    provider: String,
    key: String,
    buffer: String,
}

impl ConnectFlow {
    /// Flow neuf à l'étape clé pour ce provider (`openai` ou `muse`).
    fn new(provider: String) -> Self {
        Self {
            step: ConnectStep::Key,
            provider,
            key: String::new(),
            buffer: String::new(),
        }
    }
}

impl App {
    /// `/connect [openai|muse]` : statut (sans arg), store Muse ou flow clavier.
    /// Inconnu → erreur + valeurs (validation dans `start_connect_flow`).
    pub(crate) fn cmd_connect(&mut self, arg: Option<String>) {
        match arg {
            None => self.cmd_connect_status(),
            Some(name) if name == "muse" => self.cmd_connect_muse(),
            Some(name) => self.start_connect_flow(&name),
        }
    }

    /// `/connect` sans arg : provider + modèle, ou non connecté + usage.
    fn cmd_connect_status(&mut self) {
        match self.provider.as_ref() {
            Some(provider) => {
                let cfg = provider.config();
                self.push_system(format!(
                    "connected to {} ({})\nusage: /connect openai|muse",
                    provider_label(&cfg.base_url),
                    cfg.model
                ));
            }
            None => self.push_system(String::from(NOT_CONNECTED)),
        }
    }

    /// `/connect muse` : store Muse si lisible, sinon saisie manuelle.
    fn cmd_connect_muse(&mut self) {
        let from_store = ProviderConfig::muse_auth_path().and_then(ProviderConfig::from_muse_auth);
        match from_store {
            Ok(cfg) => self.apply_connected(cfg, "muse"),
            Err(_) => {
                self.push_system(String::from("Muse store unreadable, manual input"));
                self.start_connect_flow("muse");
            }
        }
    }

    /// Démarre le flow (étape clé, saisie masquée).
    /// Provider inconnu → erreur système + valeurs valides, sans flow.
    pub(crate) fn start_connect_flow(&mut self, provider: &str) {
        if connect_defaults(provider).is_none() {
            self.push_system(format!(
                "unknown provider: {provider} — values: openai, muse"
            ));
            return;
        }
        self.connect_flow = Some(ConnectFlow::new(provider.to_string()));
        self.sync_flow_prompt();
    }

    /// Touches pendant le flow : saisissables → buffer, Entrée avance,
    /// Échap annule (le reste — F5, Ctrl+N… — est ignoré, pas de quit).
    pub(crate) fn on_flow_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return true;
        }
        match key.code {
            KeyCode::Esc => self.cmd(|s| s.cancel_flow()),
            KeyCode::Enter => self.cmd(|s| s.advance_flow()),
            KeyCode::Backspace => self.cmd(|s| s.flow_pop()),
            KeyCode::Char(c) => self.cmd(|s| s.flow_push(c)),
            _ => true,
        }
    }

    /// Installe le provider + confirme (`connecté à {p} ({m})`).
    /// Échec (construction client) → erreur système, flow inchangé.
    pub(crate) fn apply_connected(&mut self, cfg: ProviderConfig, name: &str) {
        let model = cfg.model.clone();
        match Provider::new(cfg) {
            Ok(provider) => {
                self.provider = Some(Arc::new(provider));
                self.connect_flow = None;
                self.restore_prompt();
                self.push_system(format!("connected to {name} ({model})"));
            }
            Err(err) => self.push_system(format!("connection failed: {err:#}")),
        }
    }

    /// Ajoute un caractère au buffer du flow + resynchronise le prompt.
    fn flow_push(&mut self, c: char) {
        if let Some(flow) = self.connect_flow.as_mut() {
            flow.buffer.push(c);
        }
        self.sync_flow_prompt();
    }

    /// Efface le dernier caractère du buffer + resynchronise le prompt.
    fn flow_pop(&mut self) {
        if let Some(flow) = self.connect_flow.as_mut() {
            flow.buffer.pop();
        }
        self.sync_flow_prompt();
    }

    /// Buffer du flow trimmé (vide si pas de flow).
    fn flow_trimmed(&self) -> String {
        self.connect_flow
            .as_ref()
            .map(|f| f.buffer.trim().to_string())
            .unwrap_or_default()
    }

    /// Prompt de saisie selon l'étape (+ aperçu = buffer, brouillon intact).
    /// Clé → `clé API {p} (masquée)` masqué ; modèle → défaut explicite.
    fn sync_flow_prompt(&mut self) {
        let Some(flow) = self.connect_flow.as_ref() else {
            return;
        };
        let defaults = connect_defaults(&flow.provider).map(|(_, m)| m);
        let (label, mask) = match flow.step {
            ConnectStep::Key => (format!("API key {} (masked)", flow.provider), true),
            ConnectStep::Model => (
                format!("model (Enter = {})", defaults.unwrap_or("?")),
                false,
            ),
        };
        let preview = flow.buffer.clone();
        let chat = &mut self.sessions.current_mut().chat;
        chat.set_prompt(Some(label), mask);
        chat.set_input_preview(Some(preview));
    }

    /// Prompt de saisie par défaut (fin ou annulation de flow).
    fn restore_prompt(&mut self) {
        let chat = &mut self.sessions.current_mut().chat;
        chat.set_prompt(None, false);
        chat.set_input_preview(None);
    }

    /// Annule le flow : prompt restauré + `connexion annulée`.
    fn cancel_flow(&mut self) {
        self.connect_flow = None;
        self.restore_prompt();
        self.push_system(String::from("connection cancelled"));
    }

    /// Entrée pendant le flow : clé → modèle, modèle → connexion.
    fn advance_flow(&mut self) {
        let step = self.connect_flow.as_ref().map(|f| f.step);
        match step {
            Some(ConnectStep::Key) => self.advance_key(),
            Some(ConnectStep::Model) => self.advance_model(),
            None => {}
        }
    }

    /// Fin de l'étape clé : vide → reste + rappel, sinon étape modèle.
    fn advance_key(&mut self) {
        let key = self.flow_trimmed();
        if key.is_empty() {
            self.push_system(String::from("empty key, type the key (Esc = cancel)"));
            return;
        }
        if let Some(flow) = self.connect_flow.as_mut() {
            flow.key = key;
            flow.buffer.clear();
            flow.step = ConnectStep::Model;
        }
        self.sync_flow_prompt();
    }

    /// Fin de l'étape modèle : provider (modèle saisi ou défaut du store).
    fn advance_model(&mut self) {
        let Some(flow) = self.connect_flow.as_ref() else {
            return;
        };
        let name = flow.provider.clone();
        let key = flow.key.clone();
        let typed = flow.buffer.trim().to_string();
        let Some((base, default_model)) = connect_defaults(&name) else {
            self.cancel_flow();
            return;
        };
        let model = if typed.is_empty() {
            default_model.to_string()
        } else {
            typed
        };
        let cfg = ProviderConfig {
            base_url: base.to_string(),
            api_key: key,
            model,
        };
        self.apply_connected(cfg, &name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_key_then_empty_model_connects_with_defaults() {
        let mut app = App::disconnected();
        app.start_connect_flow("openai");
        assert!(app.connect_flow.is_some());
        for c in "sk-test".chars() {
            app.flow_push(c);
        }
        assert!(app.sessions.current().chat.input.is_empty());
        app.advance_flow();
        assert_eq!(
            app.connect_flow.as_ref().map(|f| f.step),
            Some(ConnectStep::Model)
        );
        app.advance_flow();
        assert!(app.connect_flow.is_none());
        let cfg = app.provider.as_ref().unwrap().config();
        assert_eq!(cfg.base_url, "https://api.openai.com/v1");
        assert_eq!(cfg.model, "gpt-4o");
    }

    #[test]
    fn flow_empty_key_stays_and_unknown_provider_errors() {
        let mut app = App::disconnected();
        app.start_connect_flow("muse");
        app.advance_flow();
        assert_eq!(
            app.connect_flow.as_ref().map(|f| f.step),
            Some(ConnectStep::Key)
        );
        assert!(app.provider.is_none());
        app.cancel_flow();
        assert!(app.connect_flow.is_none());
        app.start_connect_flow("zzz");
        assert!(app.connect_flow.is_none());
        assert!(app.provider.is_none());
    }
}
