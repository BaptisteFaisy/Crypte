//! Client MCP : protocole JSON-RPC (`initialize`, `tools/list`, `tools/call`).
//!
//! [`Proto`] tourne sur toute paire (`AsyncRead`, `AsyncWrite`) : `duplex`
//! dans les tests, `ChildStd{out,in}` en stdio (`mcp_conn`). Tools
//! uniquement (resources/prompts hors scope T4).

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use crate::provider::truncate;
use crate::tools::wire::ToolDef;

/// Version de protocole annoncée (`initialize`, négociée côté serveur).
pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// Nom de client annoncé (`clientInfo.name`).
const CLIENT_NAME: &str = "kiro-horror-tui";
/// Timeout du handshake (spawn + `initialize` + accusé).
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Timeout d'une requête (`tools/list`, `tools/call`, HTTP inclus).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Messages entrelacés sautés avant abandon (notifs, requêtes serveur).
const MAX_SKIP: usize = 32;

/// Outil MCP distant (`tools/list`) : nom + doc + schéma d'entrée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Résultat d'appel (`tools/call`) : succès + texte rendu au modèle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallOutcome {
    pub ok: bool,
    pub text: String,
}

/// Canal JSON-RPC : requêtes numérotées, réponses corrélées par `id`.
pub struct Proto<R, W> {
    reader: BufReader<R>,
    writer: W,
    next_id: u64,
}

impl<R, W> Proto<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    /// Canal sur une paire lecture/écriture (stdio, duplex…).
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: BufReader::new(reader),
            writer,
            next_id: 1,
        }
    }

    /// Requête + `result` (erreur JSON-RPC = `Err`, timeout explicite).
    pub async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let raw = serde_json::to_string(&request_value(id, method, params))?;
        self.writer.write_all(raw.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        tokio::time::timeout(REQUEST_TIMEOUT, self.read_result(id, method))
            .await
            .map_err(|_| anyhow::anyhow!("MCP {method} timed out"))?
    }

    /// Notification (sans `id`, aucune réponse attendue).
    pub async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        let note = json!({"jsonrpc": "2.0", "method": method, "params": params});
        let raw = serde_json::to_string(&note)?;
        self.writer.write_all(raw.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// Réponse `id` (notifs et lignes étrangères sautées, erreurs contextées).
    async fn read_result(&mut self, id: u64, method: &str) -> Result<Value> {
        for _ in 0..=MAX_SKIP {
            let mut line = String::new();
            let read = self
                .reader
                .read_line(&mut line)
                .await
                .context("reading MCP response")?;
            if read == 0 {
                anyhow::bail!("MCP server closed the stream during {method}");
            }
            let msg: Value = serde_json::from_str(&line).with_context(|| {
                format!(
                    "parsing MCP response to {method}: {}",
                    truncate(line.trim(), 200)
                )
            })?;
            if msg.get("id") != Some(&json!(id)) {
                continue;
            }
            if let Some(err) = msg.get("error") {
                anyhow::bail!("MCP {method} failed: {}", error_text(err));
            }
            return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
        }
        anyhow::bail!("too many interleaved messages during {method}");
    }
}

/// Handshake : `initialize` (timeout court) + `notifications/initialized`.
pub async fn handshake<R, W>(proto: &mut Proto<R, W>) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let _ = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        proto.request("initialize", init_params()),
    )
    .await
    .map_err(|_| anyhow::anyhow!("MCP handshake timed out"))?
    .context("MCP handshake failed")?;
    proto.notify("notifications/initialized", json!({})).await
}

/// `tools/list` → outils distants (champs manquants = défauts).
pub async fn list_tools<R, W>(proto: &mut Proto<R, W>) -> Result<Vec<McpTool>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    parse_tools(&proto.request("tools/list", json!({})).await?)
}

/// `tools/call` → texte rendu (`isError` = échec, jamais d'exception).
pub async fn call_tool<R, W>(
    proto: &mut Proto<R, W>,
    name: &str,
    args: Value,
) -> Result<CallOutcome>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let params = json!({"name": name, "arguments": args});
    let result = proto
        .request("tools/call", params)
        .await
        .with_context(|| format!("calling MCP tool {name}"))?;
    Ok(render_result(&result))
}

/// Requête JSON-RPC (`id` numérique, réutilisée par le transport HTTP).
pub fn request_value(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

/// Params `initialize` (version + capacités outils + identité, stdio + HTTP).
pub fn init_params() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {"tools": {}},
        "clientInfo": {"name": CLIENT_NAME, "version": env!("CARGO_PKG_VERSION")},
    })
}

/// Outils depuis `tools/list` (sans nom = ignoré, doc/schéma = défauts).
pub fn parse_tools(result: &Value) -> Result<Vec<McpTool>> {
    let mut out = Vec::new();
    let empty = Vec::new();
    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    for tool in tools {
        let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        out.push(McpTool {
            name: name.to_string(),
            description: tool
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            input_schema: tool
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({"type": "object"})),
        });
    }
    Ok(out)
}

/// Rend `tools/call` : textes joints, `isError` → échec, non-texte =
/// placeholder (jamais de bytes au modèle), sortie tronquée (`MAX_OUTPUT`).
pub fn render_result(result: &Value) -> CallOutcome {
    let ok = result.get("isError").and_then(Value::as_bool) != Some(true);
    let mut texts: Vec<String> = Vec::new();
    let mut other = 0usize;
    if let Some(parts) = result.get("content").and_then(Value::as_array) {
        for part in parts {
            match part.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        texts.push(text.to_string());
                    }
                }
                _ => other += 1,
            }
        }
    }
    if other > 0 {
        texts.push(format!("[{other} non-text content part(s)]"));
    }
    CallOutcome {
        ok,
        text: truncate(&texts.join("\n"), crate::tools::MAX_OUTPUT),
    }
}

/// `message (code N)` depuis une erreur JSON-RPC (stdio + HTTP).
pub fn error_text(err: &Value) -> String {
    let msg = err
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("unknown error");
    match err.get("code").and_then(Value::as_i64) {
        Some(code) => format!("{msg} (code {code})"),
        None => msg.to_string(),
    }
}

/// Nom exposé au modèle : `mcp_<serveur>_<outil>` (RÈGLE T4 : préfixe
/// fixe + fragments assainis ; collision = dernier serveur gagné).
pub fn def_name(server: &str, tool: &str) -> String {
    format!("mcp_{}_{}", sanitize(server), sanitize(tool))
}

/// Fragment `[A-Za-z0-9_-]` (le reste → `_`, vide → `_`).
pub(crate) fn sanitize(fragment: &str) -> String {
    let clean: String = fragment
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if clean.is_empty() {
        String::from("_")
    } else {
        clean
    }
}

/// Vrai si le nom vient du pont MCP (préfixe `mcp_`, natifs = jamais).
pub fn is_mcp_name(name: &str) -> bool {
    name.starts_with("mcp_")
}

/// Schéma OpenAI d'un outil MCP (nom préfixé, serveur rappelé en doc).
pub fn to_tool_def(server: &str, tool: &McpTool) -> ToolDef {
    let about = if tool.description.is_empty() {
        format!("MCP tool {} from server {server}", tool.name)
    } else {
        format!("{} (MCP server {server})", tool.description)
    };
    ToolDef::new(
        &def_name(server, &tool.name),
        &about,
        tool.input_schema.clone(),
    )
}

/// Vrai si l'outil est pré-approuvé (`autoApprove` = nom brut ou préfixé).
pub fn is_auto_approved(auto_approve: &[String], server: &str, tool: &str) -> bool {
    auto_approve
        .iter()
        .any(|entry| entry == tool || entry == &def_name(server, tool))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tokio::io::{
        duplex, split, AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf,
    };

    /// Canal client sur paire `duplex` (l'autre bout = faux serveur).
    fn channel(pair: DuplexStream) -> Proto<ReadHalf<DuplexStream>, WriteHalf<DuplexStream>> {
        let (reader, writer) = split(pair);
        Proto::new(reader, writer)
    }

    /// Faux serveur MCP (réponses fixes, `fail_list` = `tools/list` en erreur).
    async fn serve_fake(pair: DuplexStream, fail_list: bool) {
        let (reader, mut writer) = split(pair);
        let mut lines = BufReader::new(reader);
        loop {
            let mut line = String::new();
            if lines.read_line(&mut line).await.unwrap_or(0) == 0 {
                break;
            }
            let msg: Value = match serde_json::from_str(&line) {
                Ok(msg) => msg,
                Err(_) => continue,
            };
            let Some(id) = msg.get("id").cloned() else {
                continue;
            };
            let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
            let reply = fake_reply(id, method, &msg, fail_list);
            let mut raw = serde_json::to_string(&reply).unwrap();
            raw.push('\n');
            if writer.write_all(raw.as_bytes()).await.is_err() {
                break;
            }
        }
    }

    /// Réponse fixe du faux serveur (outils `search` + `nodoc`, `boom` = erreur).
    fn fake_reply(id: Value, method: &str, msg: &Value, fail_list: bool) -> Value {
        let body = match method {
            "initialize" => json!({"result": {"protocolVersion": "9"}}),
            "tools/list" if fail_list => {
                json!({"error": {"code": -32601, "message": "no tools here"}})
            }
            "tools/list" => json!({"result": {"tools": [
                {"name": "search", "description": "web search",
                 "inputSchema": {"type": "object", "properties": {"q": {"type": "string"}}}},
                {"name": "nodoc"},
            ]}}),
            "tools/call" => {
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                if name == "boom" {
                    json!({"result": {"content": [{"type": "text", "text": "kaboom"}],
                                        "isError": true}})
                } else {
                    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
                    json!({"result": {"content": [{"type": "text",
                        "text": format!("called {name} {args}")}]}})
                }
            }
            _ => json!({"error": {"code": -32601, "message": "unknown method"}}),
        };
        let mut resp = json!({"jsonrpc": "2.0", "id": id});
        resp.as_object_mut()
            .unwrap()
            .extend(body.as_object().unwrap().clone());
        resp
    }

    #[tokio::test]
    async fn protocole_complet_sur_duplex() {
        let (client, server) = duplex(4096);
        let handle = tokio::spawn(serve_fake(server, false));
        let mut proto = channel(client);
        handshake(&mut proto).await.unwrap();
        let tools = list_tools(&mut proto).await.unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "search");
        assert_eq!(tools[0].description, "web search");
        assert_eq!(tools[1].description, "");
        assert_eq!(tools[1].input_schema, json!({"type": "object"}));
        let done = call_tool(&mut proto, "search", json!({"q": "x"}))
            .await
            .unwrap();
        assert!(done.ok);
        assert!(done.text.contains("called search"), "{}", done.text);
        let boom = call_tool(&mut proto, "boom", json!({})).await.unwrap();
        assert!(!boom.ok);
        assert_eq!(boom.text, "kaboom");
        drop(proto);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn erreur_jsonrpc_remontee_avec_contexte() {
        let (client, server) = duplex(4096);
        let handle = tokio::spawn(serve_fake(server, true));
        let mut proto = channel(client);
        handshake(&mut proto).await.unwrap();
        let err = list_tools(&mut proto).await.unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("no tools here"), "{text}");
        assert!(text.contains("code -32601"), "{text}");
        drop(proto);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn ligne_invalide_et_fermeture_en_erreur() {
        let (client, mut server) = duplex(256);
        server.write_all(b"not json\n").await.unwrap();
        // Serveur vivant mais muet : la requête tient dans le tampon,
        // la réponse lue est la ligne invalide.
        let mut proto = channel(client);
        let err = proto.request("tools/list", json!({})).await.unwrap_err();
        assert!(format!("{err:#}").contains("parsing"), "{err:#}");
        drop(server);
        drop(proto);
        let (client, server) = duplex(256);
        tokio::spawn(async move {
            let (reader, _) = split(server);
            let mut lines = BufReader::new(reader);
            let mut line = String::new();
            let _ = lines.read_line(&mut line).await;
        });
        let mut proto = channel(client);
        let err = proto.request("ping", json!({})).await.unwrap_err();
        assert!(format!("{err:#}").contains("closed"), "{err:#}");
    }

    #[test]
    fn parse_tools_sans_nom_ignore_defauts() {
        let result = json!({"tools": [
            {"description": "orphelin"},
            {"name": "ok", "description": "d", "inputSchema": {"type": "string"}},
        ]});
        let tools = parse_tools(&result).unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].input_schema, json!({"type": "string"}));
        assert!(parse_tools(&json!({})).unwrap().is_empty());
        assert!(parse_tools(&json!({"tools": "nope"})).unwrap().is_empty());
    }

    #[test]
    fn render_result_texte_erreur_et_non_texte() {
        let ok = render_result(&json!({"content": [
            {"type": "text", "text": "a"},
            {"type": "text", "text": "b"},
        ]}));
        assert!(ok.ok);
        assert_eq!(ok.text, "a\nb");
        let failed = render_result(&json!({"content": [{"type": "text", "text": "x"}],
            "isError": true}));
        assert!(!failed.ok);
        let mixed = render_result(&json!({"content": [
            {"type": "image", "data": "zzz"},
            {"type": "text", "text": "seen"},
        ]}));
        assert!(mixed.ok);
        assert!(mixed.text.contains("seen"), "{}", mixed.text);
        assert!(mixed.text.contains("1 non-text"), "{}", mixed.text);
        assert!(!mixed.text.contains("zzz"), "{}", mixed.text);
        let big = render_result(&json!({"content": [{"type": "text",
            "text": "y".repeat(crate::tools::MAX_OUTPUT + 10)}]}));
        assert!(big.text.chars().count() <= crate::tools::MAX_OUTPUT);
    }

    #[test]
    fn prefixage_assaini_et_schema() {
        assert_eq!(def_name("gh", "search"), "mcp_gh_search");
        assert_eq!(def_name("my srv!", "a/b"), "mcp_my_srv__a_b");
        assert_eq!(def_name("", ""), "mcp____");
        assert!(is_mcp_name("mcp_gh_search"));
        assert!(!is_mcp_name("shell_run"));
        assert!(!is_mcp_name("mcp"));
        let tool = McpTool {
            name: String::from("search"),
            description: String::from("web search"),
            input_schema: json!({"type": "object"}),
        };
        let def = to_tool_def("gh", &tool);
        assert_eq!(def.function.name, "mcp_gh_search");
        assert!(def.function.description.contains("MCP server gh"));
        let bare = McpTool {
            description: String::new(),
            ..tool
        };
        assert!(to_tool_def("gh", &bare)
            .function
            .description
            .contains("search"));
    }

    #[test]
    fn auto_approve_brut_ou_prefixe() {
        let allowed = vec![String::from("search"), String::from("mcp_gh_pull")];
        assert!(is_auto_approved(&allowed, "gh", "search"));
        assert!(is_auto_approved(&allowed, "gh", "pull"));
        assert!(!is_auto_approved(&allowed, "gh", "push"));
        assert!(!is_auto_approved(&allowed, "other", "pull"));
        assert!(!is_auto_approved(&[], "gh", "search"));
    }
}
