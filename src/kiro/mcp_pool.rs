//! Parc MCP : serveurs `enabled`, connexions paresseuses, cache d'outils.
//!
//! [`McpState`] (partagé, `Send`) vit dans [`App`](crate::app::App) :
//! connexion au premier besoin outillé, cache `mcp_<serveur>_<outil>`,
//! arrêt propre à la sortie. Un serveur en échec est noté et réessayé
//! seulement via `/mcp refresh` (jamais de respawn à chaque envoi).

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::Mutex;

use super::mcp::{self, Config, Server};
use super::mcp_client::{self, CallOutcome, McpTool};
use super::mcp_conn::Client;
use super::mcp_lists::{McpPrompt, McpResource, ResourceContent};
use crate::provider::truncate;
use crate::tools::exec::Outcome;
use crate::tools::wire::ToolDef;

/// État MCP partagé (`App` + tâches async) : verrou court, jamais gardé sur du LLM.
#[derive(Clone)]
pub struct McpState {
    inner: Arc<Mutex<McpPool>>,
}

impl McpState {
    /// Parc depuis la config fusionnée (projet + global, non connecté).
    pub fn new() -> Self {
        let project = super::project_dir();
        let global = super::global_dir();
        Self {
            inner: Arc::new(Mutex::new(McpPool::from_config(mcp::load_merged_in(
                &project,
                global.as_deref(),
            )))),
        }
    }

    /// Parc depuis une config explicite (tests : chemins injectés).
    #[cfg(test)]
    pub fn for_config(config: Config) -> Self {
        Self {
            inner: Arc::new(Mutex::new(McpPool::from_config(config))),
        }
    }

    /// Schémas natifs + MCP (connexion paresseuse au premier besoin).
    pub async fn tooled_defs(&self) -> Vec<ToolDef> {
        let mut all = crate::tools::defs();
        let mut pool = self.inner.lock().await;
        pool.ensure_connected().await;
        all.extend(pool.defs());
        all
    }

    /// Exécute un outil `mcp_*` (échecs = `ok: false`, journalisés).
    pub async fn call(&self, chat: usize, name: &str, args: &str) -> Outcome {
        let started = std::time::Instant::now();
        let outcome = self.call_inner(name, args).await;
        let ms = started.elapsed().as_millis().min(9_999_999) as u64;
        crate::tools::log::append(
            chat,
            name,
            outcome.ok,
            ms,
            args.chars().count(),
            outcome.text.chars().count(),
        );
        outcome
    }

    /// Exécution pure (sans journal) : args parsés, appel, erreurs en texte.
    async fn call_inner(&self, name: &str, args: &str) -> Outcome {
        let parsed: Value = match serde_json::from_str(args) {
            Ok(value) => value,
            Err(_) => {
                return Outcome {
                    ok: false,
                    text: format!("invalid JSON arguments for {name}"),
                };
            }
        };
        let mut pool = self.inner.lock().await;
        match pool.call(name, parsed).await {
            Ok(done) => Outcome {
                ok: done.ok,
                text: done.text,
            },
            Err(err) => Outcome {
                ok: false,
                text: format!("{err:#}"),
            },
        }
    }

    /// Vrai si l'outil est pré-approuvé (défaut : non, donc confirmation).
    pub fn is_auto(&self, def_name: &str) -> bool {
        if !mcp_client::is_mcp_name(def_name) {
            return false;
        }
        match self.inner.try_lock() {
            Ok(pool) => pool.is_auto(def_name),
            Err(_) => false,
        }
    }

    /// Statut `/mcp` (cache seul, jamais bloquant).
    pub fn status(&self) -> String {
        match self.inner.try_lock() {
            Ok(pool) => pool.status(),
            Err(_) => String::from("mcp: busy, try again"),
        }
    }

    /// Reconnecte tout : arrêt + relecture config + connexion + statut.
    pub async fn refresh(&self) -> String {
        let project = super::project_dir();
        let global = super::global_dir();
        self.refresh_with(mcp::load_merged_in(&project, global.as_deref()))
            .await
    }

    /// Reconnecte sur une config explicite (cœur testable, sans lecture fs).
    async fn refresh_with(&self, fresh: Config) -> String {
        let mut pool = self.inner.lock().await;
        pool.shutdown().await;
        *pool = McpPool::from_config(fresh);
        pool.ensure_connected().await;
        pool.status()
    }

    /// Arrête tous les clients (sortie : stdio tués, HTTP oubliés).
    pub async fn shutdown(&self) {
        self.inner.lock().await.shutdown().await;
    }

    /// Resources par serveur (connexion paresseuse, erreur par serveur).
    pub async fn resources(&self) -> Vec<(String, Result<Vec<McpResource>, String>)> {
        let mut pool = self.inner.lock().await;
        pool.query_all("resources/list", json!({}))
            .await
            .into_iter()
            .map(|(name, result)| {
                let parsed = result.and_then(|value| {
                    super::mcp_lists::parse_resources(&value).map_err(|err| format!("{err:#}"))
                });
                (name, parsed)
            })
            .collect()
    }

    /// Prompts par serveur (connexion paresseuse, erreur par serveur).
    pub async fn prompts(&self) -> Vec<(String, Result<Vec<McpPrompt>, String>)> {
        let mut pool = self.inner.lock().await;
        pool.query_all("prompts/list", json!({}))
            .await
            .into_iter()
            .map(|(name, result)| {
                let parsed = result.and_then(|value| {
                    super::mcp_lists::parse_prompts(&value).map_err(|err| format!("{err:#}"))
                });
                (name, parsed)
            })
            .collect()
    }

    /// Lit une resource (`serveur`, `uri`) : contenu texte brut.
    pub async fn read(&self, server: &str, uri: &str) -> Result<ResourceContent, String> {
        let mut pool = self.inner.lock().await;
        pool.ensure_connected().await;
        let Some(idx) = pool.find_server(server) else {
            return Err(format!("unknown MCP server: {server}"));
        };
        pool.read_one(idx, uri).await
    }

    /// Rend un prompt (`serveur`, `nom`, args JSON) : messages en texte.
    pub async fn prompt(&self, server: &str, name: &str, args: Value) -> Result<String, String> {
        let mut pool = self.inner.lock().await;
        pool.ensure_connected().await;
        let Some(idx) = pool.find_server(server) else {
            return Err(format!("unknown MCP server: {server}"));
        };
        pool.prompt_one(idx, name, args).await
    }
}

/// Serveur suivi : config + client + outils + erreur éventuelle.
struct Tracked {
    name: String,
    server: Server,
    client: Option<Client>,
    tools: Vec<McpTool>,
    error: Option<String>,
}

/// Parc : serveurs `enabled` + cache des définitions exposées.
struct McpPool {
    servers: Vec<Tracked>,
}

impl McpPool {
    /// Parc depuis une config (`enabled` seuls, triés par nom).
    fn from_config(config: Config) -> Self {
        let servers = mcp::enabled(&config)
            .into_iter()
            .map(|(name, server)| Tracked {
                name: name.clone(),
                server: server.clone(),
                client: None,
                tools: Vec::new(),
                error: None,
            })
            .collect();
        Self { servers }
    }

    /// Connecte les serveurs manquants (échecs notés, voir `refresh`).
    async fn ensure_connected(&mut self) {
        for idx in 0..self.servers.len() {
            if self.servers[idx].client.is_some() || self.servers[idx].error.is_some() {
                continue;
            }
            self.connect_one(idx).await;
        }
    }

    /// Connecte + `tools/list` (échec noté, client jeté, outils gardés si lus).
    async fn connect_one(&mut self, idx: usize) {
        let name = self.servers[idx].name.clone();
        let server = self.servers[idx].server.clone();
        match Client::connect(&name, &server).await {
            Err(err) => self.servers[idx].error = Some(format!("{err:#}")),
            Ok(mut client) => match client.list_tools(&name).await {
                Ok(tools) => {
                    self.servers[idx].tools = tools;
                    self.servers[idx].client = Some(client);
                }
                Err(err) => {
                    self.servers[idx].error = Some(format!("{err:#}"));
                    client.shutdown().await;
                }
            },
        }
    }

    /// Définitions exposées au modèle (cache `mcp_<serveur>_<outil>`).
    fn defs(&self) -> Vec<ToolDef> {
        let mut out = Vec::new();
        for tracked in &self.servers {
            for tool in &tracked.tools {
                out.push(mcp_client::to_tool_def(&tracked.name, tool));
            }
        }
        out
    }

    /// Index du serveur + nom brut de l'outil (`None` si inconnu).
    fn find(&self, def_name: &str) -> Option<(usize, String)> {
        for (idx, tracked) in self.servers.iter().enumerate() {
            for tool in &tracked.tools {
                if mcp_client::def_name(&tracked.name, &tool.name) == def_name {
                    return Some((idx, tool.name.clone()));
                }
            }
        }
        None
    }

    /// Exécute `def_name` (serveur connecté exigé, sinon erreur claire).
    async fn call(&mut self, def_name: &str, args: Value) -> Result<CallOutcome, anyhow::Error> {
        let found = self.find(def_name);
        let Some((idx, tool)) = found else {
            anyhow::bail!("unknown MCP tool: {def_name}");
        };
        let tracked = &mut self.servers[idx];
        let name = tracked.name.clone();
        match tracked.client.as_mut() {
            Some(client) => client.call(&name, &tool, args).await,
            None => anyhow::bail!("{}", not_connected(tracked)),
        }
    }

    /// Vrai si l'outil est listé dans le `autoApprove` de son serveur.
    fn is_auto(&self, def_name: &str) -> bool {
        match self.find(def_name) {
            Some((idx, tool)) => {
                let tracked = &self.servers[idx];
                mcp_client::is_auto_approved(&tracked.server.auto_approve, &tracked.name, &tool)
            }
            None => false,
        }
    }

    /// Texte `/mcp` : serveurs (transport, état, outils) + total.
    fn status(&self) -> String {
        if self.servers.is_empty() {
            return String::from("mcp: no servers configured (.kiro/settings/mcp.json)");
        }
        let total: usize = self.servers.iter().map(|tracked| tracked.tools.len()).sum();
        let servers = self.servers.len();
        let plural = |count: usize| if count > 1 { "s" } else { "" };
        let mut out = format!(
            "mcp: {servers} server{ps}, {total} tool{pt}",
            ps = plural(servers),
            pt = plural(total),
        );
        for tracked in &self.servers {
            out.push_str(&format!("\n  {} ({})", tracked.name, server_state(tracked)));
        }
        out
    }

    /// Index d'un serveur par nom (`None` = inconnu ou désactivé).
    fn find_server(&self, name: &str) -> Option<usize> {
        self.servers.iter().position(|tracked| tracked.name == name)
    }

    /// Requête brute sur chaque serveur (erreur par serveur).
    async fn query_all(
        &mut self,
        method: &str,
        params: Value,
    ) -> Vec<(String, Result<Value, String>)> {
        self.ensure_connected().await;
        let mut out = Vec::new();
        for idx in 0..self.servers.len() {
            let name = self.servers[idx].name.clone();
            let tracked = &mut self.servers[idx];
            let result = match tracked.client.as_mut() {
                Some(client) => client
                    .request(&name, method, params.clone())
                    .await
                    .map_err(|err| format!("{err:#}")),
                None => Err(tracked
                    .error
                    .clone()
                    .unwrap_or_else(|| String::from("not connected"))),
            };
            out.push((name, result));
        }
        out
    }

    /// Lit une resource sur un serveur (connecté exigé, sinon clair).
    async fn read_one(&mut self, idx: usize, uri: &str) -> Result<ResourceContent, String> {
        let name = self.servers[idx].name.clone();
        let tracked = &mut self.servers[idx];
        match tracked.client.as_mut() {
            Some(client) => {
                let result = client
                    .request(&name, "resources/read", json!({"uri": uri}))
                    .await
                    .map_err(|err| format!("{err:#}"))?;
                Ok(super::mcp_lists::render_resource(uri, &result))
            }
            None => Err(not_connected(tracked)),
        }
    }

    /// Rend un prompt sur un serveur (connecté exigé, sinon clair).
    async fn prompt_one(&mut self, idx: usize, name: &str, args: Value) -> Result<String, String> {
        let server = self.servers[idx].name.clone();
        let tracked = &mut self.servers[idx];
        match tracked.client.as_mut() {
            Some(client) => {
                let params = json!({"name": name, "arguments": args});
                let result = client
                    .request(&server, "prompts/get", params)
                    .await
                    .map_err(|err| format!("{err:#}"))?;
                Ok(super::mcp_lists::render_prompt(&result))
            }
            None => Err(not_connected(tracked)),
        }
    }

    /// Arrête les clients (outils oubliés, erreurs effacées, reconnectable).
    async fn shutdown(&mut self) {
        for tracked in &mut self.servers {
            if let Some(client) = tracked.client.take() {
                client.shutdown().await;
            }
            tracked.tools.clear();
            tracked.error = None;
        }
    }
}

/// `MCP server 'x' not connected…` (cause notée si connue).
fn not_connected(tracked: &Tracked) -> String {
    match &tracked.error {
        Some(err) => format!("MCP server '{}' not connected: {err}", tracked.name),
        None => format!("MCP server '{}' not connected", tracked.name),
    }
}

/// `stdio, connected, 3 tools` (ou `remote, error: …`, `not connected`).
fn server_state(tracked: &Tracked) -> String {
    let transport = if tracked.server.command.is_some() {
        "stdio"
    } else {
        "remote"
    };
    if let Some(err) = &tracked.error {
        return format!("{transport}, error: {}", truncate(err, 120));
    }
    match tracked.client {
        Some(_) => format!("{transport}, connected, {} tools", tracked.tools.len()),
        None => format!("{transport}, not connected"),
    }
}

#[cfg(test)]
mod tests;
