//! Handlers `/mcp` (statut + listes + lecture, `impl App` déporté).
//!
//! `/mcp` affiche le cache (serveurs + outils, jamais bloquant) ;
//! `refresh`/`resources`/`prompts`/`read`/`prompt` interrogent en
//! tâche de fond (réponse via `Incoming::McpText`). Un prompt lu
//! est armé dans la saisie (marqueur interne `PROMPT_MARK`,
//! transitoire comme `/skill`).

use crate::app::{App, Incoming};

/// Aide `/mcp` (aussi sur sous-commande inconnue).
const MCP_USAGE: &str = "usage: /mcp [refresh|resources|prompts] | read <server> <uri> | prompt <server> <name> [k=v ...]";
/// Marqueur interne : le texte livré arme la saisie (prompt lu).
const PROMPT_MARK: &str = "\u{0}mcp-prompt\n";

impl App {
    /// `/mcp ...` : statut, `refresh`, listes, `read`, `prompt`.
    pub(crate) fn cmd_mcp(&mut self, args: Vec<String>) {
        let mut words = args.iter().map(String::as_str);
        match words.next() {
            None => self.push_system(self.mcp.status()),
            Some("refresh") => {
                self.mcp_query("reconnecting", |mcp| async move { mcp.refresh().await })
            }
            Some("resources") => self.mcp_query("listing resources", |mcp| async move {
                render_listed("resources", &mcp.resources().await, |item| {
                    let mut line = format!("{} — {}", item.uri, item.name);
                    if let Some(mime) = &item.mime {
                        line.push_str(&format!(" ({mime})"));
                    }
                    line
                })
            }),
            Some("prompts") => self.mcp_query("listing prompts", |mcp| async move {
                render_listed("prompts", &mcp.prompts().await, |item| {
                    let mut line = format!("{} — {}", item.name, item.description);
                    if !item.arguments.is_empty() {
                        let args = item
                            .arguments
                            .iter()
                            .map(|arg| {
                                if arg.required {
                                    format!("{}*", arg.name)
                                } else {
                                    arg.name.clone()
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        line.push_str(&format!(" ({args})"));
                    }
                    line
                })
            }),
            Some("read") => match (words.next(), words.collect::<Vec<_>>().join(" ")) {
                (Some(server), uri) if !uri.trim().is_empty() => {
                    let server = server.to_string();
                    self.mcp_query("reading", |mcp| async move {
                        match mcp.read(&server, uri.trim()).await {
                            Ok(content) => {
                                let mime = content.mime.as_deref().unwrap_or("?");
                                let text = crate::provider::truncate(&content.text, 2000);
                                format!("{} ({mime}):\n{text}", content.uri)
                            }
                            Err(err) => format!("mcp read: {err}"),
                        }
                    });
                }
                _ => self.push_system(String::from("usage: /mcp read <server> <uri>")),
            },
            Some("prompt") => match words.next() {
                Some(server) => match words.next() {
                    Some(name) => self.mcp_prompt(server, name, words.collect()),
                    None => self
                        .push_system(String::from("usage: /mcp prompt <server> <name> [k=v ...]")),
                },
                None => {
                    self.push_system(String::from("usage: /mcp prompt <server> <name> [k=v ...]"))
                }
            },
            Some("help") => self.push_system(String::from(MCP_USAGE)),
            Some(other) => self.push_system(format!("unknown /mcp {other} — {MCP_USAGE}")),
        }
    }

    /// `/mcp prompt` : args `k=v`, rendu armé dans la saisie.
    fn mcp_prompt(&mut self, server: &str, name: &str, rest: Vec<&str>) {
        let (inputs, malformed) = crate::kiro::workflow_cmds::split_inputs(&rest);
        if let Some(bad) = malformed {
            self.push_system(format!("mcp prompt: bad input '{bad}' (want key=value)"));
            return;
        }
        let server = server.to_string();
        let name = name.to_string();
        let args = serde_json::to_value(&inputs).unwrap_or(serde_json::Value::Null);
        self.mcp_query(&format!("prompt {name}"), |mcp| async move {
            match mcp.prompt(&server, &name, args).await {
                Ok(text) => format!("{PROMPT_MARK}{text}"),
                Err(err) => format!("mcp prompt: {err}"),
            }
        });
    }

    /// Requête async (`ack` immédiat, texte via `Incoming::McpText`).
    fn mcp_query<F, Fut>(&mut self, doing: &str, run: F)
    where
        F: FnOnce(crate::kiro::mcp_pool::McpState) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = String> + Send + 'static,
    {
        let mcp = self.mcp.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let text = run(mcp).await;
            let _ = tx.send(Incoming::McpText(text)).await;
        });
        self.push_system(format!("mcp: {doing}..."));
    }

    /// Texte MCP async : prompt lu → saisie armée, sinon affiché.
    pub(crate) fn drain_mcp_text(&mut self, text: String) {
        if let Some(body) = text.strip_prefix(PROMPT_MARK) {
            let draft = std::mem::take(&mut self.sessions.current_mut().chat.input);
            let armed = if draft.trim().is_empty() {
                body.to_string()
            } else {
                format!("{body}\n\n{draft}")
            };
            self.sessions.current_mut().chat.input = armed;
            self.push_system(String::from("mcp prompt armed — send to use (transient)"));
        } else {
            self.push_system(text);
        }
    }
}

/// Rend une liste par serveur (`resources`/`prompts`, erreurs incluses).
fn render_listed<T>(
    what: &str,
    per_server: &[(String, Result<Vec<T>, String>)],
    line: impl Fn(&T) -> String,
) -> String {
    if per_server.is_empty() {
        return String::from("mcp: no servers configured (.kiro/settings/mcp.json)");
    }
    let mut out = Vec::new();
    for (server, result) in per_server {
        match result {
            Ok(items) if items.is_empty() => out.push(format!("{server}: no {what}")),
            Ok(items) => {
                out.push(format!("{server}:"));
                for item in items {
                    out.push(format!("  {}", line(item)));
                }
            }
            Err(err) => out.push(format!("{server}: {err}")),
        }
    }
    out.join("\n")
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

    /// Tout le chat courant (asserts `contains` multi-lignes).
    fn all_text(app: &App) -> String {
        app.sessions
            .get(0)
            .unwrap()
            .chat
            .messages()
            .iter()
            .map(|msg| msg.text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// App branchée sur le stub MCP partagé (voir `mcp_lists`).
    fn stub_app(tag: &str) -> (App, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("kh-mcp-cmds-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let stub = crate::kiro::mcp_lists::write_stub(&dir, tag);
        let mut app = test_app();
        app.mcp = McpState::for_config(Config {
            mcp_servers: HashMap::from([(
                String::from("stub"),
                Server {
                    command: Some(stub.to_string_lossy().into_owned()),
                    ..Default::default()
                },
            )]),
        });
        (app, dir)
    }

    /// Pompe un texte MCP async jusqu'au premier livré (300 ms max).
    async fn pump_once(app: &mut App) -> bool {
        match tokio::time::timeout(std::time::Duration::from_millis(300), app.rx.recv()).await {
            Ok(Some(Incoming::McpText(text))) => {
                app.drain_mcp_text(text);
                true
            }
            _ => false,
        }
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
    fn mcp_sous_commandes_usages() {
        let mut app = test_app();
        app.run_command("/mcp nope");
        assert!(
            last_text(&app).contains("unknown /mcp nope"),
            "{}",
            last_text(&app)
        );
        app.run_command("/mcp read");
        assert!(
            last_text(&app).contains("usage: /mcp read"),
            "{}",
            last_text(&app)
        );
        app.run_command("/mcp prompt stub");
        assert!(
            last_text(&app).contains("usage: /mcp prompt"),
            "{}",
            last_text(&app)
        );
        app.run_command("/mcp prompt stub revue oops");
        assert!(
            last_text(&app).contains("bad input 'oops'"),
            "{}",
            last_text(&app)
        );
    }

    /// Listes via stub : resource + prompt schématisés affichés.
    #[tokio::test]
    async fn mcp_listes_stub() {
        let (mut app, dir) = stub_app("listes");
        app.run_command("/mcp resources");
        assert!(pump_once(&mut app).await);
        assert!(
            all_text(&app).contains("stub://doc — doc (text/plain)"),
            "{}",
            all_text(&app)
        );
        app.run_command("/mcp prompts");
        assert!(pump_once(&mut app).await);
        assert!(
            all_text(&app).contains("revue — Relit. (fichier*)"),
            "{}",
            all_text(&app)
        );
        app.mcp.shutdown().await;
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Lecture affichée, prompt armé dans la saisie (transitoire).
    #[tokio::test]
    async fn mcp_read_affiche_prompt_arme() {
        let (mut app, dir) = stub_app("lire");
        app.run_command("/mcp read stub stub://doc");
        assert!(pump_once(&mut app).await);
        assert!(all_text(&app).contains("hello stub"), "{}", all_text(&app));
        app.run_command("/mcp prompt stub revue fichier=a");
        assert!(pump_once(&mut app).await);
        assert!(all_text(&app).contains("armed"), "{}", all_text(&app));
        let input = app.sessions.get(0).unwrap().chat.input.clone();
        assert!(input.starts_with("user: Relis."), "{input}");
        app.mcp.shutdown().await;
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Marqueur prompt : saisie armée ; texte nu : affiché.
    #[test]
    fn drain_texte_marqué_ou_nu() {
        let mut app = test_app();
        app.drain_mcp_text(format!("{PROMPT_MARK}user: Hi."));
        assert!(last_text(&app).contains("armed"), "{}", last_text(&app));
        assert!(app
            .sessions
            .get(0)
            .unwrap()
            .chat
            .input
            .starts_with("user: Hi."));
        app.drain_mcp_text(String::from("mcp: 1 server"));
        assert_eq!(last_text(&app), "mcp: 1 server");
    }
}
