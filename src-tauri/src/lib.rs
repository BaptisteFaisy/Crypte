//! Crypte — backend Tauri v2 + Svelte (portage du TUI `kiro-horror-tui`).
//!
//! Assemblage des tranches `back-core` (§1–§3 chat/agents/providers,
//! état `Core`, persistance), `back-tools` (§4–§6 web/outils/kiro,
//! §8–§10 hub/haunt/vps, apparition, assets) et `back-interagent`
//! (§11 mbox broadcast v2) sur les contrats `docs/tauri/commands.md` +
//! `src/lib/contracts.ts`. La tranche `back-sessions` (§1–§3 aussi)
//! n'est pas versée : doublon de `back-core` (canonique, cf. PLAN §7),
//! seul son équivalent `--smoke`/`--sheet` est repris dans [`cli`].
//!
//! Conventions : index chats 0-based côté commandes, 1-based affiché ;
//! erreurs `Result<T, String>` sans secret ; async = ACK + event.
//! Canaux : `crypte-event` (domaine) + `crypte-ui` (éphémère).

pub mod agents;
pub mod almanac;
pub mod almanac_disk;
pub mod apparition;
pub mod bestiary;
pub mod bridge;
pub mod bus;
pub mod chat_state;
pub mod cli;
pub mod clip;
pub mod cmd_apparition;
pub mod cmd_bestiary;
pub mod cmd_bus;
pub mod cmd_chat;
pub mod cmd_chat_meta;
pub mod cmd_critic;
pub mod cmd_haunt;
pub mod cmd_hub;
pub mod cmd_kiro;
pub mod cmd_mbox_core;
pub mod cmd_mbox_ops;
pub mod cmd_mbox_ordres;
pub mod cmd_mcp;
pub mod cmd_provider;
pub mod cmd_shop;
pub mod cmd_spec;
pub mod cmd_tools;
pub mod cmd_vps;
pub mod cmd_web;
pub mod compact;
pub mod core;
pub mod drain;
pub mod drain_bus;
pub mod dto;
pub mod events;
pub mod events_log;
pub mod fx;
pub mod haunt;
pub mod history;
pub mod hub;
pub mod interop;
pub mod kiro;
pub mod known;
pub mod mbox;
pub mod mbox_dto;
pub mod outbox;
pub mod paths;
pub mod permissions;
pub mod persist;
pub mod phantom;
pub mod pool;
pub mod preview;
pub mod provider;
pub mod pump;
pub mod question;
pub mod session_chat;
pub mod session_close;
pub mod sessions;
pub mod settings;
pub mod shop;
pub mod slash;
pub mod sonde_timer;
pub mod spec_runner;
pub mod state;
pub mod subagents;
pub mod tauri_chat;
pub mod tauri_more;
#[cfg(test)]
pub(crate) mod test_env;
pub mod texts;
pub mod theme;
pub mod tokens;
pub mod tools;
pub mod vault;
pub mod views;
pub mod vps;
pub mod vps_run;
pub mod web;
pub mod workspace;

pub use core::{Core, CoreChannels, CoreInner, Incoming};
pub use drain::DrainOutcome;
pub use events::{Event, EventLog};
pub use sessions::{AgentStatus, AskError, ChatSession, CloseOutcome, Sessions, SubAgent};
pub use state::{BackendOut, ToolsBackend};

/// Lance l'app Tauri : migration legacy, états gérés, boucles de fond
/// (`Incoming`, events, tick 10 Hz, sonde mbox), 93 commandes, autosave.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run(events_log: Option<std::path::PathBuf>) {
    let migrated = paths::migrate();
    if !migrated.is_empty() {
        eprintln!(
            "crypte: migrated {} legacy file(s) to com.crypte.app",
            migrated.len()
        );
    }
    let journal = events_log.or_else(|| paths::data_dir().map(|dir| dir.join("events.jsonl")));
    let (core, channels) = Core::new(
        settings::Settings::load(),
        almanac::Almanac::open_default(),
        EventLog::new(journal.clone()),
    );
    core.enable_history();
    core.emit(Event::started());
    let tools = std::sync::Mutex::new(ToolsBackend::new());
    bridge::ensure_agents(&core, &tools);
    fire_session_start(&core, &tools);
    tauri::Builder::default()
        .manage(core)
        .manage(tools)
        .setup(|app| {
            setup_tasks(app.handle().clone(), channels, journal);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            tauri_chat::chat_list,
            tauri_chat::chat_new,
            tauri_chat::chat_select,
            tauri_chat::chat_close,
            tauri_chat::chat_clear,
            tauri_chat::chat_send,
            tauri_chat::chat_stop,
            tauri_chat::chat_state,
            tauri_chat::chat_compact,
            tauri_chat::chat_set_model,
            tauri_chat::chat_set_thinking,
            tauri_chat::usage_get,
            tauri_chat::ctx_gauge,
            tauri_more::agent_spawn,
            tauri_more::agent_ask,
            tauri_more::agents_list,
            tauri_more::tell_send,
            tauri_more::msg_send,
            tauri_more::link_add,
            tauri_more::link_remove,
            tauri_more::links_list,
            tauri_more::connect_status,
            tauri_more::connect_save,
            tauri_more::connect_muse,
            tauri_more::providers_list,
            tauri_more::provider_add,
            tauri_more::provider_set_key,
            tauri_more::provider_remove,
            tauri_more::provider_select,
            tauri_more::bestiary_get,
            tauri_more::bestiary_random,
            tauri_more::bestiary_list,
            cmd_tools::tools_status,
            cmd_tools::tools_set,
            cmd_tools::tools_list,
            cmd_tools::tool_decide,
            cmd_tools::permissions_get,
            cmd_tools::permissions_set_mode,
            cmd_tools::permissions_set_rule,
            cmd_tools::permissions_reset,
            cmd_web::web_search,
            cmd_kiro::kiro_status,
            cmd_kiro::skills_list,
            cmd_kiro::skill_inject,
            cmd_kiro::memory_remember,
            cmd_kiro::checkpoints_list,
            cmd_kiro::checkpoint_rewind,
            cmd_kiro::hooks_list,
            cmd_spec::spec_new,
            cmd_spec::spec_show,
            cmd_spec::spec_run,
            cmd_spec::spec_respond,
            cmd_mcp::mcp_status,
            cmd_mcp::mcp_refresh,
            cmd_critic::critic_set,
            cmd_critic::critic_smack,
            cmd_shop::shop_state,
            cmd_shop::shop_buy,
            cmd_apparition::apparition_state,
            cmd_apparition::spook,
            cmd_hub::hub_snapshot,
            cmd_haunt::haunt_start,
            cmd_haunt::haunt_stop,
            cmd_haunt::haunt_input,
            cmd_vps::vps_list,
            cmd_vps::vps_launch,
            cmd_vps::vps_stop,
            cmd_vps::vps_status,
            cmd_vps::vps_add,
            cmd_vps::vps_remove,
            cmd_vps::vps_install,
            cmd_mbox_core::mbox_tail,
            cmd_mbox_core::mbox_send,
            cmd_mbox_core::mbox_bus,
            cmd_mbox_core::mbox_digest,
            cmd_mbox_core::mbox_rotate,
            cmd_mbox_core::mbox_hb,
            cmd_mbox_core::mbox_hb_list,
            cmd_mbox_ordres::mbox_ordre_new,
            cmd_mbox_ordres::mbox_ordre_list,
            cmd_mbox_ordres::mbox_ordre_revoke,
            cmd_mbox_ordres::mbox_ordre_ack,
            cmd_mbox_ordres::mbox_ordre_pull,
            cmd_mbox_ordres::mbox_ordre_delegate,
            cmd_mbox_ordres::mbox_jeton_new,
            cmd_mbox_ordres::mbox_jeton_revoke,
            cmd_mbox_ordres::mbox_jeton_list,
            cmd_mbox_ordres::mbox_jeton_purge,
            cmd_mbox_ops::mbox_doctor,
            cmd_mbox_ops::mbox_metriques,
            cmd_mbox_ops::mbox_piste,
            cmd_mbox_ops::mbox_vague,
            cmd_mbox_ops::mbox_fantome,
        ])
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                use tauri::Manager;
                window.state::<Core>().save_now();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Hooks `SessionStart` au boot (lignes versées, une seule fois).
fn fire_session_start(core: &Core, tools: &std::sync::Mutex<ToolsBackend>) {
    let Ok(mut tools) = tools.lock() else {
        return;
    };
    for out in tools.hooks.fire_session_start(core.lock().sessions.index()) {
        let kiro::hook_run::HookOut::ChatLine { chat, text } = out;
        tools.outbox.push(BackendOut::ChatLine { chat, text });
    }
}

/// Boucles de fond : routage `Incoming`, relais events, tick 10 Hz, sonde.
fn setup_tasks(
    handle: tauri::AppHandle,
    channels: CoreChannels,
    journal: Option<std::path::PathBuf>,
) {
    let CoreChannels {
        mut incoming,
        mut events,
    } = channels;
    tauri::async_runtime::spawn({
        let handle = handle.clone();
        async move {
            use tauri::Manager;
            while let Some(inc) = incoming.recv().await {
                let core = handle.state::<Core>();
                match core.route(inc) {
                    DrainOutcome::ToolCalls {
                        chat,
                        done,
                        elapsed,
                    } => {
                        let tools = handle.state::<std::sync::Mutex<ToolsBackend>>();
                        bridge::drain_tool_calls(&core, &tools, &handle, chat, &done, elapsed);
                    }
                    DrainOutcome::Done | DrainOutcome::Ignored => {}
                }
            }
        }
    });
    tauri::async_runtime::spawn({
        let handle = handle.clone();
        async move {
            use tauri::Manager;
            while let Some(event) = events.recv().await {
                let core = handle.state::<Core>();
                let tools = handle.state::<std::sync::Mutex<ToolsBackend>>();
                bridge::on_event(&core, &tools, &handle, &event);
                use tauri::Emitter;
                let _ = handle.emit(bridge::CRYPTE_EVENT, &event);
            }
        }
    });
    tauri::async_runtime::spawn({
        let handle = handle.clone();
        async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_millis(100));
            let mut tick: u64 = 0;
            let mut last_apparition = String::new();
            loop {
                ticker.tick().await;
                tick += 1;
                tick_once(&handle, tick, &mut last_apparition);
            }
        }
    });
    if let Some(journal) = journal {
        tauri::async_runtime::spawn(sonde_timer::spawn_sonde_timer(handle, journal));
    }
}

/// Un tick 10 Hz : autosave, sync, simus, frames UI.
fn tick_once(handle: &tauri::AppHandle, tick: u64, last_apparition: &mut String) {
    use tauri::{Emitter, Manager};
    let core = handle.state::<Core>();
    let tools = handle.state::<std::sync::Mutex<ToolsBackend>>();
    core.autosave_if_due(tick);
    bridge::sync_tick(&core, &tools);
    sync_llm(&core, &tools);
    if let Ok(mut tools) = tools.lock() {
        let outs = tools.tick_10hz();
        tools.outbox.extend(outs);
    }
    bridge::drive_outbox(&core, &tools, handle);
    if let Ok(tools) = tools.lock() {
        let _ = handle.emit(
            views::UI_CHANNEL,
            views::UiEvent::HubTick {
                snapshot: cmd_hub::snapshot_of(&tools.hub),
            },
        );
        let apparition = cmd_apparition::apparition_view(&tools);
        if let Ok(raw) = serde_json::to_string(&apparition) {
            if raw != *last_apparition {
                *last_apparition = raw;
                let _ = handle.emit(
                    views::UI_CHANNEL,
                    views::UiEvent::Apparition { state: apparition },
                );
            }
        }
        if tools.haunt_open {
            drop(tools);
            pump_haunt(handle);
        }
    };
}

/// Coud le LLM du core (synthèse `/web`, critique) dès qu'un provider
/// est connecté (impossible au setup : pas de provider avant `/connect`).
fn sync_llm(core: &Core, tools: &std::sync::Mutex<ToolsBackend>) {
    let provider = core.lock().provider.clone();
    if let (Some(provider), Ok(mut tools)) = (provider, tools.lock()) {
        tools.set_llm(provider);
    }
}

/// Pompe le jeu (100 ms) et émet la frame (`haunt_open` seulement).
fn pump_haunt(handle: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let tools = handle.state::<std::sync::Mutex<ToolsBackend>>();
    let Ok(mut tools) = tools.lock() else {
        return;
    };
    if !tools.haunt_open {
        return;
    }
    tools.haunt.pump(std::time::Duration::from_millis(100));
    let frame = cmd_haunt::frame_view(&tools.haunt.frame());
    let _ = handle.emit(views::UI_CHANNEL, views::UiEvent::HauntFrame { frame });
}
