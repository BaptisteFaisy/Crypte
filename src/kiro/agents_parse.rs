//! Parsing des agents customs Kiro (`.json` + `.md`, schéma officiel).
//!
//! Champs identiques dans les deux formats (`name`, `description`,
//! `prompt`, `model`, `tools`, `excludedTools`, `mcpServers`) ; en
//! Markdown le corps est le prompt système. Champs inconnus ignorés
//! (tolérant au harnais unifié : `resources`, `permissions`,
//! `welcomeMessage`, ...). `system` est un alias legacy Crypte de
//! `prompt` (`prompt` prime). Listes Markdown en flow (`[a, "b"]`)
//! ou scalaire unique ; maps imbriquées ignorées (mettre `mcpServers`
//! dans le `.json`).

use serde::Deserialize;
use std::collections::HashMap;

/// Config agent brute (JSON direct, frontmatter converti).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawAgent {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub excluded_tools: Vec<String>,
    #[serde(default)]
    pub mcp_servers: HashMap<String, crate::kiro::mcp::Server>,
    #[serde(default)]
    pub system: Option<String>,
}

/// Parse un `.json` (`stem` = nom sans extension, défaut du `name`).
/// `None` si invalide (fichier ignoré).
pub fn parse_json(stem: &str, raw: &str) -> Option<crate::kiro::agents_md::CustomAgent> {
    let parsed: RawAgent = serde_json::from_str(raw).ok()?;
    Some(build(stem, &parsed, String::new()))
}

/// Parse un `.md` (frontmatter + corps = prompt système).
pub fn parse_md(stem: &str, raw: &str) -> crate::kiro::agents_md::CustomAgent {
    let (front, body) = crate::kiro::frontmatter::split(raw);
    let mut parsed = RawAgent::default();
    if let Some(front) = front {
        parsed.name = crate::kiro::frontmatter::value(front, "name");
        parsed.description = crate::kiro::frontmatter::value(front, "description");
        parsed.prompt = crate::kiro::frontmatter::value(front, "prompt");
        parsed.model = crate::kiro::frontmatter::value(front, "model");
        parsed.tools =
            flow_list(&crate::kiro::frontmatter::value(front, "tools").unwrap_or_default());
        parsed.excluded_tools =
            flow_list(&crate::kiro::frontmatter::value(front, "excludedTools").unwrap_or_default());
        parsed.system = crate::kiro::frontmatter::value(front, "system");
    }
    build(stem, &parsed, body.to_string())
}

/// Construit l'agent : `name` explicite non vide sinon `stem` ; tête =
/// `prompt` (canonique) sinon `system` (legacy), préfixée au corps md.
fn build(stem: &str, parsed: &RawAgent, body: String) -> crate::kiro::agents_md::CustomAgent {
    let name = parsed
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(stem);
    let head = parsed
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|head| !head.is_empty())
        .or_else(|| {
            parsed
                .system
                .as_deref()
                .map(str::trim)
                .filter(|head| !head.is_empty())
        });
    let system_prompt = match head {
        Some(head) if body.trim().is_empty() => head.to_string(),
        Some(head) => format!("{head}\n\n{body}"),
        None => body,
    };
    crate::kiro::agents_md::CustomAgent {
        name: name.to_string(),
        description: parsed.description.clone().unwrap_or_default(),
        system_prompt,
        model: parsed
            .model
            .clone()
            .filter(|model| !model.trim().is_empty()),
        tools: parsed.tools.clone(),
        excluded_tools: parsed.excluded_tools.clone(),
        mcp_servers: parsed.mcp_servers.clone(),
    }
}

/// Liste flow `[a, "b", 'c']` (scalaire non vide = 1 élément).
pub fn flow_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if let Some(inner) = trimmed
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        return inner
            .split(',')
            .map(|item| crate::kiro::frontmatter::unquote(item.trim()))
            .filter(|item| !item.is_empty())
            .collect();
    }
    vec![crate::kiro::frontmatter::unquote(trimmed)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_complet_champs_connus_et_inconnus_ignores() {
        let agent = parse_json(
            "dev",
            r#"{"description": "Codes.",
                "prompt": "You code.", "model": "claude-x", "tools": ["read", "shell"],
                "excludedTools": ["web"], "mcpServers": {"pg": {"command": "mcp-pg", "timeout": 999}},
                "welcomeMessage": "hi", "resources": ["skill://x"]}"#,
        )
        .unwrap();
        assert_eq!(agent.name, "dev");
        assert_eq!(agent.description, "Codes.");
        assert_eq!(agent.system_prompt, "You code.");
        assert_eq!(agent.model.as_deref(), Some("claude-x"));
        assert_eq!(agent.tools, vec!["read", "shell"]);
        assert_eq!(agent.excluded_tools, vec!["web"]);
        assert_eq!(agent.mcp_servers["pg"].command.as_deref(), Some("mcp-pg"));
        assert!(parse_json("x", "{oops").is_none());
        let named = parse_json("stem", r#"{"name": "  ", "prompt": "p"}"#).unwrap();
        assert_eq!(named.name, "stem");
    }

    #[test]
    fn md_prompt_prime_system_alias_legacy() {
        let agent = parse_md("r", "---\nname: rev\nprompt: Be brief.\n---\nCheck it.");
        assert_eq!(agent.system_prompt, "Be brief.\n\nCheck it.");
        let agent = parse_md("r", "---\nname: rev\nsystem: Be brief.\n---\nCheck it.");
        assert_eq!(agent.system_prompt, "Be brief.\n\nCheck it.");
        let agent = parse_md("r", "---\nprompt: P\nsystem: S\n---\nBody.");
        assert_eq!(agent.system_prompt, "P\n\nBody.");
        let agent = parse_md("brut", "# Corps seul.");
        assert_eq!(agent.name, "brut");
        assert_eq!(agent.system_prompt, "# Corps seul.");
        assert!(agent.model.is_none());
    }

    #[test]
    fn md_model_et_listes_flow() {
        let agent = parse_md(
            "d",
            "---\nmodel: sonnet\ntools: [\"read\", 'shell']\nexcludedTools: web\n---\nB",
        );
        assert_eq!(agent.model.as_deref(), Some("sonnet"));
        assert_eq!(agent.tools, vec!["read", "shell"]);
        assert_eq!(agent.excluded_tools, vec!["web"]);
    }

    #[test]
    fn flow_list_cas_limites() {
        assert!(flow_list("").is_empty());
        assert!(flow_list("[]").is_empty());
        assert_eq!(flow_list("[a, ]"), vec!["a"]);
        assert_eq!(flow_list("shell"), vec!["shell"]);
        assert_eq!(flow_list("[a"), vec!["[a"]);
    }
}
