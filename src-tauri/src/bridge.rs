//! Pont core ↔ outils : préparation d'envoi, pilote outbox, sync tick.
//!
//! - [`prepare_send`] : skill armé + préfixe hooks + `PromptSubmit` +
//!   steering/powers + schémas natifs/MCP (miroir `send.rs` du TUI).
//! - [`drive_outbox`] : applique les sorties outils/specs/web (sync
//!   inline, async en tâches qui re-pilotent à la tombée).
//! - [`sync_tick`] : notes core → outils (activité, courant, sessions,
//!   connexion, modèle) ; [`on_event`] : signaux par fait.
//! - [`drain_tool_calls`] : réponse outillée → boucle outils.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter};

use crate::bridge_spawn::{spawn_agent, spawn_exec, spawn_gen, spawn_llm};
use crate::cmd_chat::SendPrep;
use crate::core::Core;
use crate::events::Event;
use crate::state::ToolsBackend;
use crate::views::{ToolConfirmView, UiEvent, UI_CHANNEL};
use crate::BackendOut;

/// Canal des faits domaine (contrat : `crypte-event`).
pub const CRYPTE_EVENT: &str = "crypte-event";

/// Lit agents.md + steering au premier envoi (une seule tentative),
/// épingle les chats vides et annonce (absent = silencieux, comme le TUI).
pub fn ensure_agents(core: &Core, tools: &Mutex<ToolsBackend>) {
    let (tried, count) = {
        let inner = core.lock();
        (inner.agents_tried, inner.sessions.count())
    };
    if tried || count == 0 {
        return;
    }
    core.lock().agents_tried = true;
    let Ok(guard) = tools.lock() else {
        return;
    };
    let merged =
        crate::agents::load_merged_in(None, &guard.kiro_project, guard.kiro_global.as_deref());
    drop(guard);
    let msg = crate::agents::merged_message(&merged);
    let Some(text) = merged.text else {
        return;
    };
    core.set_agents_md(Some(text));
    if let Some(msg) = msg {
        let idx = core.lock().sessions.index();
        core.push_system_to(idx, msg, false);
    }
}

/// Envoi préparé : texte résolu + préfixes + schémas + annonces.
pub struct Prepared {
    /// Texte final (skill armé + préfixe hooks appliqués).
    pub text: String,
    /// Plan d'envoi core (steering/powers/schémas).
    pub prep: SendPrep,
    /// Annonces système (steering/powers/MCP, poussées si accepté).
    pub notes: Vec<String>,
}

/// Prépare un envoi : skill, hooks, steering, powers, schémas.
/// Miroir de `App::send` (TUI) sans la partie verrou core.
pub async fn prepare_send(tools: &Mutex<ToolsBackend>, index: usize, text: &str) -> Prepared {
    let mut notes = Vec::new();
    let (text, steer, powers, mcp) = {
        let mut tools = tools.lock().unwrap_or_else(|poison| poison.into_inner());
        let armed = tools.take_skill_prefix();
        let mut text = match armed {
            Some(prefix) => format!("{prefix}\n\n{text}"),
            None => text.to_string(),
        };
        text = tools.hooks.prefixed_input(&text);
        let fired = tools
            .hooks
            .fire(crate::kiro::hooks::Trigger::PromptSubmit, &text, index);
        for out in fired {
            let crate::kiro::hook_run::HookOut::ChatLine { chat, text } = out;
            tools.outbox.push(BackendOut::ChatLine { chat, text });
        }
        let steer = steer_blocks(&tools, &text, &mut notes);
        let powers = power_blocks(&tools, &text, &mut notes);
        let mcp = tools.mcp.clone();
        let on = tools.tools_on;
        (text, steer, powers, if on { Some(mcp) } else { None })
    };
    let defs = match mcp {
        None => None,
        Some(mcp) => {
            let defs = mcp.tooled_defs().await;
            let json: Vec<serde_json::Value> = defs
                .iter()
                .map(|def| serde_json::to_value(def).unwrap_or(serde_json::Value::Null))
                .collect();
            Some(json)
        }
    };
    Prepared {
        text,
        prep: SendPrep {
            steer,
            powers,
            defs,
        },
        notes,
    }
}

/// Blocs steering `auto` matchant le prompt (+ annonce).
fn steer_blocks(
    tools: &ToolsBackend,
    text: &str,
    notes: &mut Vec<String>,
) -> Vec<(String, String)> {
    let files =
        crate::kiro::steering::load_all_in(&tools.kiro_project, tools.kiro_global.as_deref());
    let blocks: Vec<(String, String)> = crate::kiro::steering::auto_for(&files, text)
        .into_iter()
        .map(|file| (file.name.clone(), file.body.clone()))
        .collect();
    if !blocks.is_empty() {
        let names: Vec<&str> = blocks.iter().map(|(name, _)| name.as_str()).collect();
        notes.push(format!("steering auto: {}", names.join(", ")));
    }
    blocks
}

/// Skills des powers matchant le prompt (+ activation MCP, annoncée).
fn power_blocks(
    tools: &ToolsBackend,
    text: &str,
    notes: &mut Vec<String>,
) -> Vec<(String, String)> {
    let powers = crate::kiro::powers::load_all(tools.kiro_global.as_deref());
    let matched: Vec<_> = powers
        .into_iter()
        .filter(|power| crate::kiro::powers::matches(power, text))
        .collect();
    if matched.is_empty() {
        return Vec::new();
    }
    let tagged: Vec<(String, String)> = matched
        .iter()
        .flat_map(|power| {
            crate::kiro::powers::activation_blocks(std::slice::from_ref(power), text)
                .into_iter()
                .map(|(name, skill, body)| (format!("{name}/{skill}"), body))
        })
        .collect();
    let mut names: Vec<&str> = matched.iter().map(|p| p.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    notes.push(format!("powers: {}", names.join(", ")));
    for power in &matched {
        activate_power_mcp(tools, power, notes);
    }
    tagged
}

/// Active le MCP d'un power matché (ajout parc, annoncé une fois).
fn activate_power_mcp(
    tools: &ToolsBackend,
    power: &crate::kiro::powers::Power,
    notes: &mut Vec<String>,
) {
    use crate::kiro::mcp_pool::power::PowerAdd;
    if power.mcp_servers.is_empty() {
        return;
    }
    let PowerAdd::Added(added) = tools.mcp.try_add_power(power) else {
        return;
    };
    let mut line = format!("power {}: mcp {}", power.name, added.fresh.join(", "));
    if added.fresh.is_empty() {
        line = format!("power {}: mcp already configured", power.name);
    }
    if !added.dupes.is_empty() {
        line.push_str(&format!(
            " (already configured: {})",
            added.dupes.join(", ")
        ));
    }
    notes.push(line);
}

/// Réponse outillée : ledger (parité TUI) + boucle outils + pilotage.
pub fn drain_tool_calls(
    core: &Core,
    tools: &Mutex<ToolsBackend>,
    app: &AppHandle,
    chat: usize,
    done: &crate::provider::Completion,
    elapsed: f64,
) {
    core.ledger_add(chat, done.prompt_tokens, done.completion_tokens, elapsed);
    let history = core.send_window_of(chat);
    if let Ok(mut tools) = tools.lock() {
        tools.tool_route(chat, history, done);
    }
    drive_outbox(core, tools, app);
}

/// Applique les sorties en attente (sync inline, async en tâches).
pub fn drive_outbox(core: &Core, tools: &Mutex<ToolsBackend>, app: &AppHandle) {
    let outs = match tools.lock() {
        Ok(mut tools) => tools.take_outbox(),
        Err(_) => return,
    };
    for out in outs {
        drive_one(core, tools, app, out);
    }
}

/// Applique une sortie (les `Spawn*` partent en tâches détachées).
fn drive_one(core: &Core, tools: &Mutex<ToolsBackend>, app: &AppHandle, out: BackendOut) {
    match out {
        BackendOut::ChatLine { chat, text } => core.push_system_to(chat, text, true),
        BackendOut::DisplayText { chat, text } => core.push_system_to(chat, text, false),
        BackendOut::SetSending { chat, sending } => core.set_sending(chat, sending),
        BackendOut::Confirm { chat, call } => {
            let _ = app.emit(
                UI_CHANNEL,
                UiEvent::ToolConfirm {
                    request: ToolConfirmView { chat, call },
                },
            );
        }
        BackendOut::SpawnExec { chat, call } => spawn_exec(app.clone(), chat, call),
        BackendOut::SpawnLlm { chat, wire } => spawn_llm(app.clone(), chat, wire),
        BackendOut::Delivered { chat, delivery } => {
            let _ = app.emit(UI_CHANNEL, UiEvent::ToolDelivery { chat, delivery });
        }
        BackendOut::Finalize { chat } => {
            if let Ok(mut tools) = tools.lock() {
                tools.tool_clear(chat);
            }
        }
        BackendOut::LedgerAdd {
            chat,
            prompt,
            completion,
        } => {
            core.ledger_add(chat, prompt, completion, 0.0);
            core.almanac_record(prompt, completion);
        }
        BackendOut::SpawnGen { chat, gen } => spawn_gen(app.clone(), chat, gen),
        BackendOut::SpawnAgent {
            chat,
            name,
            index,
            total,
            label,
            prompt,
            task,
        } => {
            spawn_agent(app.clone(), chat, name, index, total, label, prompt, task);
        }
        BackendOut::AskConfirm { .. } => {
            // La ligne `…? (y/n)` accompagne déjà (ChatLine) ; le frontend
            // suit la ligne + `spec_respond` (pas d'event contrat).
        }
        BackendOut::Progress {
            name,
            label,
            done,
            total,
        } => {
            let _ = app.emit(
                UI_CHANNEL,
                UiEvent::SpecProgress {
                    name,
                    label,
                    done,
                    total,
                },
            );
        }
        BackendOut::WebDone {
            chat,
            backend,
            text,
            prompt,
            completion,
        } => {
            core.ledger_add(chat, prompt, completion, 0.0);
            if (prompt, completion) != (0, 0) {
                core.almanac_record(prompt, completion);
            }
            core.emit(Event::web_answer(chat, &backend, &text));
        }
        BackendOut::Critique { text } => {
            let _ = app.emit(UI_CHANNEL, UiEvent::Critic { text });
        }
    }
}

/// Signaux par fait : specs, présages, apparition, hub.
pub fn on_event(core: &Core, tools: &Mutex<ToolsBackend>, app: &AppHandle, event: &Event) {
    match event {
        Event::AgentDone { agent, .. } => {
            if let Ok(mut tools) = tools.lock() {
                tools.spec_agent_done_cmd(*agent);
            }
            drive_outbox(core, tools, app);
        }
        Event::AgentFailed { agent, .. } => {
            if let Ok(mut tools) = tools.lock() {
                tools.spec_agent_failed_cmd(*agent);
            }
            drive_outbox(core, tools, app);
        }
        Event::AssistantMsg { chat, .. } => {
            if let Ok(mut tools) = tools.lock() {
                tools.apparition.advance();
                tools.hub.signal(crate::hub::Signal::Reply {
                    chat: chat.saturating_sub(1),
                });
            }
        }
        Event::AssistantError { .. } => {
            if let Ok(mut tools) = tools.lock() {
                tools.note_omen("error");
            }
        }
        Event::Compacted { .. } => {
            if let Ok(mut tools) = tools.lock() {
                tools.note_omen("compact");
            }
        }
        _ => {}
    }
}

/// Sync 10 Hz core → outils (activité, courant, sessions, connexion, modèle).
pub fn sync_tick(core: &Core, tools: &Mutex<ToolsBackend>) {
    let inner = core.lock();
    let sending = (0..inner.sessions.count())
        .filter_map(|i| inner.sessions.get(i))
        .any(|sess| sess.sending);
    let agents: usize = (0..inner.sessions.count())
        .filter_map(|i| inner.sessions.get(i))
        .map(|sess| {
            sess.agents
                .iter()
                .filter(|a| a.status == crate::AgentStatus::Running)
                .count()
        })
        .sum();
    let index = inner.sessions.index();
    let connected = inner.provider.is_some();
    let model = inner.effective_model_for(index);
    let effort = inner.thinking.clone();
    let view = sync_view(&inner);
    drop(inner);
    let Ok(mut tools) = tools.lock() else {
        return;
    };
    tools.note_chat_activity(sending, agents);
    tools.note_current_chat(index);
    tools.note_sessions(view);
    tools.note_connected(connected);
    tools.note_model(&model, effort.as_deref());
}

/// Vue sessions pour le hub (esprits + humeurs).
fn sync_view(inner: &crate::core::CoreInner) -> crate::hub::SyncView {
    use crate::hub::{AgentStatus, AgentSync, ChatSync, SyncView};
    let chats = (0..inner.sessions.count())
        .filter_map(|i| inner.sessions.get(i))
        .map(|sess| ChatSync {
            sending: sess.sending,
            agents: sess
                .agents
                .iter()
                .map(|a| AgentSync {
                    id: a.id,
                    status: match a.status {
                        crate::AgentStatus::Running => AgentStatus::Running,
                        crate::AgentStatus::Done => AgentStatus::Done,
                        crate::AgentStatus::Failed => AgentStatus::Failed,
                    },
                })
                .collect(),
        })
        .collect();
    SyncView { chats }
}

/// Sérialise des messages/schémas (`Null` si insérialisable, jamais panic).
pub(crate) fn json_values<T: serde::Serialize>(items: &[T]) -> Vec<serde_json::Value> {
    items
        .iter()
        .map(|item| serde_json::to_value(item).unwrap_or(serde_json::Value::Null))
        .collect()
}
