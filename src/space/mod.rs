//! Projet space : la vue 3D des projets, en fond de l'écran.
//!
//! Le module est découpé en trois couches qui ne se connaissent pas :
//!
//! - les **événements** (`SceneEvent`) : ce que l'agent fait, typé, indépendant du rendu ;
//! - l'**état** (`SceneState`) : le monde logique (fichiers lus/écrits, agents, Kiro, étapes) ;
//! - le **rendu** (`render`) : la scène 3D, fonction pure de l'état et du temps.
//!
//! Voir `docs/design/project-space/DESIGN.md` (§6 le monde, §7 les événements).

// Le module n'est pas encore branché à l'application : sans cela, le crate
// binaire signale en `dead_code` chaque élément public. À retirer au branchement UI.
#![allow(dead_code)]

pub mod actors;
pub mod cam;
pub mod layout;
pub mod paint;
pub mod proj;
pub mod raster;
pub mod render;
pub mod scan;
pub mod scene;
pub mod slabs;
pub mod view;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Une dalle ≈ 150 lignes (DESIGN §6.1).
pub const LINES_PER_SLAB: u32 = 150;

/// Les six étapes d'une requête (DESIGN §7).
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

impl Step {
    /// Rang de l'étape (1..6), pour le titre « 04 / 06 ».
    pub fn rank(self) -> u8 {
        match self {
            Step::Ask => 1,
            Step::Context => 2,
            Step::Think => 3,
            Step::Act => 4,
            Step::Critic => 5,
            Step::Answer => 6,
        }
    }

    /// Nom affiché (majuscules, comme la trace du chat).
    pub fn label(self) -> &'static str {
        match self {
            Step::Ask => "ASK",
            Step::Context => "CONTEXT",
            Step::Think => "THINK",
            Step::Act => "ACT",
            Step::Critic => "CRITIC",
            Step::Answer => "ANSWER",
        }
    }
}

/// Qui agit : le fantôme principal ou un sous-agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRef {
    Main,
    Sub(u32),
}

/// Nature d'un sous-agent (DESIGN §7 : `agent.spawn`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    /// Sous-agent classique : il travaille sur un fichier du projet.
    Sub,
    /// Sous-agent « recherche web » : sa cible est une plateforme hors projet.
    Web,
}

/// Ce vers quoi un agent vole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Path(PathBuf),
    Web,
}

/// Les événements que le rendu consomme (DESIGN §7).
///
/// Aucun ne porte de référence au rendu : c'est l'agent qui parle, la scène
/// qui écoute.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SceneEvent {
    /// Le projet actif est ouvert : on construit son quartier.
    ProjectOpen {
        path: PathBuf,
        tree: scan::ProjectTree,
    },
    /// L'utilisateur a envoyé une requête.
    PromptSent { chat: usize },
    /// Une étape démarre.
    StepStart { step: Step },
    /// Le contexte est chargé (fichiers de steering, skills…).
    ContextLoad { files: Vec<PathBuf> },
    /// Un fichier est lu.
    FileRead { path: PathBuf, agent: AgentRef },
    /// Un fichier est écrit.
    FileWrite {
        path: PathBuf,
        agent: AgentRef,
        added: u32,
        removed: u32,
    },
    /// Un fichier est créé.
    FileCreate { path: PathBuf, agent: AgentRef },
    /// Un sous-agent part.
    AgentSpawn {
        id: u32,
        kind: AgentKind,
        target: Target,
    },
    /// Un sous-agent a fini (succès ou échec).
    AgentDone { id: u32, ok: bool },
    /// Kiro commence sa critique.
    CriticStart { file: Option<PathBuf> },
    /// Kiro a fini sa critique.
    CriticDone { file: Option<PathBuf> },
    /// Un checkpoint est enregistré.
    CheckpointSaved { n: usize },
    /// `/rewind` : retour au checkpoint `to`, les fichiers listés perdent leurs dalles.
    Rewind { to: usize, files: Vec<PathBuf> },
    /// La vue « tous les projets » s'ouvre.
    HubOpen,
    /// Elle se ferme.
    HubClose,
}

/// Un projet (quartier) : sa racine et son arbre.
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub name: String,
    pub tree: Option<scan::ProjectTree>,
}

/// L'état logique d'un fichier dans la scène.
#[derive(Debug, Clone, Default)]
pub struct FileState {
    /// Nombre de dalles (1 ≈ 150 lignes).
    pub slabs: u32,
    /// Instant de la dernière lecture (s), pour le faisceau blanc.
    pub read_at: Option<f64>,
    /// Instant de la dernière écriture (s), pour l'orange.
    pub write_at: Option<f64>,
    /// Le fichier garde son aura orange tant que la modification existe.
    pub changed: bool,
}

/// Un sous-agent dans la scène.
#[derive(Debug, Clone)]
pub struct SubState {
    pub kind: AgentKind,
    pub target: Target,
    pub spawned_at: f64,
    pub done_at: Option<f64>,
    /// Poussé dans le vide (fin de vie).
    pub pushed: bool,
}

/// Kiro sur son perchoir.
#[derive(Debug, Clone)]
pub struct KiroState {
    pub file: Option<PathBuf>,
    pub started_at: f64,
    pub done_at: Option<f64>,
    pub pushed: bool,
}

/// Une vague d'effondrement (`/rewind`).
#[derive(Debug, Clone)]
pub struct RewindState {
    pub at: f64,
    pub files: Vec<PathBuf>,
}

/// Le monde logique : ce que la scène sait, indépendamment du rendu.
#[derive(Debug, Clone, Default)]
pub struct SceneState {
    /// Projets connus (le premier est le projet actif).
    pub projects: Vec<Project>,
    /// Fichiers du projet actif, par chemin.
    pub files: BTreeMap<PathBuf, FileState>,
    /// Sous-agents vivants, par identifiant.
    pub subs: BTreeMap<u32, SubState>,
    /// Fichier que le fantôme principal travaille en ce moment.
    pub main_file: Option<PathBuf>,
    /// Dernière action du fantôme principal (faisceau blanc ou orange).
    pub main_wrote: bool,
    /// Kiro en critique.
    pub kiro: Option<KiroState>,
    /// Étape en cours.
    pub step: Option<Step>,
    /// Vue « tous les projets » ouverte.
    pub hub: bool,
    /// Dernier effondrement.
    pub rewind: Option<RewindState>,
    /// Numéro du dernier checkpoint.
    pub checkpoint: usize,
}

impl SceneState {
    /// Monde vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Ouvre le projet actif : son arbre alimente les dalles.
    pub fn open_project(&mut self, path: &Path, tree: scan::ProjectTree) {
        self.projects.retain(|p| p.root != path);
        self.files.clear();
        for file in &tree.files {
            self.files.insert(
                file.path.clone(),
                FileState {
                    slabs: slabs_for(file.lines),
                    ..FileState::default()
                },
            );
        }
        self.projects.insert(
            0,
            Project {
                root: path.to_path_buf(),
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string()),
                tree: Some(tree),
            },
        );
    }

    /// Dalles d'un fichier (1 minimum, même pour un fichier vide).
    pub fn slabs_of(&self, path: &Path) -> u32 {
        self.files.get(path).map(|f| f.slabs).unwrap_or(1)
    }
}

/// Nombre de dalles pour un nombre de lignes.
pub fn slabs_for(lines: u32) -> u32 {
    (lines.max(1) + LINES_PER_SLAB - 1) / LINES_PER_SLAB
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dalles_par_lignes() {
        assert_eq!(slabs_for(0), 1);
        assert_eq!(slabs_for(1), 1);
        assert_eq!(slabs_for(150), 1);
        assert_eq!(slabs_for(151), 2);
        assert_eq!(slabs_for(600), 4);
    }

    #[test]
    fn etapes_ordonnees() {
        assert_eq!(Step::Ask.rank(), 1);
        assert_eq!(Step::Answer.rank(), 6);
        assert!(Step::Act.rank() < Step::Critic.rank());
    }
}
