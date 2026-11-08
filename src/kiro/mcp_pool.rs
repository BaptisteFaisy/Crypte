//! Parc MCP : serveurs `enabled`, connexions paresseuses, cache d'outils.
//!
//! [`McpState`] (partagé, `Send`) vit dans [`App`](crate::app::App) :
//! connexion au premier besoin outillé, cache `mcp_<serveur>_<outil>`,
//! arrêt propre à la sortie. Un serveur en échec est noté et réessayé
//! seulement via `/mcp refresh` (jamais de respawn à chaque envoi).

use std::sync::Arc;

use serde_json::Value;
use tokio::sync::Mutex;

use super::mcp::{self, Config, Server};
use super::mcp_client::{self, CallOutcome, McpTool};
use super::mcp_conn::Client;
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
            None => match &tracked.error {
                Some(err) => anyhow::bail!("MCP server '{name}' not connected: {err}"),
                None => anyhow::bail!("MCP server '{name}' not connected"),
            },
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
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Config à deux serveurs (`old` désactivé, donc exclu).
    fn cfg_two() -> Config {
        let live = Server {
            command: Some(String::from("kh-mcp-no-such-bin")),
            auto_approve: vec![String::from("search")],
            ..Default::default()
        };
        let dead = Server {
            disabled: true,
            ..Default::default()
        };
        Config {
            mcp_servers: HashMap::from([(String::from("gh"), live), (String::from("old"), dead)]),
        }
    }

    /// Config vide (aucun serveur).
    fn cfg_empty() -> Config {
        Config {
            mcp_servers: HashMap::new(),
        }
    }

    #[test]
    fn vide_sans_config() {
        let state = McpState::for_config(cfg_empty());
        assert!(state.status().contains("no servers"), "{}", state.status());
        assert!(!state.is_auto("mcp_gh_search"));
        assert!(!state.is_auto("shell_run"));
    }

    #[tokio::test]
    async fn defs_natifs_seuls_et_appel_inconnu() {
        let state = McpState::for_config(cfg_empty());
        assert_eq!(state.tooled_defs().await.len(), 14);
        let out = state.call(0, "mcp_gh_search", "{}").await;
        assert!(!out.ok);
        assert!(out.text.contains("unknown MCP tool"), "{}", out.text);
        let bad = state.call(0, "mcp_gh_search", "nope").await;
        assert!(!bad.ok);
        assert!(bad.text.contains("invalid JSON"), "{}", bad.text);
    }

    #[test]
    fn statut_liste_enabled_seuls() {
        let state = McpState::for_config(cfg_two());
        let text = state.status();
        assert!(text.contains("1 server"), "{text}");
        assert!(text.contains("gh (stdio, not connected)"), "{text}");
        assert!(!text.contains("old"), "{text}");
    }

    #[tokio::test]
    async fn refresh_reconnecte_et_arret_propre() {
        let state = McpState::for_config(cfg_empty());
        state.shutdown().await;
        let text = state.refresh_with(cfg_two()).await;
        assert!(text.contains("1 server"), "{text}");
        assert!(text.contains("gh (stdio, error: "), "{text}");
        let back = state.refresh_with(cfg_empty()).await;
        assert!(back.contains("no servers"), "{back}");
    }
}
