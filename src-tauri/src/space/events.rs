//! Événements du monde 3D (DESIGN project-space §7).
//!
//! [`SceneEvent`] est le miroir Rust exact de `src/lib/space/types.ts` :
//! serde `tag = "type"`, `rename_all = "snake_case"`, mêmes clés par
//! événement. Convention du dépôt (cf. `events.rs` + `contracts.ts`) :
//! l'enum Rust sérialise tel quel sur son canal Tauri — ici
//! `space-event`, distinct de `crypte-event`.
//!
//! [`SpaceLog`] n'émet que s'il porte une `tauri::AppHandle`
//! (`disabled()` = aucune émission, construction d'événement évitée via
//! [`SpaceLog::emit_with`]).

use serde::Serialize;

use super::scan::ProjectTree;

/// Canal Tauri du monde 3D (écouté par `src/lib/space/`).
pub const SPACE_EVENT: &str = "space-event";

/// Étape d'agent (types.ts `Step`, ordre `STEP_ORDER`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Ask,
    Context,
    Think,
    Act,
    Critic,
    Answer,
}

/// Acteur d'un événement (types.ts `AgentRef` : `"main"` | `{ sub: n }`).
///
/// `Serialize` manuel : un enum `untagged` sérialiserait les variantes
/// unité en `null`, pas en `"main"` comme l'exige le contrat TS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRef {
    Main,
    Sub { sub: usize },
}

impl Serialize for AgentRef {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Main => s.serialize_str("main"),
            Self::Sub { sub } => one_key(s, "sub", sub),
        }
    }
}

/// Nature d'un agent (types.ts `AgentKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Sub,
    Web,
}

/// Cible d'un spawn (types.ts `Target` : `{ path }` | `"web"`).
///
/// `Serialize` manuel : même raison qu'[`AgentRef`] (unité `untagged` =
/// `null`, pas `"web"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    File { path: String },
    Web,
}

impl Serialize for Target {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::File { path } => one_key(s, "path", path),
            Self::Web => s.serialize_str("web"),
        }
    }
}

/// Sérialise un objet JSON à une clé (`{"key": value}`).
fn one_key<S, T>(s: S, key: &'static str, value: &T) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
    T: Serialize,
{
    use serde::ser::SerializeMap;
    let mut map = s.serialize_map(Some(1))?;
    map.serialize_entry(key, value)?;
    map.end()
}

/// Un fait observable de la session, consommé par le monde 3D.
///
/// Miroir de `SceneEvent` dans `src/lib/space/types.ts` : chaque variante
/// sérialise en `{"type": "<snake_case>", …}` avec exactement les clés TS.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SceneEvent {
    /// Projet ouvert : le monde construit le quartier (DESIGN §7).
    ProjectOpen { path: String, tree: ProjectTree },
    /// Prompt envoyé : le fantôme s'élève.
    PromptSent { chat: usize },
    /// Étape d'agent démarrée (trace + grand titre).
    StepStart { step: Step },
    /// Fichiers chargés en contexte (faisceaux blancs).
    ContextLoad { files: Vec<String> },
    /// Fichier lu (vol de l'agent + halo blanc).
    FileRead { path: String, agent: AgentRef },
    /// Fichier écrit (orange ; dalles ajoutées/retirées).
    FileWrite {
        path: String,
        agent: AgentRef,
        added: u32,
        removed: u32,
    },
    /// Fichier créé (comme une écriture depuis 0 dalle).
    FileCreate { path: String, agent: AgentRef },
    /// Sous-agent lancé (petit fantôme vers sa cible).
    AgentSpawn {
        id: usize,
        kind: AgentKind,
        target: Target,
    },
    /// Sous-agent terminé (éjecté, impact au sol).
    AgentDone { id: usize, ok: bool },
    /// Critique démarrée (perchoir de Kiro).
    CriticStart { file: Option<String> },
    /// Critique terminée (Kiro éjecté, perchoir effacé).
    CriticDone { file: Option<String> },
    /// Checkpoint sauvegardé (ligne méta dans le chat).
    CheckpointSaved { n: usize },
    /// Retour arrière (les dalles postérieures tombent).
    Rewind { to: usize, files: Vec<String> },
    /// Hub ouvert (caméra reculée, noms des projets).
    HubOpen,
    /// Hub fermé.
    HubClose,
}

/// Émission des événements 3D (désactivée = `None`, émission gratuite).
///
/// Pendant tauri de [`crate::events::EventLog`] : silencieux tant qu'aucune
/// `AppHandle` n'est branchée, échec d'émission toujours ignoré (l'observateur
/// ne doit jamais casser l'app).
#[derive(Debug, Clone, Default)]
pub struct SpaceLog {
    handle: Option<tauri::AppHandle>,
}

impl SpaceLog {
    /// Journal désactivé (aucune émission).
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Journal branché sur l'app Tauri (canal [`SPACE_EVENT`]).
    pub fn from_app(handle: tauri::AppHandle) -> Self {
        Self {
            handle: Some(handle),
        }
    }

    /// Émet un événement sur le canal [`SPACE_EVENT`] (handle présente
    /// seulement ; échec silencieux).
    pub fn emit(&self, ev: &SceneEvent) {
        let Some(handle) = &self.handle else {
            return;
        };
        use tauri::Emitter;
        let _ = handle.emit(SPACE_EVENT, ev);
    }

    /// Comme [`SpaceLog::emit`], mais l'événement n'est construit que si le
    /// journal est branché (économise les allocations à chaque pas agent).
    pub fn emit_with(&self, build: impl FnOnce() -> SceneEvent) {
        if self.handle.is_some() {
            self.emit(&build());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::scan::{DirNode, FileNode};

    /// Clés de l'objet JSON (sans `type`), pour comparer à types.ts.
    fn keys(v: &serde_json::Value) -> Vec<String> {
        let mut ks: Vec<String> = v
            .as_object()
            .expect("object")
            .keys()
            .filter(|k| *k != "type")
            .cloned()
            .collect();
        ks.sort();
        ks
    }

    #[test]
    fn file_write_serializes_like_types_ts() {
        let ev = SceneEvent::FileWrite {
            path: "src/main.rs".to_string(),
            agent: AgentRef::Sub { sub: 3 },
            added: 12,
            removed: 4,
        };
        let raw = serde_json::to_string(&ev).unwrap();
        assert!(raw.contains("\"type\":\"file_write\""), "{raw}");
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(keys(&v), ["added", "agent", "path", "removed"]);
        assert_eq!(v["path"], "src/main.rs");
        assert_eq!(v["agent"], serde_json::json!({ "sub": 3 }));
        assert_eq!(v["added"], 12);
        assert_eq!(v["removed"], 4);
    }

    #[test]
    fn agent_ref_and_target_match_types_ts() {
        let main = serde_json::to_value(AgentRef::Main).unwrap();
        assert_eq!(main, serde_json::json!("main"));
        let spawn = SceneEvent::AgentSpawn {
            id: 7,
            kind: AgentKind::Web,
            target: Target::Web,
        };
        let v = serde_json::to_value(&spawn).unwrap();
        assert_eq!(v["type"], "agent_spawn");
        assert_eq!(v["kind"], "web");
        assert_eq!(v["target"], serde_json::json!("web"));
        let file_target = SceneEvent::AgentSpawn {
            id: 8,
            kind: AgentKind::Sub,
            target: Target::File {
                path: "lib.rs".to_string(),
            },
        };
        let v = serde_json::to_value(&file_target).unwrap();
        assert_eq!(v["kind"], "sub");
        assert_eq!(v["target"], serde_json::json!({ "path": "lib.rs" }));
    }

    #[test]
    fn project_open_carries_tree_shape() {
        let ev = SceneEvent::ProjectOpen {
            path: "/tmp/demo".to_string(),
            tree: ProjectTree {
                root: "/tmp/demo".into(),
                dirs: vec![DirNode {
                    path: "".into(),
                    parent: None,
                }],
                files: vec![FileNode {
                    path: "main.rs".into(),
                    dir: 0,
                    lines: 42,
                }],
            },
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "project_open");
        assert_eq!(v["path"], "/tmp/demo");
        assert_eq!(v["tree"]["root"], "/tmp/demo");
        assert_eq!(v["tree"]["dirs"][0]["parent"], serde_json::json!(null));
        assert_eq!(v["tree"]["files"][0]["dir"], 0);
        assert_eq!(v["tree"]["files"][0]["lines"], 42);
    }

    #[test]
    fn remaining_variants_match_types_ts() {
        let cases = vec![
            (
                SceneEvent::PromptSent { chat: 1 },
                "prompt_sent",
                vec!["chat"],
            ),
            (
                SceneEvent::StepStart { step: Step::Critic },
                "step_start",
                vec!["step"],
            ),
            (
                SceneEvent::ContextLoad { files: vec![] },
                "context_load",
                vec!["files"],
            ),
            (
                SceneEvent::FileRead {
                    path: "a.rs".into(),
                    agent: AgentRef::Main,
                },
                "file_read",
                vec!["agent", "path"],
            ),
            (
                SceneEvent::FileCreate {
                    path: "b.rs".into(),
                    agent: AgentRef::Main,
                },
                "file_create",
                vec!["agent", "path"],
            ),
            (
                SceneEvent::AgentDone { id: 2, ok: true },
                "agent_done",
                vec!["id", "ok"],
            ),
            (
                SceneEvent::CriticStart { file: None },
                "critic_start",
                vec!["file"],
            ),
            (
                SceneEvent::CriticDone {
                    file: Some("x.rs".into()),
                },
                "critic_done",
                vec!["file"],
            ),
            (
                SceneEvent::CheckpointSaved { n: 3 },
                "checkpoint_saved",
                vec!["n"],
            ),
            (
                SceneEvent::Rewind {
                    to: 2,
                    files: vec!["y.rs".into()],
                },
                "rewind",
                vec!["files", "to"],
            ),
            (SceneEvent::HubOpen, "hub_open", vec![]),
            (SceneEvent::HubClose, "hub_close", vec![]),
        ];
        for (ev, tag, mut want) in cases {
            let v = serde_json::to_value(&ev).unwrap();
            assert_eq!(v["type"], tag, "{tag}");
            want.sort();
            assert_eq!(keys(&v), want, "{tag}");
        }
        let step = serde_json::to_value(Step::Ask).unwrap();
        assert_eq!(step, "ask");
    }

    #[test]
    fn disabled_log_emits_nothing() {
        // Sans handle : `emit`/`emit_with` ne font rien (et ne paniquent pas).
        let log = SpaceLog::disabled();
        log.emit(&SceneEvent::HubOpen);
        log.emit_with(|| SceneEvent::HubClose);
        assert!(log.handle.is_none());
    }
}

// ---------------------------------------------------------------- dérivations
/// Déduit les événements du monde 3D du flux domaine existant : un seul point
/// de branchement (le relai `crypte-event` de `lib.rs`).
pub fn derive(log: &SpaceLog, event: &crate::events::Event) {
    use crate::events::Event as E;
    match event {
        E::UserMsg { chat, .. } => log.emit(&SceneEvent::PromptSent { chat: *chat }),
        E::AssistantMsg { .. } => log.emit(&SceneEvent::StepStart {
            step: Step::Answer,
        }),
        E::AgentSpawned { agent, .. } => log.emit(&SceneEvent::AgentSpawn {
            id: *agent,
            kind: AgentKind::Sub,
            target: Target::File {
                path: std::env::current_dir().unwrap_or_default().display().to_string(),
            },
        }),
        E::AgentDone { agent, .. } => log.emit(&SceneEvent::AgentDone { id: *agent, ok: true }),
        E::AgentFailed { agent, .. } => {
            log.emit(&SceneEvent::AgentDone { id: *agent, ok: false })
        }
        _ => {}
    }
}

// ------------------------------------------------- lectures/écritures shell
/// Il n'existe pas d'outil fichier typé : tout passe par `shell`. On devine
/// les chemins lus/écrits dans la commande (comme le port terminal).
const READ_CMDS: &[&str] = &[
    "cat", "head", "tail", "less", "more", "wc", "nl", "sort", "rg", "grep", "awk", "sed", "stat",
    "file", "xxd", "od",
];
const PATTERN_CMDS: &[&str] = &["rg", "grep", "awk", "sed"];

/// La commande shell d'un appel d'outil (args JSON `{"command": …}`).
pub fn shell_command(args: &str) -> String {
    serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|v| v.get("command").and_then(|c| c.as_str()).map(String::from))
        .unwrap_or_else(|| args.to_string())
}

/// Chemins lus par une commande shell.
pub fn read_paths_from_shell(cmd: &str) -> Vec<String> {
    let toks: Vec<&str> = cmd.split_whitespace().collect();
    let Some(cmd0) = toks.first().map(|t| basename(t)) else {
        return Vec::new();
    };
    if !READ_CMDS.contains(&cmd0) || toks.iter().any(|t| *t == "-i") {
        return Vec::new();
    }
    let mut args = toks.iter().skip(1).filter(|t| !t.starts_with('-'));
    if PATTERN_CMDS.contains(&cmd0) {
        args.next();
    }
    args.map(|t| (*t).to_string()).collect()
}

/// Chemins écrits par une commande shell (redirections, tee, sed -i, cp/mv).
pub fn write_paths_from_shell(cmd: &str) -> Vec<String> {
    let toks: Vec<&str> = cmd.split_whitespace().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        match toks[i] {
            ">" | ">>" | "1>" | "2>" => {
                if let Some(t) = toks.get(i + 1) {
                    if !t.starts_with('-') {
                        out.push((*t).to_string());
                    }
                }
                i += 1;
            }
            "tee" => out.extend(toks.iter().skip(i + 1).filter(|t| !t.starts_with('-')).map(|t| (*t).to_string())),
            "sed" if toks.iter().any(|t| *t == "-i") => {
                let mut it = toks.iter().skip(i + 1).filter(|t| !t.starts_with('-'));
                it.next();
                out.extend(it.map(|t| (*t).to_string()));
            }
            "cp" | "mv" => {
                if let Some(t) = toks.get(i + 2) {
                    if !t.starts_with('-') {
                        out.push((*t).to_string());
                    }
                }
                i += 2;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

fn basename(t: &str) -> &str {
    t.rsplit('/').next().unwrap_or(t)
}

#[cfg(test)]
mod sniff_tests {
    use super::*;

    #[test]
    fn lectures_devinees() {
        assert_eq!(read_paths_from_shell("cat src/a.rs"), vec!["src/a.rs"]);
        assert_eq!(read_paths_from_shell("rg -n TODO src"), vec!["src"]);
        assert!(read_paths_from_shell("ls -la").is_empty());
        assert!(read_paths_from_shell("sed -i s/a/b/ f.rs").is_empty());
    }

    #[test]
    fn ecritures_devinees() {
        assert_eq!(write_paths_from_shell("echo x > out.txt"), vec!["out.txt"]);
        assert_eq!(write_paths_from_shell("sed -i s/a/b/ f.rs"), vec!["f.rs"]);
        assert_eq!(write_paths_from_shell("cp a.rs b.rs"), vec!["b.rs"]);
        assert!(write_paths_from_shell("cat f.rs").is_empty());
    }

    #[test]
    fn derive_ne_casse_jamais_meme_desactive() {
        let log = SpaceLog::disabled();
        derive(&log, &crate::events::Event::user_msg(0, "hi"));
        derive(&log, &crate::events::Event::started());
    }
}
