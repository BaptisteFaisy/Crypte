//! Listes MCP hors outils : resources + prompts.
//!
//! Types + parsers tolérants (champs manquants = défauts, comme
//! `parse_tools`) servis par le transport brut `Client::request`.
//! Contenus binaires (`blob`) = mention, jamais injectés.

use serde_json::Value;

/// Resource listée (`resources/list`, champs optionnels tolérés).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpResource {
    /// URI opaque (`file://…`, `db://…`, …).
    pub uri: String,
    /// Nom imposé par la spec (vide accepté).
    pub name: String,
    /// Titre d'affichage, si fourni.
    pub title: Option<String>,
    /// Type MIME, si fourni.
    pub mime: Option<String>,
}

/// Prompt listé (`prompts/list`, arguments schématisés).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpPrompt {
    /// Nom requis par `prompts/get`.
    pub name: String,
    /// Description affichée par `/mcp prompts`.
    pub description: String,
    /// Arguments (`name`, `required`).
    pub arguments: Vec<PromptArg>,
}

/// Argument de prompt (`required` faux par défaut).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptArg {
    pub name: String,
    pub required: bool,
}

/// Contenu lu (`resources/read`, texte ; `blob` = mention).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceContent {
    /// URI relue (écho de la demande).
    pub uri: String,
    /// Type MIME, si fourni.
    pub mime: Option<String>,
    /// Texte joint (blocs `text` ; `blob` = `[binary …]`).
    pub text: String,
}

/// Parse `resources/list` (`resources[]`, tout optionnel sauf `uri`).
pub fn parse_resources(result: &Value) -> Result<Vec<McpResource>, anyhow::Error> {
    let mut out = Vec::new();
    let empty = Vec::new();
    let list = result
        .get("resources")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    for item in list {
        let Some(uri) = item.get("uri").and_then(Value::as_str) else {
            continue;
        };
        out.push(McpResource {
            uri: uri.to_string(),
            name: item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            title: item
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_string),
            mime: item
                .get("mimeType")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }
    Ok(out)
}

/// Rend `resources/read` (`contents[]`, `text` joint, `blob` mentionné).
pub fn render_resource(uri: &str, result: &Value) -> ResourceContent {
    let empty = Vec::new();
    let contents = result
        .get("contents")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let mut texts = Vec::new();
    let mut mime = None;
    for item in contents {
        if mime.is_none() {
            mime = item
                .get("mimeType")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if let Some(text) = item.get("text").and_then(Value::as_str) {
            texts.push(text.to_string());
        } else if item.get("blob").and_then(Value::as_str).is_some() {
            texts.push(String::from("[binary content, not shown]"));
        }
    }
    ResourceContent {
        uri: uri.to_string(),
        mime,
        text: texts.join("\n"),
    }
}

/// Parse `prompts/list` (`prompts[]`, `name` requis, reste optionnel).
pub fn parse_prompts(result: &Value) -> Result<Vec<McpPrompt>, anyhow::Error> {
    let mut out = Vec::new();
    let empty = Vec::new();
    let list = result
        .get("prompts")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    for item in list {
        let Some(name) = item.get("name").and_then(Value::as_str) else {
            continue;
        };
        let args = item
            .get("arguments")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        out.push(McpPrompt {
            name: name.to_string(),
            description: item
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            arguments: args
                .iter()
                .filter_map(|arg| {
                    arg.get("name")
                        .and_then(Value::as_str)
                        .map(|name| PromptArg {
                            name: name.to_string(),
                            required: arg
                                .get("required")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                        })
                })
                .collect(),
        });
    }
    Ok(out)
}

/// Rend `prompts/get` (`messages[].content.text`, un par `role:`).
pub fn render_prompt(result: &Value) -> String {
    let empty = Vec::new();
    let messages = result
        .get("messages")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    messages
        .iter()
        .map(|msg| {
            let role = msg.get("role").and_then(Value::as_str).unwrap_or("?");
            let text = msg
                .get("content")
                .and_then(|content| content.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("");
            format!("{role}: {text}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Stub MCP stdio partagé (tests e2e hermétiques, `pool` + `cmds`).
#[cfg(test)]
pub fn stub_script() -> &'static str {
    r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  [ -z "$id" ] && continue
  method=$(printf '%s' "$line" | sed -n 's/.*"method":"\([^"]*\)".*/\1/p')
  case "$method" in
    initialize) result='{"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"stub"}}' ;;
    tools/list) result='{"tools":[]}' ;;
    resources/list) result='{"resources":[{"uri":"stub://doc","name":"doc","mimeType":"text/plain"}]}' ;;
    resources/read) result='{"contents":[{"uri":"stub://doc","mimeType":"text/plain","text":"hello stub"}]}' ;;
    prompts/list) result='{"prompts":[{"name":"revue","description":"Relit.","arguments":[{"name":"fichier","required":true}]}]}' ;;
    prompts/get) result='{"messages":[{"role":"user","content":{"type":"text","text":"Relis."}}]}' ;;
    *) result='{}' ;;
  esac
  printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$id" "$result"
done
"#
}

/// Écrit + rend exécutable le stub (nom unique par appel : nanos +
/// compteur — `ETXTBSY` si deux exécutions partagent un binaire).
#[cfg(test)]
pub fn write_stub(dir: &std::path::Path, tag: &str) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let path = dir.join(format!("mcp-stub-{tag}-{nanos}-{unique}.sh"));
    std::fs::write(&path, stub_script()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `parse_resources` : optionnels tolérés, sans `uri` ignoré.
    #[test]
    fn parse_resources_tolerants() {
        let value: Value = serde_json::from_str(
            r#"{"resources": [
                {"uri": "db://a", "name": "a", "title": "A", "mimeType": "text/plain"},
                {"uri": "db://b"},
                {"name": "sans-uri"}]}"#,
        )
        .unwrap();
        let out = parse_resources(&value).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title.as_deref(), Some("A"));
        assert_eq!(out[0].mime.as_deref(), Some("text/plain"));
        assert!(out[1].title.is_none());
    }

    /// `render_resource` : textes joints, `blob` mentionné, mime gardé.
    #[test]
    fn render_resource_texte_et_blob() {
        let value: Value = serde_json::from_str(
            r#"{"contents": [
                {"uri": "db://a", "mimeType": "text/plain", "text": "un"},
                {"uri": "db://a", "blob": "aGk="}]}"#,
        )
        .unwrap();
        let out = render_resource("db://a", &value);
        assert_eq!(out.uri, "db://a");
        assert_eq!(out.mime.as_deref(), Some("text/plain"));
        assert!(out.text.contains("un"), "{}", out.text);
        assert!(
            out.text.contains("[binary content, not shown]"),
            "{}",
            out.text
        );
    }

    /// `parse_prompts` : arguments schématisés, sans `name` ignoré.
    #[test]
    fn parse_prompts_arguments() {
        let value: Value = serde_json::from_str(
            r#"{"prompts": [
                {"name": "revue", "description": "Relit.", "arguments": [
                    {"name": "fichier", "required": true}, {"name": "niveau"}]},
                {"description": "sans-nom"}]}"#,
        )
        .unwrap();
        let out = parse_prompts(&value).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].arguments.len(), 2);
        assert!(out[0].arguments[0].required);
        assert!(!out[0].arguments[1].required);
    }

    /// `render_prompt` : un `role: texte` par message.
    #[test]
    fn render_prompt_messages() {
        let value: Value = serde_json::from_str(
            r#"{"messages": [
                {"role": "user", "content": {"type": "text", "text": "Relis."}},
                {"role": "assistant", "content": {"type": "text", "text": "OK."}}]}"#,
        )
        .unwrap();
        assert_eq!(render_prompt(&value), "user: Relis.\nassistant: OK.");
        assert!(render_prompt(&Value::Null).is_empty());
    }
}
