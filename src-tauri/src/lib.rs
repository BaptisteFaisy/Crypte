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
pub mod almanac_usage;
pub mod apparition;
pub mod bestiary;
pub mod bridge;
pub(crate) mod bridge_spawn;
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
pub mod invoke;
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
pub mod provider_wire;
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
        .invoke_handler(invoke::dispatch)
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
    let route = handle.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(inc) = incoming.recv().await {
            route_one(&route, inc).await;
        }
    });
    let relay = handle.clone();
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        while let Some(event) = events.recv().await {
            let core = relay.state::<Core>();
            let tools = relay.state::<std::sync::Mutex<ToolsBackend>>();
            bridge::on_event(&core, &tools, &relay, &event);
            use tauri::Emitter;
            let _ = relay.emit(bridge::CRYPTE_EVENT, &event);
        }
    });
    spawn_tick(handle.clone());
    if let Some(journal) = journal {
        tauri::async_runtime::spawn(sonde_timer::spawn_sonde_timer(handle, journal));
    }
}

/// Route un `Incoming` (boucle outils si appels d'outils).
async fn route_one(handle: &tauri::AppHandle, inc: Incoming) {
    use tauri::Manager;
    let core = handle.state::<Core>();
    if let DrainOutcome::ToolCalls {
        chat,
        done,
        elapsed,
    } = core.route(inc)
    {
        let tools = handle.state::<std::sync::Mutex<ToolsBackend>>();
        bridge::drain_tool_calls(&core, &tools, handle, chat, &done, elapsed);
    }
}

/// Boucle du tick 10 Hz (autosave, sync, simus, frames UI).
fn spawn_tick(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(100));
        let mut tick: u64 = 0;
        let mut last_apparition = String::new();
        loop {
            ticker.tick().await;
            tick += 1;
            tick_once(&handle, tick, &mut last_apparition);
        }
    });
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
