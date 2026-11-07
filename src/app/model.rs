//! Réglages du prochain appel : modèle effectif + thinking affiché.
//!
//! `impl App` déporté hors `app.rs` (limite 500 lignes) : purs accès
//! aux champs, sans import au-delà de [`App`].

use super::App;

impl App {
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
}
