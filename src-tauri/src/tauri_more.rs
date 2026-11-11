//! Adaptateurs Tauri des commandes bus §2, providers §3, bestiaire §7.
//!
//! Noms exacts du contrat, délégation aux méthodes `Core` (le tick
//! 10 Hz synchronise le pont : connexion, modèle, sessions).

use crate::core::Core;
use crate::dto::{
    BusStateDto, ConnectResult, ConnectStatus, CreatureDto, CreatureEntry, LinkEndDto,
    ProviderInfo, SubAgentInfo,
};

/// `agent_spawn` : sous-agent, réponse par `crypte-event`.
#[tauri::command(rename_all = "camelCase")]
pub async fn agent_spawn(
    core: tauri::State<'_, Core>,
    prompt: String,
    target_chat: Option<usize>,
) -> Result<SubAgentInfo, String> {
    core.agent_spawn(prompt, target_chat).await
}

/// `agent_ask` : suite de conversation (enfilée si occupé).
#[tauri::command(rename_all = "camelCase")]
pub async fn agent_ask(
    core: tauri::State<'_, Core>,
    agent_id: usize,
    text: String,
) -> Result<(), String> {
    core.agent_ask(agent_id, text)
}

/// `agents_list` : sous-agents (courant, ou tous si `all`).
#[tauri::command(rename_all = "camelCase")]
pub async fn agents_list(
    core: tauri::State<'_, Core>,
    all: bool,
) -> Result<Vec<SubAgentInfo>, String> {
    Ok(core.agents_list(all))
}

/// `tell_send` : injecte + déclenche le chat cible.
#[tauri::command(rename_all = "camelCase")]
pub async fn tell_send(
    core: tauri::State<'_, Core>,
    target_chat: usize,
    text: String,
) -> Result<(), String> {
    core.tell_send(target_chat, text)
}

/// `msg_send` : `n`/`@n` chat, `#id` agent, `all` éventail.
#[tauri::command(rename_all = "camelCase")]
pub async fn msg_send(
    core: tauri::State<'_, Core>,
    target: String,
    text: String,
) -> Result<(), String> {
    core.msg_send(target, text)
}

/// `link_add` : retransmission auto `from` → `to`.
#[tauri::command(rename_all = "camelCase")]
pub async fn link_add(
    core: tauri::State<'_, Core>,
    from: LinkEndDto,
    to: LinkEndDto,
) -> Result<(), String> {
    core.link_add(from, to)
}

/// `link_remove` : retire le lien (faux s'il n'existait pas).
#[tauri::command(rename_all = "camelCase")]
pub async fn link_remove(
    core: tauri::State<'_, Core>,
    from: LinkEndDto,
    to: LinkEndDto,
) -> Result<bool, String> {
    Ok(core.link_remove(from, to))
}

/// `links_list` : liens + compteurs.
#[tauri::command(rename_all = "camelCase")]
pub async fn links_list(core: tauri::State<'_, Core>) -> Result<BusStateDto, String> {
    Ok(core.links_list())
}

/// `connect_status` : état de connexion.
#[tauri::command(rename_all = "camelCase")]
pub async fn connect_status(core: tauri::State<'_, Core>) -> Result<ConnectStatus, String> {
    Ok(core.connect_status())
}

/// `connect_save` : connecte `{provider, key, model?}`, persiste chiffré.
#[tauri::command(rename_all = "camelCase")]
pub async fn connect_save(
    core: tauri::State<'_, Core>,
    provider: String,
    key: String,
    model: Option<String>,
) -> Result<ConnectResult, String> {
    Ok(core.connect_save(provider, key, model))
}

/// `connect_muse` : coffre, sinon store Muse, sinon saisie manuelle.
#[tauri::command(rename_all = "camelCase")]
pub async fn connect_muse(core: tauri::State<'_, Core>) -> Result<ConnectResult, String> {
    Ok(core.connect_muse())
}

/// `providers_list` : connus + customs (jamais les clés).
#[tauri::command(rename_all = "camelCase")]
pub async fn providers_list(core: tauri::State<'_, Core>) -> Result<Vec<ProviderInfo>, String> {
    core.providers_list()
}

/// `provider_add` : ajoute (ou met à jour) un custom.
#[tauri::command(rename_all = "camelCase")]
pub async fn provider_add(
    core: tauri::State<'_, Core>,
    name: String,
    base_url: String,
    model: Option<String>,
) -> Result<(), String> {
    core.provider_add(name, base_url, model)
}

/// `provider_set_key` : stocke la clé (chiffré).
#[tauri::command(rename_all = "camelCase")]
pub async fn provider_set_key(
    core: tauri::State<'_, Core>,
    name: String,
    key: String,
) -> Result<(), String> {
    core.provider_set_key(name, key)
}

/// `provider_remove` : retire un custom (connus intouchables).
#[tauri::command(rename_all = "camelCase")]
pub async fn provider_remove(core: tauri::State<'_, Core>, name: String) -> Result<(), String> {
    core.provider_remove(name)
}

/// `provider_select` : provider actif (clé requise).
#[tauri::command(rename_all = "camelCase")]
pub async fn provider_select(
    core: tauri::State<'_, Core>,
    name: String,
) -> Result<ConnectStatus, String> {
    core.provider_select(name)
}

/// `bestiary_get` : créature par slug, grille 32×32.
#[tauri::command(rename_all = "camelCase")]
pub async fn bestiary_get(
    core: tauri::State<'_, Core>,
    slug: String,
) -> Result<CreatureDto, String> {
    core.bestiary_get(slug)
}

/// `bestiary_random` : créature au hasard (`/bestiary`).
#[tauri::command(rename_all = "camelCase")]
pub async fn bestiary_random(core: tauri::State<'_, Core>) -> Result<CreatureDto, String> {
    Ok(core.bestiary_random())
}

/// `bestiary_list` : catalogue des 33 (sans grilles).
#[tauri::command(rename_all = "camelCase")]
pub async fn bestiary_list(core: tauri::State<'_, Core>) -> Result<Vec<CreatureEntry>, String> {
    Ok(core.bestiary_list())
}
