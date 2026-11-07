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
pub mod hooks;
pub mod ignore;
pub mod mcp;
pub mod spec;
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

/// Dossier Kiro global (`KH_KIRO_GLOBAL` prime, sinon `~/.kiro`).
pub fn global_dir() -> Option<PathBuf> {
    if let Some(dir) = read_env("KH_KIRO_GLOBAL") {
        return Some(PathBuf::from(dir));
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
