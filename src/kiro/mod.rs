//! Compatibilité Kiro (`.kiro/`) : steering, specs, hooks, MCP, ignore.
//!
//! Crypte lit les mêmes fichiers que Kiro (projet `./.kiro/` + global
//! `~/.kiro/`) : un projet va et vient entre les deux. Le steering
//! `always` rejoint le contexte pinné (voir [`crate::agents`]), le reste
//! alimente `/kiro` en attendant T2+ (specs, hooks, MCP).
//!
//! Tests déterministes : `KH_KIRO_PROJECT` / `KH_KIRO_GLOBAL` remplacent
//! cwd / `~/.kiro` quand elles sont non vides.

pub mod cmds;
pub mod glob;
pub mod hook_cmds;
pub mod hook_exec;
pub mod hook_match;
pub mod hook_run;
pub mod hooks;
pub mod ignore;
pub mod mcp;
pub mod mcp_client;
pub mod mcp_cmds;
pub mod mcp_conn;
pub mod mcp_pool;
pub mod spec;
pub mod spec_cmds;
pub mod spec_prompts;
pub mod spec_run;
pub mod status;
pub mod steering;

use std::path::PathBuf;

/// Dossier projet (`KH_KIRO_PROJECT` prime, sinon cwd, sinon `.`).
pub fn project_dir() -> PathBuf {
    if let Some(dir) = read_env("KH_KIRO_PROJECT") {
        return PathBuf::from(dir);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Dossier Kiro global (`KH_KIRO_GLOBAL` prime, sinon `~/.kiro` ; en
/// test, `None` par défaut — jamais le home réel, hermétisme).
pub fn global_dir() -> Option<PathBuf> {
    if let Some(dir) = read_env("KH_KIRO_GLOBAL") {
        return Some(PathBuf::from(dir));
    }
    if cfg!(test) {
        return None;
    }
    home_dir().map(|home| home.join(".kiro"))
}

/// Dossier home (`None` si `HOME` absent ou blanc).
pub fn home_dir() -> Option<PathBuf> {
    read_env("HOME").map(PathBuf::from)
}

/// Lit une variable d'environnement, `None` si absente ou blanche.
fn read_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}
