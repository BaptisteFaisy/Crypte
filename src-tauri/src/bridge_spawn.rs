//! Tâches async du pont : exécutions d'outils, relances LLM, specs.
//!
//! Extrait de `bridge.rs` (budget 500 lignes) : `spawn_exec` (natif ou
//! `mcp.*`, journalisé, livré), `spawn_llm` (relance outillée vers
//! `Incoming`), `spawn_gen` (étapes `/spec new`), `spawn_agent`
//! (tâches `/spec run`). Chaque tâche re-pilote l'outbox à la tombée.

use std::sync::Mutex;

use tauri::{AppHandle, Manager};

use crate::core::Core;
use crate::state::ToolsBackend;

/// Exécute un appel (natif ou `mcp.*`), journalise, livre, re-pilote.
pub(crate) fn spawn_exec(app: AppHandle, chat: usize, call: crate::tools::wire::ToolCall) {
    space_tool_events(&app, &call);
    tokio::spawn(async move {
        let tools_state = app.state::<Mutex<ToolsBackend>>();
        let mcp = match tools_state.lock() {
            Ok(tools) => tools.mcp.clone(),
            Err(_) => return,
        };
        let started = std::time::Instant::now();
        let out = if crate::kiro::mcp_client::is_mcp_name(&call.name) {
            mcp.call(chat, &call.name, &call.args).await
        } else {
            crate::tools::exec::run_tool(chat, &call.name, &call.args).await
        };
        let ms = started.elapsed().as_millis().min(9_999_999) as u64;
        crate::tools::log::append(
            chat,
            &call.name,
            out.ok,
            ms,
            call.args.len(),
            out.text.len(),
        );
        let delivery = crate::tools::flow::Delivery::from_outcome(call, out, ms);
        if let Ok(mut tools) = tools_state.lock() {
            tools.tool_exec_done(chat, vec![delivery]);
        }
        let core_state = app.state::<Core>();
        crate::bridge::drive_outbox(&core_state, &tools_state, &app);
    });
}

/// Relance LLM outillée : réinjecte en `Incoming` (route → drain).
pub(crate) fn spawn_llm(app: AppHandle, chat: usize, wire: Vec<crate::tools::wire::WireMsg>) {
    let core_state = app.state::<Core>();
    let tools_state = app.state::<Mutex<ToolsBackend>>();
    let (provider, model, effort, pool, mcp) = {
        let inner = core_state.lock();
        let mcp = match tools_state.lock() {
            Ok(tools) => tools.mcp.clone(),
            Err(_) => return,
        };
        (
            inner.provider.clone(),
            inner.effective_model_for(chat),
            inner.thinking.clone(),
            inner.pool.clone(),
            mcp,
        )
    };
    let Some(provider) = provider else {
        if let Ok(mut tools) = tools_state.lock() {
            tools.tool_abort("error: no provider — loop aborted");
        }
        crate::bridge::drive_outbox(&core_state, &tools_state, &app);
        return;
    };
    pool.spawn(async move {
        let core_state = app.state::<Core>();
        let tools_state = app.state::<Mutex<ToolsBackend>>();
        let defs = mcp.tooled_defs().await;
        let wire_json = crate::bridge::json_values(&wire);
        let def_json = crate::bridge::json_values(&defs);
        let reply = provider
            .complete_json(&wire_json, &def_json, &model, effort.as_deref())
            .await;
        if reply.is_err() {
            // Parité TUI (`drain_chat` Err oublie la boucle, silencieux).
            if let Ok(mut tools) = tools_state.lock() {
                tools.tool_clear(chat);
            }
        }
        let epoch = core_state.epoch_of(chat);
        let tx = core_state.lock().tx.clone();
        let _ = tx.send(crate::Incoming::Chat(chat, epoch, reply)).await;
    });
}

/// Étapes `/spec new` : complète, rend au runner, re-pilote.
pub(crate) fn spawn_gen(app: AppHandle, chat: usize, gen: crate::kiro::spec_run::SpecGen) {
    tokio::spawn(async move {
        let core_state = app.state::<Core>();
        let tools_state = app.state::<Mutex<ToolsBackend>>();
        let (provider, model, effort) = {
            let inner = core_state.lock();
            (
                inner.provider.clone(),
                inner.effective_model_for(chat),
                inner.thinking.clone(),
            )
        };
        let reply = match provider {
            Some(provider) => {
                provider
                    .complete_full(&gen.messages(), &model, effort.as_deref())
                    .await
            }
            None => Err(anyhow::anyhow!("not connected")),
        };
        if let Ok(mut tools) = tools_state.lock() {
            tools.spec_gen_reply_cmd(chat, gen, reply);
        }
        crate::bridge::drive_outbox(&core_state, &tools_state, &app);
    });
}

/// Tâches `/spec run` : spawne l'agent réel, enregistre, re-pilote.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_agent(
    app: AppHandle,
    chat: usize,
    name: String,
    index: usize,
    total: usize,
    label: String,
    prompt: String,
    task: String,
) {
    tokio::spawn(async move {
        let core_state = app.state::<Core>();
        let tools_state = app.state::<Mutex<ToolsBackend>>();
        match core_state.agent_spawn(prompt, Some(chat)).await {
            Ok(info) => {
                if let Ok(mut tools) = tools_state.lock() {
                    tools.spec_agent_started_cmd(&name, chat, index, total, info.id, &task);
                }
            }
            Err(err) => core_state.push_system_to(
                chat,
                format!("spec {name} [{label}]: cannot spawn agent: {err}"),
                false,
            ),
        }
        crate::bridge::drive_outbox(&core_state, &tools_state, &app);
    });
}

/// Émet l'étape Act puis les lectures/écritures devinées dans une commande
/// shell (il n'existe pas d'outil fichier typé) vers le monde 3D.
fn space_tool_events(app: &AppHandle, call: &crate::tools::wire::ToolCall) {
    use crate::space::events::{
        read_paths_from_shell, shell_command, write_paths_from_shell, AgentRef, SceneEvent, Step,
    };
    use tauri::Manager;
    if call.name != "shell" {
        return;
    }
    let log = app
        .state::<std::sync::Mutex<crate::state::ToolsBackend>>()
        .lock()
        .map(|t| t.space.clone())
        .unwrap_or_else(|p| p.into_inner().space.clone());
    let cmd = shell_command(&call.args);
    log.emit(&SceneEvent::StepStart { step: Step::Act });
    for path in read_paths_from_shell(&cmd) {
        log.emit(&SceneEvent::FileRead {
            path,
            agent: AgentRef::Main,
        });
    }
    for path in write_paths_from_shell(&cmd) {
        log.emit(&SceneEvent::FileWrite {
            path,
            agent: AgentRef::Main,
            added: 0,
            removed: 0,
        });
    }
}
