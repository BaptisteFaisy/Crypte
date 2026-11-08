//! Compatibilité Kiro (`.kiro/`) : sous-ensemble BACK-TOOLS.
//!
//! Reprend du TUI (`src/kiro/mod.rs`) les résolutions de chemins et les
//! modules purs du contrat §6 : frontmatter, glob, ignore, skills,
//! memory, checkpoint, spec (+ prompts + run), mcp (+ client/conn/pool/
//! lists), hooks (+ match/exec/run), status, steering, powers (types
//! du refresh MCP). Hors contrat donc non portés : `agents_md`,
//! `agents_parse`, `cloud*`, `powers_cmds`, `workflow*`, `steer_cmds`,
//! `cmds`, `checkpoint_cmds`, `hook_cmds`, `mcp_cmds`, `memory_cmds`,
//! `skills_cmds`, `spec_cmds` (logique `App` → `cmd_kiro`/`spec_runner`).

pub mod checkpoint;
pub mod frontmatter;
pub mod glob;
pub mod hook_exec;
pub mod hook_match;
pub mod hook_run;
pub mod hooks;
pub mod ignore;
pub mod mcp;
pub mod mcp_client;
pub mod mcp_conn;
pub mod mcp_lists;
pub mod mcp_pool;
pub mod memory;
pub mod powers;
pub mod skills;
pub mod spec;
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
pub fn read_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}
