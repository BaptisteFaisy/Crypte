//! CLI headless : `--help`, `--smoke`, `--sheet`, `--apercu`, `--mbox`.
//!
//! Équivalents Tauri des sorties précoces du TUI (`main.rs`) : mêmes
//! noms, mêmes effets. `--smoke` est hermétique (dossiers temporaires,
//! sans réseau ni clé) ; `--sheet`/`--apercu` écrivent le fichier
//! demandé ; `--mbox` rejoue le CLI broadcast (interop `.scratch`).
//! `--events-log` ne sort pas : il arrose le journal JSONL du GUI.

use std::path::PathBuf;

/// Arguments précoces reconnus avant le boot GUI.
#[derive(Debug, Default)]
pub struct EarlyArgs {
    /// `--help` : usage sur stdout.
    pub help: bool,
    /// `--smoke` : auto-contrôle headless.
    pub smoke: bool,
    /// `--sheet <png>` : planche bestiaire en PNG.
    pub sheet: Option<String>,
    /// `--apercu <fichier>` : rapport JSON de prévisualisation.
    pub apercu: Option<String>,
    /// `--mbox <args…>` : CLI broadcast (tout le reste d'argv).
    pub mbox_args: Option<Vec<String>>,
    /// `--events-log <fichier>` : journal JSONL (`KH_EVENTS_LOG` sinon).
    pub events_log: Option<String>,
}

/// Usage (miroir du `--help` TUI, adapté desktop).
pub const HELP: &str = "crypte (Tauri) — chat LLM d'horreur\n\nUsage: crypte [options]\n\n  --smoke             Headless self-check (no network, no key)\n  --sheet <png>       Write the bestiary sheet (33 drawings) as PNG\n  --apercu <fichier>  Write the preview report as JSON\n  --mbox <args...>    Broadcast CLI (same files as bcast shell)\n  --events-log <file> Append domain events as JSONL (KH_EVENTS_LOG)\n  --help              Show this help\n";

/// Parse `argv` (sans le programme). Inconnu → `Err` (usage + code 2).
pub fn parse_early(argv: &[String]) -> Result<EarlyArgs, String> {
    let mut args = EarlyArgs::default();
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--help" | "-h" => args.help = true,
            "--smoke" => args.smoke = true,
            "--sheet" => {
                i += 1;
                args.sheet = Some(take(argv, &mut i, "--sheet")?);
            }
            "--apercu" => {
                i += 1;
                args.apercu = Some(take(argv, &mut i, "--apercu")?);
            }
            "--mbox" => {
                args.mbox_args = Some(argv[i + 1..].to_vec());
                break;
            }
            "--events-log" => {
                i += 1;
                args.events_log = Some(take(argv, &mut i, "--events-log")?);
            }
            other => return Err(format!("unknown flag: {other}\n{HELP}")),
        }
        i += 1;
    }
    Ok(args)
}

/// Valeur d'un flag à argument (`Err` si absent).
fn take(argv: &[String], i: &mut usize, flag: &str) -> Result<String, String> {
    argv.get(*i)
        .cloned()
        .ok_or_else(|| format!("{flag} needs a value\n{HELP}"))
}

/// Exécute les sorties précoces. Rend le code de sortie quand le
/// process doit s'arrêter, `None` pour continuer vers le GUI.
pub fn run_early(args: &EarlyArgs) -> Option<i32> {
    if args.help {
        print!("{HELP}");
        return Some(0);
    }
    if args.smoke {
        return match smoke() {
            Ok(line) => {
                println!("{line}");
                Some(0)
            }
            Err(err) => {
                eprintln!("smoke FAILED: {err:#}");
                Some(1)
            }
        };
    }
    if let Some(path) = &args.sheet {
        return match crate::bestiary::write_sheet(path) {
            Ok(()) => {
                println!("bestiary sheet: {path}");
                Some(0)
            }
            Err(err) => {
                eprintln!("sheet FAILED: {err:#}");
                Some(1)
            }
        };
    }
    if let Some(path) = &args.apercu {
        return match crate::preview::write_preview(path) {
            Ok(report) => {
                println!("apercu: {path} ({})", crate::preview::summary(&report));
                Some(0)
            }
            Err(err) => {
                eprintln!("apercu FAILED: {err:#}");
                Some(1)
            }
        };
    }
    if let Some(argv) = &args.mbox_args {
        let dir = crate::mbox::format::BcastDir::defaut();
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        return Some(crate::interop::run_cli(dir.racine(), &refs));
    }
    None
}

/// Auto-contrôle headless : chats, bestiaire, boutique, simus, events.
/// Hermétique (tmp seul), sans réseau ni clé. Rend la ligne `smoke OK`.
fn smoke() -> anyhow::Result<String> {
    let dir = std::env::temp_dir().join(format!("crypte-smoke-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir)?;
    let mut checks = 0;

    // Chats : refus déconnecté, lignes système, état sérialisé.
    let settings =
        crate::settings::Settings::with_store(crate::settings::Store::new(dir.join("store")));
    let (core, _channels) = crate::core::Core::new(
        settings,
        crate::almanac::Almanac::memory(),
        crate::events::EventLog::disabled(),
    );
    core.set_agents_md(Some(String::from("smoke context")));
    let at = core.add_chat();
    checks += 1;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let ack = rt.block_on(core.chat_send(at, String::from("boo")));
    anyhow::ensure!(!ack.accepted, "send without provider must refuse");
    checks += 1;
    core.push_system_to(
        at,
        String::from("smoke: the CI ghost, and it bleeds."),
        true,
    );
    let state = core.chat_state(at).map_err(|e| anyhow::anyhow!(e))?;
    let raw = serde_json::to_value(&state)?;
    anyhow::ensure!(
        raw["messages"].as_array().is_some_and(|m| m.len() >= 2),
        "chat state"
    );
    checks += 1;

    // Bestiaire + boutique + simus (aucun réseau).
    let creature = crate::bestiary::find("citrouille");
    anyhow::ensure!(creature.is_some(), "bestiary citrouille");
    let mut tools = crate::state::ToolsBackend::new_in(&dir);
    tools.note_connected(false);
    tools.note_sessions(crate::hub::SyncView {
        chats: vec![crate::hub::ChatSync {
            sending: true,
            agents: vec![crate::hub::AgentSync {
                id: 1,
                status: crate::hub::AgentStatus::Running,
            }],
        }],
    });
    tools.tick_10hz();
    let snap = crate::cmd_hub::snapshot_of(&tools.hub);
    anyhow::ensure!(snap.spirits.len() >= 2, "hub spirits");
    let apparition = crate::cmd_apparition::apparition_view(&tools);
    anyhow::ensure!(!apparition.creature.grid.is_empty(), "apparition grid");
    checks += 2;

    // Almanach démo + bloc usage + events sérialisés.
    let demo = crate::almanac::Almanac::demo(crate::almanac::today());
    anyhow::ensure!(
        demo.usage_block(crate::almanac::today())
            .contains("ALMANAC"),
        "usage block"
    );
    let event = crate::events::Event::assistant_msg(at, &smoke_done());
    let raw = serde_json::to_value(&event)?;
    anyhow::ensure!(raw["type"] == "assistant_msg", "event tag");
    anyhow::ensure!(raw["chat"] == at as u64 + 1, "event chat 1-based");
    checks += 2;

    std::fs::remove_dir_all(&dir).ok();
    Ok(format!(
        "smoke OK: {checks} checks, chats+bestiary+shop+sims+events"
    ))
}

/// Réponse factice (tokens 1/1, sans appels d'outils).
fn smoke_done() -> crate::provider::Completion {
    crate::provider::Completion {
        text: String::from("boo"),
        tool_calls: Vec::new(),
        prompt_tokens: 1,
        completion_tokens: 1,
    }
}

/// Journal JSONL : `--events-log` prime, sinon `KH_EVENTS_LOG`.
pub fn events_path(args: &EarlyArgs) -> Option<PathBuf> {
    if let Some(path) = &args.events_log {
        return Some(PathBuf::from(path));
    }
    std::env::var("KH_EVENTS_LOG").ok().map(PathBuf::from)
}
