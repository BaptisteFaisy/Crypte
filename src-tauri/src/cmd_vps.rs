//! Commandes §10 : profils + opérations VPS distantes.
//!
//! Comportement TUI inchangé (ssh `BatchMode`, tmux `crypte`,
//! profils `vps.toml`). Les opérations rendent un ACK immédiat et
//! aboutissent par event `vps_outcome` (tâche de fond par profil).

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::{lock, BackendOut, ToolsBackend};
use crate::views::{UiEvent, VpsAck, VpsProfileInput, VpsProfileView, VpsStateView, UI_CHANNEL};
use crate::vps::{VpsProfile, VpsRuntime, VpsStatus};

/// `vps_list` : profils + sélection + derniers statuts connus.
#[tauri::command]
pub fn vps_list(backend: State<Mutex<ToolsBackend>>) -> Result<VpsStateView, String> {
    let backend = lock(&backend)?;
    Ok(VpsStateView {
        profiles: backend.vps.profiles.iter().map(profile_view).collect(),
        current: backend.vps.current.clone(),
        status_by_name: backend.vps_statuses(),
    })
}

/// `vps_launch` : (re)lance le TUI distant en tmux → event.
#[tauri::command]
pub async fn vps_launch(
    app: AppHandle,
    backend: State<'_, Mutex<ToolsBackend>>,
    target: String,
) -> Result<VpsAck, String> {
    let profile = {
        let backend = lock(&backend)?;
        match crate::vps::resolve(&target, &backend.vps.profiles) {
            Ok(profile) => profile,
            Err(error) => {
                return Ok(VpsAck {
                    accepted: false,
                    detail: format!("vps: {error}"),
                })
            }
        }
    };
    let ack = VpsAck {
        accepted: true,
        detail: format!("vps: launching on {} …", profile.dst()),
    };
    spawn_op(app, backend, profile, "launch");
    Ok(ack)
}

/// `vps_stop` : tue la session distante (défaut : dernière) → event.
#[tauri::command]
pub async fn vps_stop(
    app: AppHandle,
    backend: State<'_, Mutex<ToolsBackend>>,
    target: Option<String>,
) -> Result<VpsAck, String> {
    let profile = {
        let backend = lock(&backend)?;
        let target = match target.or_else(|| backend.vps.current.clone()) {
            Some(target) => target,
            None => {
                return Ok(VpsAck {
                    accepted: false,
                    detail: String::from("vps: nothing launched here (give a target)"),
                })
            }
        };
        match crate::vps::resolve(&target, &backend.vps.profiles) {
            Ok(profile) => profile,
            Err(error) => {
                return Ok(VpsAck {
                    accepted: false,
                    detail: format!("vps: {error}"),
                })
            }
        }
    };
    let ack = VpsAck {
        accepted: true,
        detail: format!("vps: stopping {} …", profile.dst()),
    };
    spawn_op(app, backend, profile, "stop");
    Ok(ack)
}

/// `vps_status` : session up/down (défaut : tous profils) → events.
#[tauri::command]
pub async fn vps_status(
    app: AppHandle,
    backend: State<'_, Mutex<ToolsBackend>>,
    target: Option<String>,
) -> Result<VpsAck, String> {
    let profiles = {
        let backend = lock(&backend)?;
        match target {
            Some(target) => match crate::vps::resolve(&target, &backend.vps.profiles) {
                Ok(profile) => vec![profile],
                Err(error) => {
                    return Ok(VpsAck {
                        accepted: false,
                        detail: format!("vps: {error}"),
                    })
                }
            },
            None => {
                if backend.vps.profiles.is_empty() {
                    return Ok(VpsAck {
                        accepted: false,
                        detail: String::from("vps: no profiles — /vps add <name> <user@host>"),
                    });
                }
                backend.vps.profiles.clone()
            }
        }
    };
    let count = profiles.len();
    for profile in profiles {
        spawn_op(app.clone(), backend.clone(), profile, "status");
    }
    Ok(VpsAck {
        accepted: true,
        detail: format!("vps: status refresh ×{count} …"),
    })
}

/// `vps_add` : sauve un profil (crée ou met à jour, `vps.toml`).
#[tauri::command]
pub fn vps_add(
    backend: State<Mutex<ToolsBackend>>,
    profile: VpsProfileInput,
) -> Result<(), String> {
    let mut backend = lock(&backend)?;
    if profile.name.trim().is_empty() {
        return Err(String::from("vps: empty profile name"));
    }
    if profile.user.trim().is_empty() || profile.host.trim().is_empty() {
        return Err(format!(
            "vps: bad target {:?} (want [user@]host[:port])",
            profile.host
        ));
    }
    let ours = VpsProfile {
        name: profile.name.clone(),
        host: profile.host,
        user: profile.user,
        port: profile.port,
        key: profile.key.map(std::path::PathBuf::from),
        dir: profile.dir,
    };
    let updated = backend.vps.find(&profile.name).is_some();
    if updated {
        backend.vps.profiles.retain(|p| p.name != profile.name);
    }
    backend.vps.profiles.push(ours.clone());
    let chat = backend.current_chat;
    if let Err(error) = crate::vps::save(&backend.vps.profiles) {
        backend.outbox.push(BackendOut::ChatLine {
            chat,
            text: format!("vps: save failed: {error}"),
        });
    } else {
        backend.outbox.push(BackendOut::ChatLine {
            chat,
            text: format!(
                "vps: profile {:?} {} ({})",
                profile.name,
                if updated { "updated" } else { "saved" },
                ours.dst(),
            ),
        });
    }
    Ok(())
}

/// `vps_remove` : oublie un profil (`vps.toml` réécrit).
#[tauri::command]
pub fn vps_remove(backend: State<Mutex<ToolsBackend>>, name: String) -> Result<(), String> {
    let mut backend = lock(&backend)?;
    if backend.vps.find(&name).is_none() {
        return Err(format!("vps: unknown profile {name:?}"));
    }
    backend.vps.profiles.retain(|p| p.name != name);
    backend.vps.clamp_selection();
    let chat = backend.current_chat;
    if let Err(error) = crate::vps::save(&backend.vps.profiles) {
        backend.outbox.push(BackendOut::ChatLine {
            chat,
            text: format!("vps: save failed: {error}"),
        });
    } else {
        backend.outbox.push(BackendOut::ChatLine {
            chat,
            text: format!("vps: profile {name:?} deleted"),
        });
    }
    Ok(())
}

/// `vps_install` : clone/pull + build release distant → event.
#[tauri::command]
pub async fn vps_install(
    app: AppHandle,
    backend: State<'_, Mutex<ToolsBackend>>,
    target: String,
) -> Result<VpsAck, String> {
    let profile = {
        let backend = lock(&backend)?;
        match crate::vps::resolve(&target, &backend.vps.profiles) {
            Ok(profile) => profile,
            Err(error) => {
                return Ok(VpsAck {
                    accepted: false,
                    detail: format!("vps: {error}"),
                })
            }
        }
    };
    let ack = VpsAck {
        accepted: true,
        detail: format!("vps: installing on {} (minutes) …", profile.dst()),
    };
    spawn_op(app, backend, profile, "install");
    Ok(ack)
}

/// Lance `op` en tâche de fond : statut `working`, ssh, état + ligne
/// chat (outbox) + event `vps_outcome` (mêmes textes que `drain_vps`).
/// La tâche relit l'état via `AppHandle` (`State` ne survit pas au spawn).
fn spawn_op(
    app: AppHandle,
    backend: State<'_, Mutex<ToolsBackend>>,
    profile: VpsProfile,
    op: &'static str,
) {
    if let Ok(mut locked) = backend.lock() {
        locked.vps.set(&profile.name, VpsStatus::Working);
    }
    tokio::spawn(async move {
        let cmd = crate::vps_run::command_for(&profile, op);
        let outcome = crate::vps_run::run_ssh(&profile, op, &cmd).await;
        let target = outcome_target(&outcome).to_string();
        let event = {
            let state = app.state::<Mutex<ToolsBackend>>();
            let Ok(mut locked) = state.lock() else {
                return;
            };
            let prefix = crate::vps_run::ssh_prefix(locked.vps.find(&target), &target);
            let (ok, text) = crate::vps_run::outcome_text(&outcome, Some(&prefix));
            apply_outcome(&mut locked.vps, &outcome);
            let chat = locked.current_chat;
            locked.outbox.push(BackendOut::ChatLine {
                chat,
                text: text.clone(),
            });
            UiEvent::VpsOutcome {
                target: target.clone(),
                op: op.to_string(),
                ok,
                text,
            }
        };
        let _ = app.emit(UI_CHANNEL, event);
    });
}

/// Cible d'un aboutissement.
fn outcome_target(outcome: &crate::vps::VpsOutcome) -> &str {
    use crate::vps::VpsOutcome;
    match outcome {
        VpsOutcome::Launched { target }
        | VpsOutcome::Stopped { target }
        | VpsOutcome::Status { target, .. }
        | VpsOutcome::Installed { target }
        | VpsOutcome::Failed { target, .. } => target,
    }
}

/// Mémorise l'aboutissement (mêmes transitions que `drain_vps`).
fn apply_outcome(runtime: &mut VpsRuntime, outcome: &crate::vps::VpsOutcome) {
    use crate::vps::VpsOutcome;
    match outcome {
        VpsOutcome::Launched { target } => {
            runtime.set(target, VpsStatus::Up);
            runtime.current = Some(target.clone());
        }
        VpsOutcome::Stopped { target } => runtime.set(target, VpsStatus::Down),
        VpsOutcome::Status { target, up } => {
            runtime.set(target, if *up { VpsStatus::Up } else { VpsStatus::Down })
        }
        VpsOutcome::Installed { target } => runtime.set(target, VpsStatus::Down),
        VpsOutcome::Failed { target, error, .. } => {
            runtime.set(target, VpsStatus::Failed(error.clone()));
        }
    }
}

/// Profil pour le contrat (clé = chemin, jamais le contenu).
fn profile_view(profile: &VpsProfile) -> VpsProfileView {
    VpsProfileView {
        name: profile.name.clone(),
        host: profile.host.clone(),
        user: profile.user.clone(),
        port: profile.port,
        key: profile
            .key
            .as_ref()
            .map(|k| k.to_string_lossy().into_owned()),
        dir: profile.dir.clone(),
        dst: profile.dst(),
    }
}

/// Statut affichable pour `statusByName` (mots + cause d'échec).
pub fn status_text(status: &VpsStatus) -> String {
    match status {
        VpsStatus::Unknown => String::from("unknown"),
        VpsStatus::Working => String::from("working"),
        VpsStatus::Up => String::from("up"),
        VpsStatus::Down => String::from("down"),
        VpsStatus::Failed(error) => format!("failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuts_en_mots_et_transitions() {
        assert_eq!(status_text(&VpsStatus::Up), "up");
        assert!(status_text(&VpsStatus::Failed(String::from("ssh failed"))).contains("failed:"));
        let mut runtime = VpsRuntime::default();
        apply_outcome(
            &mut runtime,
            &crate::vps::VpsOutcome::Launched {
                target: String::from("r630"),
            },
        );
        assert_eq!(runtime.status_of("r630"), VpsStatus::Up);
        assert_eq!(runtime.current.as_deref(), Some("r630"));
        assert_eq!(
            outcome_target(&crate::vps::VpsOutcome::Stopped {
                target: String::from("x"),
            }),
            "x"
        );
        let view = profile_view(&crate::vps::resolve("b@h:2222", &[]).unwrap());
        assert_eq!(view.dst, "b@h:2222");
        assert_eq!(view.key, None);
    }
}
