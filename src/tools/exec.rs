//! Exécution locale des outils (processus + timeout + journal).
//!
//! [`run_tool`] dispatche par nom, parse les args JSON, lance le binaire
//! existant (jamais réinventé : `agent-browser`, `wmctrl`, `xdotool`,
//! `xfce4-screenshooter`, `sh`) et rend un [`Outcome`] — les échecs sont
//! des résultats `ok: false`, jamais des erreurs.

use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::process::Command;

use super::log;
use super::wire::ToolDef;
use super::{browser, desktop, MAX_OUTPUT};
use crate::provider::truncate;

/// Résultat d'exécution (succès + texte tronqué, affiché et renvoyé au modèle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub text: String,
}

/// Binaires exécutés (doublures injectables pour les tests).
pub struct Bins {
    pub agent_browser: String,
    pub wmctrl: String,
    pub xdotool: String,
    pub shooter: String,
}

impl Bins {
    /// Binaires système (résolus via `PATH`).
    pub fn system() -> Self {
        Self {
            agent_browser: String::from(browser::BIN),
            wmctrl: String::from(desktop::WMCTRL_BIN),
            xdotool: String::from(desktop::XDOTOOL_BIN),
            shooter: String::from(desktop::SHOOTER_BIN),
        }
    }
}

/// Plan d'exécution (binaire + argv + env + timeout + préfixe éventuel).
struct Plan {
    bin: String,
    argv: Vec<String>,
    envs: Vec<(String, String)>,
    timeout: Duration,
    prefix: Option<String>,
}

/// Timeout navigateur (chargements de page parfois longs).
const BROWSER_TIMEOUT: Duration = Duration::from_secs(90);
/// Timeout bureau (commandes instantanées).
const DESKTOP_TIMEOUT: Duration = Duration::from_secs(30);
/// Timeout shell par défaut et plafond (arg `timeout`, secondes).
const SHELL_DEFAULT: u64 = 30;
const SHELL_MAX: u64 = 120;

/// Exécute l'outil `name` avec ses args JSON (binaires système).
pub async fn run_tool(chat: usize, name: &str, args: &str) -> Outcome {
    run_tool_with(chat, name, args, &Bins::system()).await
}

/// Exécute avec des binaires explicites (journalise dans les deux cas).
pub async fn run_tool_with(chat: usize, name: &str, args: &str, bins: &Bins) -> Outcome {
    let started = Instant::now();
    let outcome = dispatch(name, args, bins).await;
    let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    log::append(
        chat,
        name,
        outcome.ok,
        ms,
        args.chars().count(),
        outcome.text.chars().count(),
    );
    outcome
}

/// Route vers le planificateur de la famille (`browser_*`, `desktop_*`,
/// `shell_run`), exécute le plan ou rend l'erreur de planification.
async fn dispatch(name: &str, args: &str, bins: &Bins) -> Outcome {
    let parsed: Value = serde_json::from_str(args).unwrap_or(Value::Null);
    let plan = plan(name, &parsed, bins);
    let plan = match plan {
        Ok(plan) => plan,
        Err(err) => {
            return Outcome {
                ok: false,
                text: err,
            };
        }
    };
    let mut out = spawn(&plan.bin, &plan.argv, &plan.envs, plan.timeout).await;
    if let Some(prefix) = plan.prefix {
        out.text = format!("{prefix}\n{}", truncate(&out.text, 500));
    }
    out
}

/// Planifie l'outil (pur : parse les args, construit argv, sans exécuter).
fn plan(name: &str, args: &Value, bins: &Bins) -> Result<Plan, String> {
    if let Some(short) = name.strip_prefix("browser_") {
        return plan_browser(short, args, &bins.agent_browser);
    }
    if let Some(short) = name.strip_prefix("desktop_") {
        return plan_desktop(short, args, bins);
    }
    if name == "shell_run" {
        return plan_shell(args);
    }
    Err(format!("unknown tool: {name}"))
}

/// Plan navigateur (session dédiée, timeout long).
fn plan_browser(short: &str, args: &Value, bin: &str) -> Result<Plan, String> {
    let argv = match short {
        "open" => {
            let url = req_str(args, "url")?;
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return Err(format!("only http(s) URLs: {url}"));
            }
            browser::open_argv(&url)
        }
        "snapshot" => browser::snapshot_argv(),
        "read" => browser::read_argv(
            opt_str(args, "url").as_deref(),
            opt_str(args, "filter").as_deref(),
        ),
        "click" => browser::click_argv(&req_str(args, "target")?),
        "fill" => browser::fill_argv(&req_str(args, "target")?, &req_str(args, "text")?),
        "type" => browser::type_argv(&req_str(args, "target")?, &req_str(args, "text")?),
        "press" => browser::press_argv(&req_str(args, "key")?),
        "shot" => {
            let path = log::shot_path("browser").ok_or("cannot create shots dir")?;
            let shown = path.display().to_string();
            return Ok(Plan {
                bin: bin.to_string(),
                argv: browser::shot_argv(&shown),
                envs: vec![(String::from(browser::SESSION_ENV), browser::session())],
                timeout: BROWSER_TIMEOUT,
                prefix: Some(format!("saved to {shown}")),
            });
        }
        _ => return Err(format!("unknown tool: browser_{short}")),
    };
    Ok(Plan {
        bin: bin.to_string(),
        argv,
        envs: vec![(String::from(browser::SESSION_ENV), browser::session())],
        timeout: BROWSER_TIMEOUT,
        prefix: None,
    })
}

/// Plan bureau (affichage réel `:0`, timeout court).
fn plan_desktop(short: &str, args: &Value, bins: &Bins) -> Result<Plan, String> {
    let envs = vec![(String::from("DISPLAY"), String::from(desktop::DISPLAY))];
    let plain = |bin: &str, argv: Vec<String>| Plan {
        bin: bin.to_string(),
        argv,
        envs: envs.clone(),
        timeout: DESKTOP_TIMEOUT,
        prefix: None,
    };
    match short {
        "windows" => Ok(plain(&bins.wmctrl, desktop::windows_argv())),
        "focus" => Ok(plain(
            &bins.wmctrl,
            desktop::focus_argv(&req_str(args, "query")?),
        )),
        "key" => {
            let keys: Vec<String> = opt_str(args, "keys")
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_string)
                .collect();
            if keys.is_empty() {
                return Err(String::from("missing keys"));
            }
            Ok(plain(&bins.xdotool, desktop::key_argv(&keys)))
        }
        "type" => Ok(plain(
            &bins.xdotool,
            desktop::type_argv(&req_str(args, "text")?),
        )),
        "shot" => {
            let path = log::shot_path("desktop").ok_or("cannot create shots dir")?;
            let window = args.get("window").and_then(Value::as_bool).unwrap_or(false);
            let shown = path.display().to_string();
            Ok(Plan {
                bin: bins.shooter.clone(),
                argv: desktop::shot_argv(window, &shown),
                envs,
                timeout: DESKTOP_TIMEOUT,
                prefix: Some(format!("saved to {shown}")),
            })
        }
        _ => Err(format!("unknown tool: desktop_{short}")),
    }
}

/// Plan shell (`sh -c`, timeout arg ou défaut, plafonné).
fn plan_shell(args: &Value) -> Result<Plan, String> {
    let cmd = req_str(args, "cmd")?;
    let secs = args
        .get("timeout")
        .and_then(Value::as_u64)
        .unwrap_or(SHELL_DEFAULT)
        .clamp(1, SHELL_MAX);
    Ok(Plan {
        bin: String::from("sh"),
        argv: vec![String::from("-c"), cmd],
        envs: Vec::new(),
        timeout: Duration::from_secs(secs),
        prefix: None,
    })
}

/// Lance un processus (sorties pipées, stdin fermé, timeout, tronqué).
async fn spawn(
    bin: &str,
    argv: &[String],
    envs: &[(String, String)],
    timeout: Duration,
) -> Outcome {
    let mut cmd = Command::new(bin);
    cmd.args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, val) in envs {
        cmd.env(key, val);
    }
    let out = match tokio::time::timeout(timeout, cmd.output()).await {
        Err(_) => {
            return Outcome {
                ok: false,
                text: format!("timeout after {}s", timeout.as_secs()),
            };
        }
        Ok(Err(err)) => {
            return Outcome {
                ok: false,
                text: format!("cannot run {bin}: {err}"),
            };
        }
        Ok(Ok(out)) => out,
    };
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Outcome {
            ok: true,
            text: truncate(&stdout, MAX_OUTPUT),
        }
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        Outcome {
            ok: false,
            text: truncate(
                &format!("exit {}: {stdout}{stderr}", out.status.code().unwrap_or(-1)),
                MAX_OUTPUT,
            ),
        }
    }
}

/// Arg string requise (non vide après trim).
fn req_str(args: &Value, key: &str) -> Result<String, String> {
    match args.get(key).and_then(Value::as_str).map(str::trim) {
        Some(val) if !val.is_empty() => Ok(val.to_string()),
        _ => Err(format!("missing {key}")),
    }
}

/// Arg string optionnelle (vide → `None`).
fn opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|val| !val.is_empty())
        .map(str::to_string)
}

/// Schéma de `shell_run` (commande + timeout optionnel).
pub fn shell_def() -> ToolDef {
    ToolDef::new(
        "shell_run",
        "Run a shell command (sh -c, non-interactive, output truncated). Sudo fails (no TTY).",
        serde_json::json!({"type":"object","properties":{
            "cmd":{"type":"string","description":"shell command"},
            "timeout":{"type":"integer","description":"timeout in seconds (default 30, max 120)"}
        },"required":["cmd"],"additionalProperties":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn outil_inconnu_et_args_invalides() {
        let out = run_tool(0, "nope", "{}").await;
        assert!(!out.ok);
        assert!(out.text.contains("unknown tool"));
        let out = run_tool(0, "shell_run", "not json").await;
        assert!(!out.ok);
        assert!(out.text.contains("missing cmd"));
        let out = run_tool(0, "browser_open", r#"{"url":"ftp://x"}"#).await;
        assert!(!out.ok);
        assert!(out.text.contains("http(s)"));
    }

    #[tokio::test]
    async fn shell_echo_et_timeout() {
        let out = run_tool(0, "shell_run", r#"{"cmd":"echo hi"}"#).await;
        assert!(out.ok);
        assert_eq!(out.text, "hi\n");
        let out = run_tool(0, "shell_run", r#"{"cmd":"exit 3"}"#).await;
        assert!(!out.ok);
        assert!(out.text.contains("exit 3"));
        let out = run_tool(0, "shell_run", r#"{"cmd":"sleep 5","timeout":1}"#).await;
        assert!(!out.ok);
        assert!(out.text.contains("timeout"));
    }

    #[tokio::test]
    async fn navigateur_via_doublure_avec_session() {
        let dir = std::env::temp_dir().join(format!("kh-tools-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("agent-browser");
        std::fs::write(
            &fake,
            "#!/bin/sh\necho \"SESSION=$AGENT_BROWSER_SESSION ARGS=$*\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake, perms).unwrap();
        }
        let bins = Bins {
            agent_browser: fake.display().to_string(),
            ..Bins::system()
        };
        let out = run_tool_with(0, "browser_click", r#"{"target":"@e3"}"#, &bins).await;
        assert!(out.ok);
        assert!(out
            .text
            .contains(&format!("SESSION=crypte-{}", std::process::id())));
        assert!(out.text.contains("ARGS=click @e3"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parse_args_requis_optionnel() {
        let args: Value = serde_json::from_str(r#"{"a":" x ","b":""}"#).unwrap();
        assert_eq!(req_str(&args, "a").unwrap(), "x");
        assert!(req_str(&args, "b").is_err());
        assert!(req_str(&args, "z").is_err());
        assert_eq!(opt_str(&args, "a").unwrap(), "x");
        assert_eq!(opt_str(&args, "b"), None);
    }

    #[test]
    fn plan_shell_plafonne_le_timeout() {
        let args: Value = serde_json::from_str(r#"{"cmd":"true","timeout":9999}"#).unwrap();
        let plan = plan("shell_run", &args, &Bins::system()).unwrap();
        assert_eq!(plan.timeout, Duration::from_secs(SHELL_MAX));
        assert_eq!(plan.bin, "sh");
    }
}
