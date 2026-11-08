//! Commandes §6 (hors specs/MCP) : statut, skills, mémoire,
//! checkpoints, hooks. Les annonces vont en outbox (lignes chat).

use std::sync::Mutex;

use tauri::State;

use crate::state::{lock, BackendOut, ToolsBackend};
use crate::views::{HooksView, KiroStatusView, NamedHooksView, SkillView, SnapshotView};

/// `kiro_status` : compteurs + texte `/kiro` (identique au TUI).
#[tauri::command]
pub fn kiro_status(backend: State<Mutex<ToolsBackend>>) -> Result<KiroStatusView, String> {
    let backend = lock(&backend)?;
    let project = backend.kiro_project.clone();
    let global = backend.kiro_global.clone();
    let steering = crate::kiro::steering::load_all_in(&project, global.as_deref());
    let specs = crate::kiro::spec::load_all(&project.join(".kiro/specs"));
    let hooks = crate::kiro::hooks::load_all_in(&project, global.as_deref());
    let hook_count: usize = hooks.iter().map(|named| named.hooks.len()).sum();
    let mcp = crate::kiro::mcp::load_merged_in(&project, global.as_deref());
    let servers = crate::kiro::mcp::enabled(&mcp);
    let skills = crate::kiro::skills::load_all_in(&project, global.as_deref());
    let text = crate::kiro::status::text(&project, global.as_deref());
    Ok(KiroStatusView {
        project_dir: project.to_string_lossy().into_owned(),
        global_dir: global.map(|g| g.to_string_lossy().into_owned()),
        steering: steering.len(),
        specs: specs.len(),
        hooks: hook_count,
        mcp_servers: servers.len(),
        skills: skills.len(),
        text,
    })
}

/// `skills_list` : nom + description (projet écrase le global).
#[tauri::command]
pub fn skills_list(backend: State<Mutex<ToolsBackend>>) -> Result<Vec<SkillView>, String> {
    let backend = lock(&backend)?;
    let skills =
        crate::kiro::skills::load_all_in(&backend.kiro_project, backend.kiro_global.as_deref());
    Ok(skills
        .iter()
        .map(|skill| SkillView {
            name: skill.name.clone(),
            description: skill.description.clone(),
        })
        .collect())
}

/// `skill_inject` : arme le skill (corps + texte en tête du prochain
/// envoi, transitoire). L'intégrateur préfixe à la saisie via
/// `take_skill_prefix` (même ordre que le TUI : corps, texte, brouillon).
#[tauri::command]
pub fn skill_inject(
    backend: State<Mutex<ToolsBackend>>,
    name: String,
    text: Option<String>,
) -> Result<(), String> {
    let mut backend = lock(&backend)?;
    let body = backend.skill_body(&name)?;
    let mut parts = vec![body];
    if let Some(text) = text.filter(|t| !t.trim().is_empty()) {
        parts.push(text);
    }
    backend.skill_prefix = Some(parts.join("\n\n"));
    let chat = backend.current_chat;
    backend.outbox.push(BackendOut::ChatLine {
        chat,
        text: format!("skill '{name}' armed — send to use (transient)"),
    });
    Ok(())
}

/// `memory_remember` : ajoute une ligne à `.kiro/MEMORY.md` (projet).
#[tauri::command]
pub fn memory_remember(backend: State<Mutex<ToolsBackend>>, text: String) -> Result<(), String> {
    let mut backend = lock(&backend)?;
    crate::kiro::memory::remember_via(
        &backend.kiro_project.clone(),
        &text,
        backend.checkpoint_dir.as_deref(),
    )
    .map_err(|err| format!("cannot remember: {err}"))?;
    let chat = backend.current_chat;
    backend.outbox.push(BackendOut::ChatLine {
        chat,
        text: String::from("remembered to .kiro/MEMORY.md"),
    });
    Ok(())
}

/// `checkpoints_list` : snapshots, plus récent d'abord.
#[tauri::command]
pub fn checkpoints_list(backend: State<Mutex<ToolsBackend>>) -> Result<Vec<SnapshotView>, String> {
    let backend = lock(&backend)?;
    let snaps = backend
        .checkpoint_dir
        .as_deref()
        .map(crate::kiro::checkpoint::list_in)
        .unwrap_or_default();
    Ok(snaps
        .iter()
        .map(|snap| SnapshotView {
            name: snap.original.to_string_lossy().into_owned(),
            created: created_of(&snap.file),
        })
        .collect())
}

/// `checkpoint_rewind` : restaure (rang, basename ou fragment).
#[tauri::command]
pub fn checkpoint_rewind(
    backend: State<Mutex<ToolsBackend>>,
    target: String,
) -> Result<String, String> {
    let backend = lock(&backend)?;
    match backend.checkpoint_dir.as_deref() {
        Some(ckpt) => crate::kiro::checkpoint::rewind_in(ckpt, &target),
        None => Err(String::from("no checkpoints")),
    }
}

/// `hooks_list` : hooks par fichier + derniers déclenchements.
#[tauri::command]
pub fn hooks_list(backend: State<Mutex<ToolsBackend>>) -> Result<HooksView, String> {
    let backend = lock(&backend)?;
    let loaded =
        crate::kiro::hooks::load_all_in(&backend.kiro_project, backend.kiro_global.as_deref());
    let hooks = loaded
        .iter()
        .map(|named| {
            let mut triggers = Vec::new();
            for hook in &named.hooks {
                let name = hook.trigger.as_str().to_string();
                if !triggers.contains(&name) {
                    triggers.push(name);
                }
            }
            NamedHooksView {
                file: named.file.clone(),
                triggers,
                count: named.hooks.len(),
            }
        })
        .collect();
    let recent = backend
        .hooks
        .state()
        .recent
        .iter()
        .map(|fire| format!("{} {} {}", fire.name, fire.trigger.as_str(), fire.status))
        .collect();
    Ok(HooksView { hooks, recent })
}

/// Date de création d'un snapshot : nanos du nom de fichier, sinon
/// mtime, sinon `unknown` (RFC 3339 dans les deux premiers cas).
fn created_of(file: &std::path::Path) -> String {
    if let Some(stem) = file.file_stem().and_then(|s| s.to_str()) {
        if let Some(nanos) = stem.split('-').next().and_then(|n| n.parse::<i128>().ok()) {
            let secs = (nanos / 1_000_000_000) as i64;
            let nsub = (nanos % 1_000_000_000) as u32;
            if let Some(dt) = chrono::DateTime::from_timestamp(secs, nsub) {
                return dt.to_rfc3339();
            }
        }
    }
    std::fs::metadata(file)
        .and_then(|meta| meta.modified())
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339())
        .unwrap_or_else(|_| String::from("unknown"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend(tag: &str) -> (ToolsBackend, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("bt-cmd-kiro-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        (ToolsBackend::new_in(&dir), dir)
    }

    #[test]
    fn statut_vide_et_skill_inconnu() {
        let (backend, dir) = backend("statut");
        let text = crate::kiro::status::text(&backend.kiro_project, None);
        assert!(text.contains("nothing loaded"), "{text}");
        assert!(backend
            .skill_body("zzz")
            .unwrap_err()
            .contains("unknown skill: zzz"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remember_puis_rewind_roundtrip() {
        let (backend, dir) = backend("remember");
        assert_eq!(backend.current_chat, 0);
        let ckpt = dir.join("ckpt");
        std::fs::create_dir_all(&ckpt).unwrap();
        crate::kiro::memory::remember_via(&dir, "premier", Some(&ckpt)).unwrap();
        crate::kiro::memory::remember_via(&dir, "second", Some(&ckpt)).unwrap();
        let snaps = crate::kiro::checkpoint::list_in(&ckpt);
        assert_eq!(snaps.len(), 1);
        assert_ne!(created_of(&snaps[0].file), "unknown");
        let msg = crate::kiro::checkpoint::rewind_in(&ckpt, "1").unwrap();
        assert!(msg.contains("restored"), "{msg}");
        let raw = std::fs::read_to_string(dir.join(".kiro/MEMORY.md")).unwrap();
        assert_eq!(raw, "premier\n");
        std::fs::remove_dir_all(&dir).ok();
    }
}
