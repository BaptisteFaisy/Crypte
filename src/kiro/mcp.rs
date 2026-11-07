//! Config MCP (`settings/mcp.json`, projet + global).
//!
//! Serveurs stdio (`command`) ou distants (`url`) ; `disabled` et
//! `autoApprove` piloteront (T4) le client MCP branché sur `tools::flow`.
//! `cwd`/`headers` ignorés en T1 (relus par le client T4).

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Un serveur MCP (stdio et/ou distant, champs optionnels).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub auto_approve: Vec<String>,
}

/// `{"mcpServers": {nom: serveur}}`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub mcp_servers: HashMap<String, Server>,
}

/// Charge un `mcp.json` (`None` si absent ou invalide).
pub fn load(path: &Path) -> Option<Config> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Fusionne global + projet (le projet prime par nom).
pub fn merge(global: Option<Config>, project: Option<Config>) -> Config {
    let mut servers = global.map(|config| config.mcp_servers).unwrap_or_default();
    if let Some(config) = project {
        servers.extend(config.mcp_servers);
    }
    Config {
        mcp_servers: servers,
    }
}

/// Charge et fusionne global + projet (chemins injectés, tests).
pub fn load_merged_in(project: &Path, global: Option<&Path>) -> Config {
    let global = global.and_then(|dir| load(&dir.join("settings/mcp.json")));
    let project = load(&project.join(".kiro/settings/mcp.json"));
    merge(global, project)
}

/// Serveurs actifs triés par nom (`disabled` exclus).
pub fn enabled(config: &Config) -> Vec<(&String, &Server)> {
    let mut out: Vec<_> = config
        .mcp_servers
        .iter()
        .filter(|(_, server)| !server.disabled)
        .collect();
    out.sort_by(|a, b| a.0.cmp(b.0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("kh-kiro-mcp-{}-{name}", std::process::id()))
    }

    #[test]
    fn fusion_projet_prime_et_disabled_exclu() {
        let dir = tmp_dir("merge");
        let (global, project) = (dir.join("home/.kiro"), dir.join("proj"));
        std::fs::create_dir_all(global.join("settings")).unwrap();
        std::fs::create_dir_all(project.join(".kiro/settings")).unwrap();
        std::fs::write(
            global.join("settings/mcp.json"),
            r#"{"mcpServers":{
              "github":{"command":"gh-mcp","autoApprove":["search"]},
              "old":{"command":"old","disabled":true}}}"#,
        )
        .unwrap();
        std::fs::write(
            project.join(".kiro/settings/mcp.json"),
            r#"{"mcpServers":{"github":{"url":"https://mcp.local/github"}}}"#,
        )
        .unwrap();
        let config = load_merged_in(&project, Some(&global));
        let live = enabled(&config);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].0, "github");
        assert_eq!(live[0].1.url.as_deref(), Some("https://mcp.local/github"));
        assert!(load(&dir.join("absent.json")).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
