//! Préview headless (équivalent `--apercu` pour Tauri).
//!
//! Le TUI rendait des frames ANSI via ratatui ; ici `collect()` exerce
//! chaque simu/commande pure SANS réseau ni clé API et rend un rapport
//! JSON + résumé texte : contrôle CI (`write_preview`) et sonde
//! d'intégration. Lecture seule (hors `/tmp`), aucun secret affiché.

use serde::Serialize;

use crate::hub::{AgentStatus, AgentSync, ChatSync, Signal, SyncView};

/// Rapport de préview (sérialisable, champs stables).
#[derive(Debug, Serialize)]
pub struct PreviewReport {
    pub theme_colors: usize,
    pub theme_spot: String,
    pub almanac_block: String,
    pub kiro_text: String,
    pub tools_total: usize,
    pub tools_read: usize,
    pub apparition_slug: String,
    pub apparition_scaring: bool,
    pub hub_spirits: usize,
    pub hub_orbs: usize,
    pub hub_sample: String,
    pub haunt_frames: Vec<HauntSummary>,
    pub phantom_mood: String,
    pub phantom_bubble: Option<String>,
    pub fx_fear: u8,
    pub fx_flash: u8,
    pub vps_profiles: usize,
    pub vps_names: Vec<String>,
    pub mcp_servers: usize,
    pub web_backends: Vec<String>,
    pub events_jsonl_ok: bool,
    pub spec_tasks_sample: usize,
    pub perms_matrix: Vec<String>,
    pub bestiary_ok: usize,
    pub sheet_bytes: u64,
}

/// Résumé d'une frame de démo haunt.
#[derive(Debug, Serialize)]
pub struct HauntSummary {
    pub scene: &'static str,
    pub running: bool,
    pub foes: usize,
    pub shots: usize,
    pub score: u32,
    pub lives: u8,
    pub fear: u8,
}

/// Collecte le rapport (sans réseau, lecture seule hors `/tmp`).
pub fn collect() -> PreviewReport {
    let core = core_sample();
    let sims = sims_sample();
    let infra = infra_sample();
    PreviewReport {
        theme_colors: core.theme_colors,
        theme_spot: core.theme_spot,
        almanac_block: core.almanac_block,
        kiro_text: core.kiro_text,
        tools_total: core.tools_total,
        tools_read: core.tools_read,
        apparition_slug: sims.apparition_slug,
        apparition_scaring: sims.apparition_scaring,
        hub_spirits: sims.hub_spirits,
        hub_orbs: sims.hub_orbs,
        hub_sample: sims.hub_sample,
        haunt_frames: sims.haunt_frames,
        phantom_mood: sims.phantom_mood,
        phantom_bubble: sims.phantom_bubble,
        fx_fear: sims.fx_fear,
        fx_flash: sims.fx_flash,
        vps_profiles: infra.vps_profiles,
        vps_names: infra.vps_names,
        mcp_servers: infra.mcp_servers,
        web_backends: infra.web_backends,
        events_jsonl_ok: infra.events_jsonl_ok,
        spec_tasks_sample: infra.spec_tasks_sample,
        perms_matrix: infra.perms_matrix,
        bestiary_ok: infra.bestiary_ok,
        sheet_bytes: infra.sheet_bytes,
    }
}

/// Échantillons socle (thème, almanach, kiro, outils).
struct CoreSample {
    theme_colors: usize,
    theme_spot: String,
    almanac_block: String,
    kiro_text: String,
    tools_total: usize,
    tools_read: usize,
}

/// Échantillons simus (apparition, hub, haunt, phantom, fx).
struct SimsSample {
    apparition_slug: String,
    apparition_scaring: bool,
    hub_spirits: usize,
    hub_orbs: usize,
    hub_sample: String,
    haunt_frames: Vec<HauntSummary>,
    phantom_mood: String,
    phantom_bubble: Option<String>,
    fx_fear: u8,
    fx_flash: u8,
}

/// Échantillons infra (vps, mcp, web, events, spec, perms, bestiaire).
struct InfraSample {
    vps_profiles: usize,
    vps_names: Vec<String>,
    mcp_servers: usize,
    web_backends: Vec<String>,
    events_jsonl_ok: bool,
    spec_tasks_sample: usize,
    perms_matrix: Vec<String>,
    bestiary_ok: usize,
    sheet_bytes: u64,
}

/// Socle : palette, bloc almanach démo, statut kiro, schémas outils.
fn core_sample() -> CoreSample {
    let theme = crate::theme::palette_hex();
    let theme_spot = theme
        .iter()
        .find(|(name, _)| *name == "pumpkin")
        .map(|(_, hex)| hex.clone())
        .unwrap_or_default();
    let demo = crate::almanac::Almanac::demo(crate::almanac::today());
    let kiro_text = crate::kiro::status::text(
        &crate::kiro::project_dir(),
        crate::kiro::global_dir().as_deref(),
    );
    let defs = crate::tools::defs();
    let reads = defs
        .iter()
        .filter(|d| crate::tools::class(&d.function.name) == crate::tools::Class::Read)
        .count();
    CoreSample {
        theme_colors: theme.len(),
        theme_spot,
        almanac_block: demo.usage_block(crate::almanac::today()),
        kiro_text,
        tools_total: defs.len(),
        tools_read: reads,
    }
}

/// Simus : une passe de chaque moteur (déterministes).
fn sims_sample() -> SimsSample {
    let (apparition_slug, apparition_scaring) = apparition_sample();
    let (hub_spirits, hub_orbs, hub_sample) = hub_sample();
    let (phantom_mood, phantom_bubble) = phantom_sample();
    let (fx_fear, fx_flash) = fx_sample();
    SimsSample {
        apparition_slug,
        apparition_scaring,
        hub_spirits,
        hub_orbs,
        hub_sample,
        haunt_frames: haunt_samples(),
        phantom_mood,
        phantom_bubble,
        fx_fear,
        fx_flash,
    }
}

/// Infra : lecture seule partout (aucun réseau, aucun secret).
fn infra_sample() -> InfraSample {
    let profiles = crate::vps::load();
    let mcp = crate::kiro::mcp_pool::McpState::new();
    let (bestiary_ok, sheet_bytes) = bestiary_sample();
    InfraSample {
        vps_profiles: profiles.len(),
        vps_names: profiles.iter().map(|p| p.dst()).collect(),
        mcp_servers: mcp.status_struct().len(),
        web_backends: configured_backends(),
        events_jsonl_ok: events_roundtrip(),
        spec_tasks_sample: crate::kiro::spec::parse_tasks("- [ ] one\n- [x] two\n- nope\n").len(),
        perms_matrix: perms_sample(),
        bestiary_ok,
        sheet_bytes,
    }
}

/// Écrit le rapport JSON dans `path` (+ résumé sur stdout).
pub fn write_preview(path: &str) -> anyhow::Result<PreviewReport> {
    let report = collect();
    let json = serde_json::to_string_pretty(&report)?;
    std::fs::write(path, &json)?;
    println!("preview: {path}");
    println!("{}", summary(&report));
    Ok(report)
}

/// Résumé texte une ligne par section (contrôle CI rapide).
pub fn summary(report: &PreviewReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "theme: {} colors (pumpkin {})\n",
        report.theme_colors, report.theme_spot
    ));
    out.push_str(&format!(
        "tools: {} total, {} read\n",
        report.tools_total, report.tools_read
    ));
    out.push_str(&format!(
        "apparition: {} (scaring={})\n",
        report.apparition_slug, report.apparition_scaring
    ));
    out.push_str(&format!(
        "hub: {} spirits, {} orbs ({})\n",
        report.hub_spirits, report.hub_orbs, report.hub_sample
    ));
    for frame in &report.haunt_frames {
        out.push_str(&format!(
            "haunt[{}]: running={} foes={} shots={} lives={} fear={}\n",
            frame.scene, frame.running, frame.foes, frame.shots, frame.lives, frame.fear
        ));
    }
    out.push_str(&format!(
        "phantom: {} ({:?})\n",
        report.phantom_mood, report.phantom_bubble
    ));
    out.push_str(&format!(
        "fx: fear={} flash={}\n",
        report.fx_fear, report.fx_flash
    ));
    out.push_str(&format!(
        "vps: {} profiles, mcp: {} servers, web: {:?}\n",
        report.vps_profiles, report.mcp_servers, report.web_backends
    ));
    out.push_str(&format!(
        "events: jsonl_ok={} spec_tasks={} perms={:?} bestiary={} sheet={}B\n",
        report.events_jsonl_ok,
        report.spec_tasks_sample,
        report.perms_matrix,
        report.bestiary_ok,
        report.sheet_bytes
    ));
    let kiro_head = report.kiro_text.lines().next().unwrap_or("");
    out.push_str(&format!("kiro: {kiro_head}\n"));
    out
}

/// Apparition : rotation + effroi (slug + scare, comme `--apercu`).
fn apparition_sample() -> (String, bool) {
    let mut apparition = crate::apparition::Apparition::new();
    for _ in 0..65 {
        apparition.tick();
    }
    apparition.scare();
    let slug = apparition.current(false, 0).slug.to_string();
    (slug, apparition.scaring())
}

/// Vue démo : 3 chats (un en envoi) + 5 agents (tous statuts).
fn demo_view() -> SyncView {
    let agent = |id: usize, status: AgentStatus| AgentSync { id, status };
    SyncView {
        chats: vec![
            ChatSync {
                sending: false,
                agents: vec![agent(1, AgentStatus::Running), agent(2, AgentStatus::Done)],
            },
            ChatSync {
                sending: true,
                agents: vec![agent(3, AgentStatus::Failed)],
            },
            ChatSync {
                sending: false,
                agents: vec![
                    agent(4, AgentStatus::Running),
                    agent(5, AgentStatus::Running),
                ],
            },
        ],
    }
}

/// Hub : démo 3 chats + 5 agents (miroir `populate_hub_demo`).
fn hub_sample() -> (usize, usize, String) {
    let mut hub = crate::hub::Hub::new();
    hub.sync(&demo_view());
    hub.signal(Signal::Tell { from: 0, to: 2 });
    hub.signal(Signal::Deliver { agent: 1, to: 1 });
    for _ in 0..60 {
        hub.step();
    }
    hub.scare();
    for _ in 0..5 {
        hub.step();
    }
    let sample = hub
        .spectres
        .first()
        .map(|s| format!("{:?}@{:.1},{:.1}", s.key, s.x, s.y))
        .unwrap_or_default();
    (hub.spectres.len(), hub.orbs.len(), sample)
}

/// Haunt : 4 scènes démo (titre, cimetière, marais, arène).
fn haunt_samples() -> Vec<HauntSummary> {
    ["title", "graveyard", "swamp", "arena"]
        .into_iter()
        .enumerate()
        .map(|(i, scene)| {
            let mut game = crate::haunt::Haunt::new();
            let frame = game.demo_scene(i as u8);
            HauntSummary {
                scene,
                running: frame.running,
                foes: frame.foes.len(),
                shots: frame.shots.len(),
                score: frame.hud.score,
                lives: frame.hud.lives,
                fear: frame.hud.fear,
            }
        })
        .collect()
}

/// Phantom : 200 ticks + coup (humeur + bulle).
fn phantom_sample() -> (String, Option<String>) {
    let mut ghost = crate::phantom::Phantom::new(7);
    for _ in 0..200 {
        ghost.tick();
    }
    let bubble = ghost.smack_center();
    (ghost.mood_word().to_string(), bubble)
}

/// Fx : 10 ticks d'effroi (peur + éclair).
fn fx_sample() -> (u8, u8) {
    let mut fx = crate::fx::Fx::new();
    for _ in 0..10 {
        fx.tick(true);
    }
    (fx.fear(), fx.flash_left())
}

/// Backends web configurés (booléens seuls, jamais les clés).
fn configured_backends() -> Vec<String> {
    let mut backends = vec![String::from("duckduckgo"), String::from("wikipedia")];
    for (var, name) in [("BRAVE_API_KEY", "brave"), ("TAVILY_API_KEY", "tavily")] {
        let present = std::env::var(var)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);
        if present {
            backends.push(name.to_string());
        }
    }
    backends.sort();
    backends
}

/// Puits JSONL : 2 lignes dans `/tmp`, relues, supprimées.
fn events_roundtrip() -> bool {
    let path = sample_path("events.jsonl");
    let _ = std::fs::remove_file(&path);
    let log = crate::events_log::EventLog::new(Some(path.clone()));
    log.emit(&serde_json::json!({"type": "started", "ts": 1}));
    log.emit(&serde_json::json!({"type": "mailbox", "canal": "bus", "texte": "x"}));
    let ok = std::fs::read_to_string(&path)
        .map(|content| content.lines().count() == 2)
        .unwrap_or(false);
    let _ = std::fs::remove_file(&path);
    ok
}

/// Matrice de décision permissions (auto/yolo × lecture/modif).
fn perms_sample() -> Vec<String> {
    use crate::permissions::{Mode, Rules};
    let rules = Rules::default();
    [Mode::Auto, Mode::Yolo]
        .into_iter()
        .flat_map(|mode| {
            ["browser_read", "shell_run"]
                .map(|tool| format!("{:?}/{tool}={:?}", mode, rules.decide(mode, tool)))
        })
        .collect()
}

/// Chemin d'échantillon unique par appel : deux `collect()` concurrents
/// (tests parallèles) ne partagent jamais le même fichier `/tmp`.
fn sample_path(name: &str) -> std::path::PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("bt-preview-{}-{seq}-{name}", std::process::id()))
}

/// Bestiaire : 33 décodés + planche PNG dans `/tmp` (puis supprimée).
fn bestiary_sample() -> (usize, u64) {
    let mut ok = 0;
    for creature in crate::bestiary::BESTIARY.iter() {
        if creature.sprite().is_ok() {
            ok += 1;
        }
    }
    let path = sample_path("sheet.png");
    let bytes = crate::bestiary::write_sheet(path.to_string_lossy().as_ref())
        .ok()
        .and_then(|()| std::fs::metadata(&path).ok())
        .map(|meta| meta.len())
        .unwrap_or(0);
    let _ = std::fs::remove_file(&path);
    (ok, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rapport_coherent_sans_reseau() {
        let report = collect();
        assert_eq!(report.theme_colors, 20);
        assert_eq!(report.theme_spot, "#F2701E");
        assert_eq!(report.tools_total, 14);
        assert_eq!(report.tools_read, 5);
        assert!(report.apparition_scaring);
        assert_eq!(report.hub_spirits, 8);
        assert_eq!(report.haunt_frames.len(), 4);
        assert!(report.haunt_frames.iter().all(|f| f.lives == 3));
        assert_eq!(report.fx_fear, 70);
        assert!(report.events_jsonl_ok);
        assert_eq!(report.spec_tasks_sample, 2);
        assert_eq!(report.bestiary_ok, 33);
        assert!(report.sheet_bytes > 1000);
        assert!(summary(&report).contains("hub: 8 spirits"));
    }

    #[test]
    fn ecrit_un_json_lisible() {
        let path = std::env::temp_dir().join(format!("bt-preview-{}-w.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        write_preview(path.to_string_lossy().as_ref()).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["tools_total"], 14);
        assert_eq!(parsed["theme_spot"], "#F2701E");
        let _ = std::fs::remove_file(&path);
    }
}
