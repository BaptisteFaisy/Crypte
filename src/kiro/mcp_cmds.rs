//! Handlers `/mcp` (statut + `refresh`, `impl App` déporté).
//!
//! `/mcp` affiche le cache (serveurs + outils, jamais bloquant) ;
//! `/mcp refresh` reconnecte en tâche de fond (réponse via `Incoming`).

use crate::app::{App, Incoming};

impl App {
    /// `/mcp [refresh]` : statut cache, ou reconnexion async (`refresh`).
    pub(crate) fn cmd_mcp(&mut self, arg: Option<String>) {
        match arg.as_deref() {
            None => self.push_system(self.mcp.status()),
            Some("refresh") => {
                let mcp = self.mcp.clone();
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let text = mcp.refresh().await;
                    let _ = tx.send(Incoming::McpRefresh(text)).await;
                });
                self.push_system(String::from("mcp: reconnecting..."));
            }
            Some(other) => self.push_system(format!("invalid: {other} — values: refresh")),
        }
    }

    /// Fin de `/mcp refresh` : affiche le statut reconnecté.
    pub(crate) fn drain_mcp_refresh(&mut self, text: String) {
        self.push_system(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kiro::mcp::{Config, Server};
    use crate::kiro::mcp_pool::McpState;
    use std::collections::HashMap;

    /// App de test (hermétique sans env : global None, hooks no-op).
    fn test_app() -> App {
        let cfg = crate::provider::ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        App::from_config(cfg).unwrap()
    }

    /// Dernière ligne affichée du chat courant.
    fn last_text(app: &App) -> String {
        app.sessions
            .get(0)
            .unwrap()
            .chat
            .messages()
            .last()
            .map(|msg| msg.text.clone())
            .unwrap_or_default()
    }

    #[test]
    fn mcp_affiche_statut_cache() {
        let mut app = test_app();
        let live = Server {
            command: Some(String::from("gh-mcp")),
            ..Default::default()
        };
        app.mcp = McpState::for_config(Config {
            mcp_servers: HashMap::from([(String::from("gh"), live)]),
        });
        app.run_command("/mcp");
        let text = last_text(&app);
        assert!(text.contains("1 server"), "{text}");
        assert!(text.contains("gh (stdio, not connected)"), "{text}");
    }

    #[test]
    fn mcp_arg_inconnu_aide() {
        let mut app = test_app();
        app.run_command("/mcp nope");
        let text = last_text(&app);
        assert!(text.contains("invalid: nope"), "{text}");
        assert!(text.contains("refresh"), "{text}");
    }

    #[test]
    fn drain_refresh_affiche_statut() {
        let mut app = test_app();
        app.drain_mcp_refresh(String::from("mcp: 1 server, 2 tools"));
        assert_eq!(last_text(&app), "mcp: 1 server, 2 tools");
    }
}
