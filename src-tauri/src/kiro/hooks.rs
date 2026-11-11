//! Hooks Kiro (`.kiro/hooks/*.json`, format v1, projet + global).
//!
//! Déclencheurs en PascalCase, inconnus → `Unknown` (tolérant aux
//! évolutions Kiro). Moteur d'exécution = T3.

use serde::Deserialize;
use std::path::Path;

/// Déclencheur (noms Kiro v1, `Unknown` = inconnu toléré).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Trigger {
    PromptSubmit,
    AgentStop,
    SessionStart,
    AgentSpawn,
    PreToolUse,
    PostToolUse,
    PostFileCreate,
    PostFileSave,
    PostFileDelete,
    PreTaskExecution,
    PostTaskExecution,
    #[serde(other)]
    Unknown,
}

impl Trigger {
    /// Nom Kiro (statut `/kiro`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PromptSubmit => "PromptSubmit",
            Self::AgentStop => "AgentStop",
            Self::SessionStart => "SessionStart",
            Self::AgentSpawn => "AgentSpawn",
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::PostFileCreate => "PostFileCreate",
            Self::PostFileSave => "PostFileSave",
            Self::PostFileDelete => "PostFileDelete",
            Self::PreTaskExecution => "PreTaskExecution",
            Self::PostTaskExecution => "PostTaskExecution",
            Self::Unknown => "Unknown",
        }
    }
}

/// Action (`command` shell ou `agent` prompt ; autres = opaques).
#[derive(Debug, Clone, Deserialize)]
pub struct Action {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
}

/// Un hook nommé (fichier v1).
#[derive(Debug, Clone, Deserialize)]
pub struct Hook {
    #[serde(default)]
    pub name: String,
    pub trigger: Trigger,
    #[serde(default)]
    pub matcher: Option<String>,
    pub action: Action,
}

/// Fichier `{"version": "v1", "hooks": […]}`.
#[derive(Debug, Clone, Deserialize)]
pub struct HookFile {
    #[serde(default)]
    pub hooks: Vec<Hook>,
}

/// Hooks d'un fichier + son nom.
#[derive(Debug, Clone)]
pub struct NamedHooks {
    pub file: String,
    pub hooks: Vec<Hook>,
}

/// Charge un dossier (`.json` triés, invalides ignorés).
pub fn load_dir(dir: &Path) -> Vec<NamedHooks> {
    let mut paths = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            paths.push(path);
        }
    }
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let raw = std::fs::read_to_string(&path).ok()?;
            let parsed: HookFile = serde_json::from_str(&raw).ok()?;
            let file = path.file_name()?.to_string_lossy().into_owned();
            Some(NamedHooks {
                file,
                hooks: parsed.hooks,
            })
        })
        .collect()
}

/// Charge global puis projet (chemins injectés, tests).
pub fn load_all_in(project: &Path, global: Option<&Path>) -> Vec<NamedHooks> {
    let mut out = Vec::new();
    if let Some(global) = global {
        out.extend(load_dir(&global.join("hooks")));
    }
    out.extend(load_dir(&project.join(".kiro/hooks")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("kh-kiro-hooks-{}-{name}", std::process::id()))
    }

    #[test]
    fn charge_command_et_agent_ignore_l_invalide() {
        let dir = tmp_dir("load");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("verify.json"),
            r#"{"version":"v1","hooks":[
              {"name":"fmt","trigger":"PostFileSave","matcher":"\\.rs$",
               "action":{"type":"command","command":"cargo fmt"}},
              {"name":"review","trigger":"PromptSubmit",
               "action":{"type":"agent","prompt":"relis le diff"}}]}"#,
        )
        .unwrap();
        std::fs::write(dir.join("broken.json"), "{oops").unwrap();
        let all = load_dir(&dir);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].file, "verify.json");
        assert_eq!(all[0].hooks.len(), 2);
        assert_eq!(all[0].hooks[0].trigger, Trigger::PostFileSave);
        assert_eq!(all[0].hooks[0].action.command.as_deref(), Some("cargo fmt"));
        assert_eq!(all[0].hooks[1].trigger.as_str(), "PromptSubmit");
        assert!(load_dir(&dir.join("absent")).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn declencheur_inconnu_tolerant() {
        let file: HookFile = serde_json::from_str(
            r#"{"hooks":[{"name":"futur","trigger":"QuantumLeap","action":{"type":"command"}}]}"#,
        )
        .unwrap();
        assert_eq!(file.hooks[0].trigger, Trigger::Unknown);
        assert_eq!(file.hooks[0].trigger.as_str(), "Unknown");
    }
}
