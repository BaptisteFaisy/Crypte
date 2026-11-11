//! Pont événements → état de la scène (DESIGN §7).
//!
//! [`SceneState::apply`] est le seul endroit qui traduit « l'agent a fait X »
//! en « le monde montre X ». [`SceneLog`] transporte les événements depuis la
//! logique de l'agent jusqu'au rendu, sans que celle-ci connaisse celui-ci :
//! quand la vue 3D est coupée, `emit` ne fait rien du tout.

use std::path::PathBuf;

use tokio::sync::mpsc::{self, Receiver, Sender};

use super::{
    AgentRef, KiroState, RewindState, SceneEvent, SceneState, Step, SubState, LINES_PER_SLAB,
};

impl SceneState {
    /// Plie un événement dans l'état logique du monde.
    pub fn apply(&mut self, ev: &SceneEvent, now: f64) {
        match ev {
            SceneEvent::ProjectOpen { path, tree } => self.open_project(path, tree.clone()),
            SceneEvent::PromptSent { .. } => self.step = Some(Step::Ask),
            SceneEvent::StepStart { step } => self.step = Some(*step),
            SceneEvent::ContextLoad { files } => {
                for f in files {
                    touch_read(self, f, now);
                }
            }
            SceneEvent::FileRead { path, agent } => {
                touch_read(self, path, now);
                if *agent == AgentRef::Main {
                    self.main_file = Some(path.clone());
                    self.main_wrote = false;
                }
            }
            SceneEvent::FileWrite {
                path, agent, added, ..
            } => {
                touch_write(self, path, now, *added);
                if *agent == AgentRef::Main {
                    self.main_file = Some(path.clone());
                    self.main_wrote = true;
                }
            }
            SceneEvent::FileCreate { path, agent } => {
                let f = self.files.entry(path.clone()).or_default();
                f.slabs = 1;
                touch_write(self, path, now, 0);
                if *agent == AgentRef::Main {
                    self.main_file = Some(path.clone());
                    self.main_wrote = true;
                }
            }
            SceneEvent::AgentSpawn { id, kind, target } => {
                self.subs.insert(
                    *id,
                    SubState {
                        kind: *kind,
                        target: target.clone(),
                        spawned_at: now,
                        done_at: None,
                        pushed: false,
                    },
                );
            }
            SceneEvent::AgentDone { id, .. } => {
                if let Some(s) = self.subs.get_mut(id) {
                    s.done_at = Some(now);
                    s.pushed = true;
                }
            }
            SceneEvent::CriticStart { file } => {
                self.kiro = Some(KiroState {
                    file: file.clone(),
                    started_at: now,
                    done_at: None,
                    pushed: false,
                });
            }
            SceneEvent::CriticDone { file } => {
                if let Some(k) = &mut self.kiro {
                    if k.file.is_none() {
                        k.file = file.clone();
                    }
                    k.done_at = Some(now);
                    k.pushed = true;
                }
            }
            SceneEvent::CheckpointSaved { n } => self.checkpoint = *n,
            SceneEvent::Rewind { to, files } => {
                self.checkpoint = *to;
                self.rewind = Some(RewindState {
                    at: now,
                    files: files.clone(),
                });
                for f in files {
                    if let Some(fs) = self.files.get_mut(f) {
                        fs.changed = false;
                        fs.slabs = 1;
                    }
                }
            }
            SceneEvent::HubOpen => self.hub = true,
            SceneEvent::HubClose => self.hub = false,
        }
    }
}

/// Marque un fichier comme lu à `now`.
fn touch_read(s: &mut SceneState, path: &PathBuf, now: f64) {
    s.files.entry(path.clone()).or_default().read_at = Some(now);
}

/// Marque un fichier comme écrit à `now`, et ajoute les dalles gagnées.
fn touch_write(s: &mut SceneState, path: &PathBuf, now: f64, added: u32) {
    let f = s.files.entry(path.clone()).or_default();
    f.write_at = Some(now);
    f.changed = true;
    f.slabs += added / LINES_PER_SLAB;
}

/// Le transport des événements : un canal borné, ou rien du tout.
#[derive(Debug, Clone)]
pub struct SceneLog {
    tx: Option<Sender<SceneEvent>>,
}

impl SceneLog {
    /// Vue 3D coupée : aucune émission, aucun coût.
    pub fn disabled() -> Self {
        Self { tx: None }
    }

    /// Ouvre le canal ; le rendu consomme le `Receiver`.
    pub fn channel(cap: usize) -> (Self, Receiver<SceneEvent>) {
        let (tx, rx) = mpsc::channel(cap.max(1));
        (Self { tx: Some(tx) }, rx)
    }

    /// Vrai si quelqu'un écoute.
    pub fn enabled(&self) -> bool {
        self.tx.is_some()
    }

    /// Émet sans jamais bloquer (canal plein = événement jeté).
    pub fn emit(&self, ev: SceneEvent) {
        if let Some(tx) = &self.tx {
            let _ = tx.try_send(ev);
        }
    }

    /// Émet paresseusement : la fermeture n'est appelée que si on écoute.
    pub fn emit_with(&self, f: impl FnOnce() -> SceneEvent) {
        if self.tx.is_some() {
            self.emit(f());
        }
    }
}

/// Commandes shell qui lisent un fichier.
const READ_CMDS: &[&str] = &[
    "cat", "head", "tail", "less", "more", "wc", "nl", "sort", "rg", "grep", "awk", "sed", "stat",
    "file", "xxd", "od",
];

/// Commandes shell dont le premier argument est un motif, pas un chemin.
const PATTERN_CMDS: &[&str] = &["rg", "grep", "awk", "sed"];

/// Chemins lus par une commande shell (heuristique : il n'existe pas d'outil
/// fichier typé dans ce dépôt, tout passe par `shell`).
pub fn read_paths_from_shell(cmd: &str) -> Vec<PathBuf> {
    let toks: Vec<&str> = cmd.split_whitespace().collect();
    let Some(cmd0) = toks.first().map(|t| basename(t)) else {
        return Vec::new();
    };
    if !READ_CMDS.contains(&cmd0) || toks.iter().any(|t| *t == "-i") {
        return Vec::new();
    }
    let mut args = toks.iter().skip(1).filter(|t| !t.starts_with('-'));
    if PATTERN_CMDS.contains(&cmd0) {
        args.next(); // le motif
    }
    args.map(PathBuf::from).collect()
}

/// Chemins écrits par une commande shell : redirections, `tee`, `sed -i`,
/// cible de `cp`/`mv`.
pub fn write_paths_from_shell(cmd: &str) -> Vec<PathBuf> {
    let toks: Vec<&str> = cmd.split_whitespace().collect();
    let mut out: Vec<PathBuf> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        match toks[i] {
            ">" | ">>" | "1>" | "2>" => {
                if let Some(t) = toks.get(i + 1) {
                    if looks_like_path(t) {
                        out.push(PathBuf::from(*t));
                    }
                }
                i += 1;
            }
            "tee" => {
                for t in toks.iter().skip(i + 1) {
                    if looks_like_path(t) {
                        out.push(PathBuf::from(*t));
                    }
                }
            }
            "sed" if toks.iter().any(|t| *t == "-i") => {
                let mut it = toks.iter().skip(i + 1).filter(|t| !t.starts_with('-'));
                it.next(); // l'expression sed
                for t in it {
                    out.push(PathBuf::from(*t));
                }
            }
            "cp" | "mv" => {
                if let Some(t) = toks.get(i + 2) {
                    if looks_like_path(t) {
                        out.push(PathBuf::from(*t));
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

/// Nom de commande sans le chemin (`/usr/bin/cat` → `cat`).
fn basename(t: &str) -> &str {
    t.rsplit('/').next().unwrap_or(t)
}

/// La commande shell d'un appel d'outil : les args sont du JSON
/// (`{"command": …}`) ; hors JSON, on prend la chaîne telle quelle.
pub fn shell_command(args: &str) -> String {
    serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|v| v.get("command").and_then(|c| c.as_str()).map(String::from))
        .unwrap_or_else(|| args.to_string())
}

/// Un argument qui n'est pas un drapeau (cible de redirection, `tee`, `cp`…).
fn looks_like_path(t: &str) -> bool {
    !t.starts_with('-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::scan::{DirNode, FileNode, ProjectTree};
    use std::path::Path;

    fn tree() -> ProjectTree {
        ProjectTree {
            root: PathBuf::from("/p"),
            dirs: vec![DirNode {
                path: PathBuf::from("src"),
                parent: None,
            }],
            files: vec![FileNode {
                path: PathBuf::from("src/a.rs"),
                dir: 0,
                lines: 300,
            }],
        }
    }

    fn open() -> SceneState {
        let mut s = SceneState::new();
        s.apply(
            &SceneEvent::ProjectOpen {
                path: PathBuf::from("/p"),
                tree: tree(),
            },
            0.0,
        );
        s
    }

    #[test]
    fn lecture_marque_le_faisceau_blanc() {
        let mut s = open();
        s.apply(
            &SceneEvent::FileRead {
                path: PathBuf::from("src/a.rs"),
                agent: AgentRef::Main,
            },
            1.0,
        );
        let f = &s.files[Path::new("src/a.rs")];
        assert_eq!(f.read_at, Some(1.0));
        assert_eq!(s.main_file.as_deref(), Some(Path::new("src/a.rs")));
        assert!(!s.main_wrote);
    }

    #[test]
    fn ecriture_allume_et_grossit_la_pile() {
        let mut s = open();
        let before = s.files[Path::new("src/a.rs")].slabs;
        s.apply(
            &SceneEvent::FileWrite {
                path: PathBuf::from("src/a.rs"),
                agent: AgentRef::Main,
                added: 400,
                removed: 10,
            },
            2.0,
        );
        let f = &s.files[Path::new("src/a.rs")];
        assert!(f.changed);
        assert_eq!(f.write_at, Some(2.0));
        assert_eq!(f.slabs, before + 2);
        assert!(s.main_wrote);
    }

    #[test]
    fn creation_donne_une_dalle() {
        let mut s = open();
        s.apply(
            &SceneEvent::FileCreate {
                path: PathBuf::from("src/neuf.rs"),
                agent: AgentRef::Main,
            },
            3.0,
        );
        assert_eq!(s.files[Path::new("src/neuf.rs")].slabs, 1);
    }

    #[test]
    fn sous_agent_poussé_a_sa_fin() {
        let mut s = open();
        s.apply(
            &SceneEvent::AgentSpawn {
                id: 7,
                kind: super::super::AgentKind::Sub,
                target: super::super::Target::Path(PathBuf::from("src/a.rs")),
            },
            1.0,
        );
        assert_eq!(s.subs[&7].done_at, None);
        s.apply(&SceneEvent::AgentDone { id: 7, ok: true }, 4.0);
        assert_eq!(s.subs[&7].done_at, Some(4.0));
        assert!(s.subs[&7].pushed);
    }

    #[test]
    fn kiro_perche_puis_poussé() {
        let mut s = open();
        s.apply(
            &SceneEvent::CriticStart {
                file: Some(PathBuf::from("src/a.rs")),
            },
            1.0,
        );
        assert!(s.kiro.is_some() && s.kiro.as_ref().unwrap().done_at.is_none());
        s.apply(&SceneEvent::CriticDone { file: None }, 5.0);
        let k = s.kiro.as_ref().unwrap();
        assert_eq!(k.done_at, Some(5.0));
        assert!(k.pushed);
    }

    #[test]
    fn rewind_effondre_les_dalles() {
        let mut s = open();
        s.apply(
            &SceneEvent::FileWrite {
                path: PathBuf::from("src/a.rs"),
                agent: AgentRef::Main,
                added: 900,
                removed: 0,
            },
            1.0,
        );
        assert!(s.files[Path::new("src/a.rs")].slabs > 2);
        s.apply(
            &SceneEvent::Rewind {
                to: 3,
                files: vec![PathBuf::from("src/a.rs")],
            },
            9.0,
        );
        let f = &s.files[Path::new("src/a.rs")];
        assert_eq!(f.slabs, 1);
        assert!(!f.changed);
        assert_eq!(s.checkpoint, 3);
        assert!(s.rewind.is_some());
    }

    #[test]
    fn hub_et_étapes() {
        let mut s = open();
        s.apply(&SceneEvent::HubOpen, 1.0);
        assert!(s.hub);
        s.apply(&SceneEvent::HubClose, 2.0);
        assert!(!s.hub);
        s.apply(&SceneEvent::StepStart { step: Step::Act }, 3.0);
        assert_eq!(s.step, Some(Step::Act));
        s.apply(&SceneEvent::PromptSent { chat: 0 }, 4.0);
        assert_eq!(s.step, Some(Step::Ask));
        s.apply(&SceneEvent::CheckpointSaved { n: 5 }, 5.0);
        assert_eq!(s.checkpoint, 5);
    }

    #[test]
    fn le_canal_est_gratuit_quand_coupé() {
        let log = SceneLog::disabled();
        assert!(!log.enabled());
        log.emit(SceneEvent::HubOpen); // ne doit ni paniquer ni allouer
        let mut called = false;
        log.emit_with(|| {
            called = true;
            SceneEvent::HubOpen
        });
        assert!(!called);
    }

    #[test]
    fn le_canal_transporte() {
        let (log, mut rx) = SceneLog::channel(4);
        assert!(log.enabled());
        log.emit(SceneEvent::HubOpen);
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn heuristique_shell_lectures() {
        assert_eq!(
            read_paths_from_shell("cat src/a.rs"),
            vec![PathBuf::from("src/a.rs")]
        );
        assert_eq!(
            read_paths_from_shell("rg -n TODO src"),
            vec![PathBuf::from("src")]
        );
        assert!(read_paths_from_shell("ls -la").is_empty());
        assert!(read_paths_from_shell("sed -i s/a/b/ f.rs").is_empty());
    }

    #[test]
    fn heuristique_shell_écritures() {
        assert_eq!(
            write_paths_from_shell("echo x > out.txt"),
            vec![PathBuf::from("out.txt")]
        );
        assert_eq!(
            write_paths_from_shell("sed -i s/a/b/ f.rs"),
            vec![PathBuf::from("f.rs")]
        );
        assert_eq!(
            write_paths_from_shell("cp a.rs b.rs"),
            vec![PathBuf::from("b.rs")]
        );
        assert!(write_paths_from_shell("cat f.rs").is_empty());
    }
}
