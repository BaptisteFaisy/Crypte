//! Adaptateurs Tauri des commandes chat §1 (noms exacts du contrat).
//!
//! Chaque adaptateur verrouille le `State<Core>` et délègue à la méthode
//! `Core`. `chat_send` passe par le pont (`prepare_send` : skill, hooks,
//! steering, powers, schémas) ; `chat_stop` annule aussi la boucle outil.

use std::sync::Mutex;

use crate::bridge;
use crate::core::Core;
use crate::dto::{ChatStateDto, ChatSummary, CompactResult, CtxGauge, SendAck, UsageReport};
use crate::state::ToolsBackend;

/// `chat_list` : résumés des chats ouverts.
#[tauri::command(rename_all = "camelCase")]
pub async fn chat_list(core: tauri::State<'_, Core>) -> Result<Vec<ChatSummary>, String> {
    Ok(core.chat_list())
}

/// `chat_new` : nouveau chat.
#[tauri::command(rename_all = "camelCase")]
pub async fn chat_new(core: tauri::State<'_, Core>) -> Result<ChatSummary, String> {
    Ok(core.chat_new())
}

/// `chat_select` : chat courant + état complet.
#[tauri::command(rename_all = "camelCase")]
pub async fn chat_select(
    core: tauri::State<'_, Core>,
    index: usize,
) -> Result<ChatStateDto, String> {
    core.chat_select(index)
}

/// `chat_close` : ferme + rend la liste.
#[tauri::command(rename_all = "camelCase")]
pub async fn chat_close(
    core: tauri::State<'_, Core>,
    index: usize,
) -> Result<Vec<ChatSummary>, String> {
    core.chat_close(index)
}

/// `chat_clear` : vide la conversation.
#[tauri::command(rename_all = "camelCase")]
pub async fn chat_clear(
    core: tauri::State<'_, Core>,
    index: usize,
) -> Result<ChatStateDto, String> {
    core.chat_clear(index)
}

/// `chat_send` : prépare (pont), enfile + spawne ; ACK immédiat, suite
/// par `crypte-event`. Les annonces steering/powers partent si accepté,
/// Kiro juge le prompt en parallèle (bulle par event `critic`).
#[tauri::command(rename_all = "camelCase")]
pub async fn chat_send(
    core: tauri::State<'_, Core>,
    tools: tauri::State<'_, Mutex<ToolsBackend>>,
    index: usize,
    text: String,
) -> Result<SendAck, String> {
    bridge::ensure_agents(&core, &tools);
    let prepared = bridge::prepare_send(&tools, index, &text).await;
    let ack = core
        .chat_send_tooled(index, prepared.text.clone(), prepared.prep)
        .await;
    if ack.accepted {
        for note in prepared.notes {
            core.push_system_to(index, note, false);
        }
        let model = core.lock().effective_model_for(index);
        if let Ok(mut tools) = tools.lock() {
            tools.note_user_send(&prepared.text, &model);
        }
    }
    Ok(ack)
}

/// `chat_stop` : stoppe le vol (+ boucle outil du chat, comme Ctrl+C).
#[tauri::command(rename_all = "camelCase")]
pub async fn chat_stop(
    core: tauri::State<'_, Core>,
    tools: tauri::State<'_, Mutex<ToolsBackend>>,
    index: usize,
) -> Result<bool, String> {
    let stopped = core.chat_stop(index);
    if stopped {
        if let Ok(mut tools) = tools.lock() {
            tools.tool_clear(index);
        }
    }
    Ok(stopped)
}

/// `chat_state` : état complet (refresh après event).
#[tauri::command(rename_all = "camelCase")]
pub async fn chat_state(
    core: tauri::State<'_, Core>,
    index: usize,
) -> Result<ChatStateDto, String> {
    core.chat_state(index)
}

/// `chat_compact` : résume, rend `{before, after}`.
#[tauri::command(rename_all = "camelCase")]
pub async fn chat_compact(
    core: tauri::State<'_, Core>,
    index: usize,
) -> Result<CompactResult, String> {
    core.chat_compact(index).await
}

/// `chat_set_model` : modèle du chat, rend l'effectif.
#[tauri::command(rename_all = "camelCase")]
pub async fn chat_set_model(
    core: tauri::State<'_, Core>,
    index: usize,
    model: Option<String>,
) -> Result<String, String> {
    core.chat_set_model(index, model)
}

/// `chat_set_thinking` : effort global, rend l'effectif.
#[tauri::command(rename_all = "camelCase")]
pub async fn chat_set_thinking(
    core: tauri::State<'_, Core>,
    level: Option<String>,
) -> Result<String, String> {
    core.chat_set_thinking(level)
}

/// `usage_get` : tokens chat + session + almanach.
#[tauri::command(rename_all = "camelCase")]
pub async fn usage_get(core: tauri::State<'_, Core>, index: usize) -> Result<UsageReport, String> {
    core.usage_get(index)
}

/// `ctx_gauge` : `{used, window}`.
#[tauri::command(rename_all = "camelCase")]
pub async fn ctx_gauge(core: tauri::State<'_, Core>, index: usize) -> Result<CtxGauge, String> {
    core.ctx_gauge(index)
}
