//! Transports MCP : stdio (processus fils) et distant (Streamable HTTP).
//!
//! [`Client`] choisit (`command` → stdio, sinon `url`), fait le handshake
//! puis expose `list_tools` / `call` / `shutdown`. HTTP : POST JSON-RPC,
//! réponse JSON simple, repli SSE défensif (voir `extract_sse`).

use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::header::{HeaderName, HeaderValue};
use serde_json::Value;
use tokio::io::AsyncReadExt;

use super::mcp::Server;
use super::mcp_client::{
    call_tool, error_text, handshake, init_params, list_tools, parse_tools, render_result,
    request_value, CallOutcome, McpTool, Proto, REQUEST_TIMEOUT,
};
use crate::provider::truncate;

/// Arrêt stdio : EOF puis `wait` borné, `kill` au-delà.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Connexion stdio : fils + canal + vidange `stderr` (jamais bloquante).
pub struct StdioConn {
    child: tokio::process::Child,
    proto: Proto<tokio::process::ChildStdout, tokio::process::ChildStdin>,
    stderr_drain: tokio::task::JoinHandle<()>,
}

impl StdioConn {
    /// Lance le serveur (`command` + `args` + `env`, `cwd` si fourni).
    pub async fn spawn(name: &str, server: &Server) -> Result<Self> {
        let command = server.command.as_deref().unwrap_or("");
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(&server.args)
            .envs(&server.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(cwd) = server.cwd.as_deref().filter(|cwd| !cwd.trim().is_empty()) {
            cmd.current_dir(cwd);
        }
        let mut child = cmd
            .spawn()
            .with_context(|| format!("spawning MCP server '{name}' ({command})"))?;
        let stdin = child.stdin.take().context("MCP stdio stdin unavailable")?;
        let stdout = child
            .stdout
            .take()
            .context("MCP stdio stdout unavailable")?;
        let stderr = child
            .stderr
            .take()
            .context("MCP stdio stderr unavailable")?;
        let stderr_drain = tokio::spawn(async move {
            let mut err = stderr;
            let mut buf = [0u8; 1024];
            while let Ok(read) = err.read(&mut buf).await {
                if read == 0 {
                    break;
                }
            }
        });
        let mut proto = Proto::new(stdout, stdin);
        handshake(&mut proto)
            .await
            .with_context(|| format!("MCP handshake with '{name}'"))?;
        Ok(Self {
            child,
            proto,
            stderr_drain,
        })
    }

    /// `tools/list` (erreurs contextées au serveur).
    pub async fn list_tools(&mut self, name: &str) -> Result<Vec<McpTool>> {
        list_tools(&mut self.proto)
            .await
            .with_context(|| format!("MCP tools/list from '{name}'"))
    }

    /// `tools/call` (erreurs contextées au serveur + outil).
    pub async fn call(&mut self, name: &str, tool: &str, args: Value) -> Result<CallOutcome> {
        call_tool(&mut self.proto, tool, args)
            .await
            .with_context(|| format!("MCP tool '{tool}' on '{name}'"))
    }

    /// Arrêt propre : stdin fermé (EOF), `wait` borné, `kill` au-delà.
    pub async fn shutdown(self) {
        let Self {
            mut child,
            proto,
            stderr_drain,
        } = self;
        drop(proto);
        let _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, child.wait()).await;
        let _ = child.kill().await;
        stderr_drain.abort();
    }
}

/// Connexion distante (Streamable HTTP : POST JSON-RPC, `id` local).
pub struct HttpConn {
    client: reqwest::Client,
    url: String,
    headers: Vec<(HeaderName, HeaderValue)>,
    next_id: u64,
}

impl HttpConn {
    /// Handshake distant (`url` + `headers`, timeout requête).
    pub async fn connect(name: &str, server: &Server) -> Result<Self> {
        let mut conn = Self {
            client: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .context("building MCP HTTP client")?,
            url: server.url.as_deref().unwrap_or("").to_string(),
            headers: valid_headers(&server.headers),
            next_id: 1,
        };
        conn.post(name, "initialize", init_params())
            .await
            .with_context(|| format!("MCP handshake with '{name}'"))?;
        conn.notify("notifications/initialized").await;
        Ok(conn)
    }

    /// POST JSON-RPC → `result` (erreur HTTP/RPC = `Err` contexté).
    async fn post(&mut self, name: &str, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut req = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .json(&request_value(id, method, params));
        for (key, val) in &self.headers {
            req = req.header(key.clone(), val.clone());
        }
        let resp = req
            .send()
            .await
            .with_context(|| format!("MCP POST {method} to '{name}'"))?;
        let status = resp.status();
        let raw = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!(
                "MCP {method} on '{name}' -> HTTP {status}: {}",
                truncate(&raw, 300)
            );
        }
        let msg =
            response_value(&raw).with_context(|| format!("parsing MCP {method} from '{name}'"))?;
        if let Some(err) = msg.get("error") {
            anyhow::bail!("MCP {method} on '{name}' failed: {}", error_text(err));
        }
        Ok(msg.get("result").cloned().unwrap_or(Value::Null))
    }

    /// Notification HTTP (best-effort : réponse ignorée, succès comme échec).
    async fn notify(&mut self, method: &str) {
        let note = serde_json::json!({"jsonrpc": "2.0", "method": method, "params": {}});
        let mut req = self.client.post(&self.url).json(&note);
        for (key, val) in &self.headers {
            req = req.header(key.clone(), val.clone());
        }
        let _ = req.send().await;
    }

    /// `tools/list` distant (réponse JSON ou SSE).
    pub async fn list_tools(&mut self, name: &str) -> Result<Vec<McpTool>> {
        let result = self.post(name, "tools/list", serde_json::json!({})).await?;
        parse_tools(&result)
    }

    /// `tools/call` distant (texte rendu, `isError` = échec).
    pub async fn call(&mut self, name: &str, tool: &str, args: Value) -> Result<CallOutcome> {
        let params = serde_json::json!({"name": tool, "arguments": args});
        let result = self.post(name, "tools/call", params).await?;
        Ok(render_result(&result))
    }
}

/// En-têtes configurés valides (invalides = ignorés silencieusement).
fn valid_headers(headers: &HashMap<String, String>) -> Vec<(HeaderName, HeaderValue)> {
    headers
        .iter()
        .filter_map(|(key, val)| {
            let name = HeaderName::from_bytes(key.as_bytes()).ok()?;
            let value = HeaderValue::from_str(val).ok()?;
            Some((name, value))
        })
        .collect()
}

/// Réponse HTTP : JSON direct, sinon repli SSE (`data:`).
fn response_value(raw: &str) -> Result<Value> {
    if let Ok(msg) = serde_json::from_str(raw) {
        return Ok(msg);
    }
    extract_sse(raw)
}

/// Première ligne `data:` SSE qui parse en JSON (sinon erreur claire).
fn extract_sse(raw: &str) -> Result<Value> {
    for line in raw.lines() {
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        if let Ok(msg) = serde_json::from_str(payload.trim()) {
            return Ok(msg);
        }
    }
    anyhow::bail!("not JSON nor SSE: {}", truncate(raw.trim(), 200))
}

/// Client MCP connecté (stdio ou distant), handshake fait.
pub enum Client {
    Stdio(StdioConn),
    Http(HttpConn),
}

impl Client {
    /// Connecte (`command` → stdio, sinon `url`) + handshake.
    /// Ni l'un ni l'autre = erreur (serveur vide).
    pub async fn connect(name: &str, server: &Server) -> Result<Self> {
        if server
            .command
            .as_deref()
            .is_some_and(|cmd| !cmd.trim().is_empty())
        {
            Ok(Self::Stdio(StdioConn::spawn(name, server).await?))
        } else if server
            .url
            .as_deref()
            .is_some_and(|url| !url.trim().is_empty())
        {
            Ok(Self::Http(HttpConn::connect(name, server).await?))
        } else {
            anyhow::bail!("MCP server '{name}' has neither command nor url")
        }
    }

    /// `tools/list` (transport réel, erreurs contextées).
    pub async fn list_tools(&mut self, name: &str) -> Result<Vec<McpTool>> {
        match self {
            Self::Stdio(conn) => conn.list_tools(name).await,
            Self::Http(conn) => conn.list_tools(name).await,
        }
    }

    /// `tools/call` (transport réel, erreurs contextées).
    pub async fn call(&mut self, name: &str, tool: &str, args: Value) -> Result<CallOutcome> {
        match self {
            Self::Stdio(conn) => conn.call(name, tool, args).await,
            Self::Http(conn) => conn.call(name, tool, args).await,
        }
    }

    /// Arrêt propre (stdio : EOF + `wait` + `kill` ; HTTP : rien à faire).
    pub async fn shutdown(self) {
        match self {
            Self::Stdio(conn) => conn.shutdown().await,
            Self::Http(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Faux serveur MCP stdio (python3, `argv[1]` = fichier PID).
    const FAKE_SERVER_PY: &str = r#"import sys, json, os
open(sys.argv[1], "w").write(str(os.getpid()))
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        msg = json.loads(line)
    except Exception:
        continue
    if "id" not in msg:
        continue
    method = msg.get("method", "")
    if method == "initialize":
        body = {"result": {"protocolVersion": "9"}}
    elif method == "tools/list":
        body = {"result": {"tools": [{"name": "echo", "description": "repeat",
            "inputSchema": {"type": "object"}}]}}
    elif method == "tools/call":
        params = msg.get("params", {})
        body = {"result": {"content": [{"type": "text",
            "text": "echo:" + json.dumps(params.get("arguments", {}))}]}}
    else:
        body = {"error": {"code": -32601, "message": "unknown method"}}
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": msg["id"], **body}) + "\n")
    sys.stdout.flush()
"#;

    /// Écrit le faux serveur dans `dir` (rend son chemin).
    fn write_fake_server(dir: &std::path::Path) -> std::path::PathBuf {
        let path = dir.join("mcp-fake-server.py");
        std::fs::write(&path, FAKE_SERVER_PY).unwrap();
        path
    }

    /// Vrai si le PID a disparu (Linux : `/proc` ; ailleurs : toujours vrai).
    fn gone(pid: u32) -> bool {
        #[cfg(target_os = "linux")]
        {
            !std::path::Path::new(&format!("/proc/{pid}")).exists()
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = pid;
            true
        }
    }

    #[tokio::test]
    async fn connect_sans_transport_erreur_claire() {
        let err = Client::connect("ghost", &Server::default())
            .await
            .err()
            .unwrap();
        let text = format!("{err:#}");
        assert!(text.contains("ghost"), "{text}");
        assert!(text.contains("neither command nor url"), "{text}");
    }

    #[tokio::test]
    async fn spawn_binaire_absent_erreur_contexte() {
        let server = Server {
            command: Some(String::from("kh-mcp-no-such-bin")),
            ..Default::default()
        };
        let err = StdioConn::spawn("gone", &server).await.err().unwrap();
        let text = format!("{err:#}");
        assert!(text.contains("gone"), "{text}");
        assert!(text.contains("spawning"), "{text}");
    }

    #[tokio::test]
    async fn http_injoignable_erreur_contexte() {
        let server = Server {
            url: Some(String::from("http://localhost:9/")),
            ..Default::default()
        };
        let err = HttpConn::connect("far", &server).await.err().unwrap();
        let text = format!("{err:#}");
        assert!(text.contains("far"), "{text}");
        assert!(text.contains("handshake"), "{text}");
    }

    #[test]
    fn reponse_json_direct_ou_sse() {
        let direct: Value = response_value(r#"{"result": 1}"#).unwrap();
        assert_eq!(direct["result"], 1);
        let sse: Value = response_value("event: message\ndata: {\"result\": 2}\n\n").unwrap();
        assert_eq!(sse["result"], 2);
        let err = response_value("garbage").unwrap_err();
        assert!(format!("{err:#}").contains("not JSON nor SSE"), "{err:#}");
        let headers = HashMap::from([
            (String::from("X-Ok"), String::from("yes")),
            (String::from("bad name"), String::from("no")),
        ]);
        assert_eq!(valid_headers(&headers).len(), 1);
    }

    #[tokio::test]
    #[ignore = "live stdio: vrai python3 + handshake + call + arrêt (skip sans python3)"]
    async fn live_stdio_protocole_reel_et_arret() {
        let has_python = std::process::Command::new("python3")
            .arg("--version")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false);
        if !has_python {
            return;
        }
        let dir = std::env::temp_dir().join(format!("kh-mcp-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = write_fake_server(&dir);
        let pidfile = dir.join("server.pid");
        let server = Server {
            command: Some(String::from("python3")),
            args: vec![script.display().to_string(), pidfile.display().to_string()],
            ..Default::default()
        };
        let mut conn = StdioConn::spawn("pyfake", &server).await.unwrap();
        let tools = conn.list_tools("pyfake").await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        let done = conn
            .call("pyfake", "echo", serde_json::json!({"x": 1}))
            .await
            .unwrap();
        assert!(done.ok);
        assert!(
            done.text.contains("echo:") && done.text.contains('x'),
            "{}",
            done.text
        );
        conn.shutdown().await;
        let pid: u32 = std::fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        for _ in 0..100 {
            if gone(pid) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(gone(pid), "le serveur stdio a survécu à shutdown");
        std::fs::remove_dir_all(&dir).ok();
    }
}
