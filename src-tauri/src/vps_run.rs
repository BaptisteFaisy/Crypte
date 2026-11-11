//! Opérations VPS distantes, version Tauri.
//!
//! Reprend de `src/cmds_vps.rs` (TUI) `run_ssh` (à l'identique) et les
//! textes de `drain_vps`. Adaptation : pas de `App`, l'intégrateur
//! exécute les tâches de fond et émet l'event `vps_outcome`.

use crate::vps::{self, VpsOutcome, VpsProfile};

/// Opérations distantes (`launch|stop|status|install`, comme le TUI).
pub fn command_for(profile: &VpsProfile, op: &str) -> String {
    match op {
        "launch" => profile.launch_cmd(),
        "stop" => profile.stop_cmd(),
        "status" => profile.status_cmd(),
        _ => profile.install_cmd(),
    }
}

/// `ssh … -- <cmd>` puis traduction de la sortie en [`VpsOutcome`].
pub async fn run_ssh(profile: &VpsProfile, op: &'static str, cmd: &str) -> VpsOutcome {
    let target = profile.name.clone();
    let fail = |error: String| VpsOutcome::Failed {
        target: target.clone(),
        op: op.to_string(),
        error,
    };
    let mut argv = profile.ssh_argv();
    argv.push(cmd.to_string());
    // stdin coupé : sinon ssh le dévore et le TUI devient sourd.
    let out = match tokio::process::Command::new("ssh")
        .args(&argv)
        .stdin(std::process::Stdio::null())
        .output()
        .await
    {
        Ok(out) => out,
        Err(error) => return fail(format!("ssh: {error}")),
    };
    match op {
        "status" => match vps::parse_status(&out) {
            Ok(up) => VpsOutcome::Status { target, up },
            Err(error) => fail(error),
        },
        "launch" => match vps::parse_marker(&out, op, "UP") {
            Ok(()) => VpsOutcome::Launched { target },
            Err(error) => fail(error),
        },
        "stop" => match vps::parse_marker(&out, op, "STOPPED") {
            Ok(()) => VpsOutcome::Stopped { target },
            Err(error) => fail(error),
        },
        _ => match vps::parse_marker(&out, op, "INSTALLED") {
            Ok(()) => VpsOutcome::Installed { target },
            Err(error) => fail(error),
        },
    }
}

/// Texte d'aboutissement (mêmes textes que `drain_vps`).
/// `ssh_prefix` = `ssh …` affichable (fourni par l'intégrateur).
pub fn outcome_text(outcome: &VpsOutcome, ssh_prefix: Option<&str>) -> (bool, String) {
    match outcome {
        VpsOutcome::Launched { target } => {
            let dst = ssh_prefix.unwrap_or(target);
            (
                true,
                format!(
                    "vps: {target} UP — attach: {dst} -t 'tmux attach -t crypte' · \
                     events: {dst} 'tail -F ~/.crypte-log/events.jsonl'"
                ),
            )
        }
        VpsOutcome::Stopped { target } => (true, format!("vps: {target} stopped")),
        VpsOutcome::Status { target, up } => (
            true,
            format!("vps: {target} {}", if *up { "UP" } else { "DOWN" }),
        ),
        VpsOutcome::Installed { target } => (
            true,
            format!("vps: {target} installed — /vps {target} to launch"),
        ),
        VpsOutcome::Failed { target, op, error } => (false, format!("vps: {op} {target}: {error}")),
    }
}

/// Préfixe ssh affichable (`ssh [-p P] [-i K] user@host`).
pub fn ssh_prefix(profile: Option<&VpsProfile>, target: &str) -> String {
    match profile {
        Some(profile) => {
            let mut argv = profile.ssh_argv();
            argv.pop();
            format!("ssh {}", argv.join(" "))
        }
        None => format!("ssh {target}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> VpsProfile {
        vps::resolve("b@h:2222", &[]).unwrap()
    }

    #[test]
    fn textes_d_aboutissement_identiques_au_tui() {
        let p = profile();
        let prefix = ssh_prefix(Some(&p), &p.name);
        let (ok, text) = outcome_text(
            &VpsOutcome::Launched {
                target: String::from("r630"),
            },
            Some(&prefix),
        );
        assert!(ok);
        assert!(text.contains("UP — attach:"), "{text}");
        assert!(text.contains("tmux attach -t crypte"), "{text}");
        let (ok, text) = outcome_text(
            &VpsOutcome::Status {
                target: String::from("r630"),
                up: false,
            },
            None,
        );
        assert!(ok && text.contains("DOWN"), "{text}");
        let (ok, text) = outcome_text(
            &VpsOutcome::Failed {
                target: String::from("r630"),
                op: String::from("launch"),
                error: String::from("ssh failed"),
            },
            None,
        );
        assert!(!ok && text.contains("launch r630: ssh failed"), "{text}");
        assert!(prefix.starts_with("ssh -n"));
    }

    #[tokio::test]
    async fn ssh_injoignable_devient_failed() {
        let mut p = profile();
        p.host = String::from("127.0.0.1");
        p.port = 9;
        let out = run_ssh(&p, "status", "true").await;
        assert!(matches!(out, VpsOutcome::Failed { .. }), "{out:?}");
    }
}
