//! Handler `/web` (extrait de `cmds.rs`, limite 500 lignes).
//!
//! `impl App` déporté : lit/écrit l'état via les accès `pub(crate)` de [`App`].

use crate::app::{App, Incoming};

impl App {
    /// `/web <query>` : web search + LLM synthesis with cited sources.
    /// Async (`Incoming::Web`) ; works disconnected too (raw results).
    pub(crate) fn cmd_web(&mut self, query: String) {
        let id = self.sessions.index();
        self.sessions.current_mut().waiting = true;
        let provider = self.provider.clone();
        let model = self.effective_model();
        let effort = self.thinking.clone();
        let tx = self.tx.clone();
        self.push_system(format!("web search: {query}…"));
        self.pool.spawn(async move {
            let reply =
                crate::web::answer(query, provider.as_deref(), &model, effort.as_deref()).await;
            let _ = tx.send(Incoming::Web(id, reply)).await;
        });
    }
}
