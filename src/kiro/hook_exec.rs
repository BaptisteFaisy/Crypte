//! Exécution shell des hooks `command` (T3, extrait de `hook_run`).
//!
//! `sh -c` async (cwd projet, stdin nul, stdout+stderr joints,
//! tronqués) : `(ok, texte, ms)`. Timeout 30 s = échec `timed out`,
//! résultat livré via `Incoming::HookDone` (jamais bloquant).

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use super::hooks::Trigger;
use crate::app::Incoming;

/// Timeout d'une commande hook (Kiro-like, jamais bloquant).
pub const HOOK_TIMEOUT: Duration = Duration::from_secs(30);
/// Sortie hook gardée (caractères, voir `provider::truncate`).
const HOOK_OUT_MAX: usize = 2000;

/// Résultat d'une commande hook (livré via `Incoming::HookDone`).
#[derive(Debug, Clone)]
pub struct HookOutcome {
    pub chat: usize,
    pub name: String,
    pub trigger: Trigger,
    pub ok: bool,
    pub text: String,
    pub ms: u64,
}

/// Exécute `sh -c` : `(ok, texte, ms)`. Timeout = échec `timed out`.
pub async fn run_hook_command(cmd: &str, cwd: &Path, timeout: Duration) -> (bool, String, u64) {
    let started = std::time::Instant::now();
    let ms = |started: std::time::Instant| started.elapsed().as_millis().min(9_999_999) as u64;
    let spawn = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let child = match spawn {
        Ok(child) => child,
        Err(err) => return (false, format!("spawn failed: {err}"), ms(started)),
    };
    let out = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Err(_) => {
            return (
                false,
                format!("timed out after {}s", timeout.as_secs()),
                ms(started),
            )
        }
        Ok(Err(err)) => return (false, format!("wait failed: {err}"), ms(started)),
        Ok(Ok(out)) => out,
    };
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&err);
    }
    let text = crate::provider::truncate(&text, HOOK_OUT_MAX);
    (out.status.success(), text, ms(started))
}

/// Lance une commande en fond, résultat via `Incoming::HookDone`
/// (chats et sous-agents partagent ce spawn, même tombée).
pub(crate) fn spawn_command(
    tx: &tokio::sync::mpsc::Sender<Incoming>,
    chat: usize,
    name: &str,
    cmd: &str,
    trigger: Trigger,
    cwd: &Path,
) {
    let tx = tx.clone();
    let cwd = cwd.to_path_buf();
    let name = name.to_string();
    let cmd = cmd.to_string();
    tokio::spawn(async move {
        let (ok, text, ms) = run_hook_command(&cmd, &cwd, HOOK_TIMEOUT).await;
        let out = HookOutcome {
            chat,
            name,
            trigger,
            ok,
            text,
            ms,
        };
        let _ = tx.send(Incoming::HookDone(out)).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("kh-hook-exec-{}-{name}", std::process::id()))
    }

    #[tokio::test]
    async fn run_command_echo_echec_timeout_troncature() {
        let dir = tmp_dir("shell");
        std::fs::create_dir_all(&dir).unwrap();
        let (ok, text, _) =
            run_hook_command("echo hi; echo err >&2", &dir, Duration::from_secs(5)).await;
        assert!(ok);
        assert!(text.contains("hi") && text.contains("err"), "{text}");
        let (ok, _, _) = run_hook_command("exit 3", &dir, Duration::from_secs(5)).await;
        assert!(!ok);
        let (ok, text, _) = run_hook_command("sleep 30", &dir, Duration::from_millis(50)).await;
        assert!(!ok);
        assert!(text.contains("timed out"), "{text}");
        let (ok, text, _) = run_hook_command("seq 1 3000", &dir, Duration::from_secs(5)).await;
        assert!(ok);
        assert!(text.chars().count() <= HOOK_OUT_MAX + 1, "tronqué");
        std::fs::remove_dir_all(&dir).ok();
    }
}
