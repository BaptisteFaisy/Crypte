//! Sous-agents : requête annexe dont la réponse arrive en async.
//!
//! À ne pas confondre avec `crate::agents` (contexte système agents.md,
//! chargé au démarrage et pinné en tête d'historique).

use std::collections::VecDeque;
use std::path::PathBuf;

use crate::bus::QueuedAsk;

/// Statut d'un sous-agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Running,
    Done,
    Failed,
}

impl AgentStatus {
    /// Étiquette courte française (liste `/agents`).
    pub fn label(self) -> &'static str {
        match self {
            AgentStatus::Running => "running",
            AgentStatus::Done => "done",
            AgentStatus::Failed => "failed",
        }
    }
}

/// Échec d'une suite de conversation `/ask` : id inconnu ou déjà en cours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskError {
    Unknown,
    Busy,
}

/// Tâche de workflow `/spec` portée par l'agent (`None` = lancé seul).
#[derive(Debug, Clone)]
pub struct SpecTag {
    pub name: String,
    pub index: usize,
    pub total: usize,
}

impl SpecTag {
    /// Étiquette `2/5` (tâche courante sur total, 1-based).
    pub fn task_label(&self) -> String {
        format!("{}/{}", self.index + 1, self.total)
    }
}

/// Un sous-agent : requête annexe dont la réponse arrive en async.
/// `history` garde l'échange user/assistant (`/ask` : suite de
/// conversation) ; le système agents.md n'y est pas stocké
/// (re-préfixé à chaque appel, voir `launch_agent`).
#[derive(Debug, Clone)]
pub struct SubAgent {
    pub id: usize,
    /// Extrait du prompt (tronqué à la création).
    pub prompt: String,
    pub status: AgentStatus,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub history: Vec<(String, String)>,
    /// Asks en attente (déclenchés dès que l'agent est libre).
    pub inbox: VecDeque<QueuedAsk>,
    /// Relais auto de l'appel en vol (0 = déclenché par le user).
    pub auto_hops: u8,
    /// Workspace disque (`None` si pas créé : HOME absent ou échec).
    pub workspace: Option<PathBuf>,
    /// Workflow `/spec` porteur (`None` = lancé seul via `/agent`).
    pub spec: Option<SpecTag>,
}

impl SubAgent {
    /// Nouveau sous-agent `Running`, prompt tronqué à 60 caractères.
    pub fn new(id: usize, prompt: &str, history: Vec<(String, String)>) -> Self {
        Self {
            id,
            prompt: prompt.chars().take(AGENT_PROMPT_LEN).collect(),
            status: AgentStatus::Running,
            prompt_tokens: 0,
            completion_tokens: 0,
            history,
            inbox: VecDeque::new(),
            auto_hops: 0,
            workspace: None,
            spec: None,
        }
    }

    /// Ajoute un message user à l'échange (borné, tête system gardée).
    pub fn push_user(&mut self, text: String) {
        self.history.push((String::from("user"), text));
        trim_agent_history(&mut self.history);
    }

    /// Ajoute la réponse assistant à l'échange (borné, tête system gardée).
    pub fn push_assistant(&mut self, text: String) {
        self.history.push((String::from("assistant"), text));
        trim_agent_history(&mut self.history);
    }

    /// Échange complet, prêt à afficher (`/agents #id`, déplié inline)
    /// : en-tête `#id statut — prompt` (+ ligne `spec …` si workflow)
    /// puis les tours `you:` / `agent:` (texte assistant rendu comme au
    /// chat, rôles `system` ignorés).
    pub fn transcript(&self) -> String {
        let mut out = format!("#{} {} — {}", self.id, self.status.label(), self.prompt);
        if let Some(spec) = &self.spec {
            out.push_str(&format!(
                "\nspec {} · task {}",
                spec.name,
                spec.task_label()
            ));
        }
        for (role, text) in &self.history {
            match role.as_str() {
                "user" => {
                    out.push_str("\nyou: ");
                    out.push_str(text);
                }
                "assistant" => {
                    out.push_str("\nagent: ");
                    out.push_str(&crate::question::render_display(text).0);
                }
                _ => {}
            }
        }
        out
    }
}

/// Longueur max de l'extrait de prompt d'un sous-agent.
const AGENT_PROMPT_LEN: usize = 60;
/// Échanges max gardés par agent (au-delà, les plus vieux sortent).
const AGENT_HISTORY_MAX: usize = 20;

/// Borne l'historique agent : sort les plus vieux, garde la tête `system`.
fn trim_agent_history(history: &mut Vec<(String, String)>) {
    while history.len() > AGENT_HISTORY_MAX {
        let pinned = history
            .first()
            .map(|(role, _)| role == "system")
            .unwrap_or(false);
        history.remove(usize::from(pinned));
    }
}

/// Échange minimal : user seul (partagé avec les tests `super`).
#[cfg(test)]
pub(crate) fn user(text: &str) -> Vec<(String, String)> {
    vec![(String::from("user"), text.to_string())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_history_bornee_garde_la_tete_system() {
        let mut a = SubAgent::new(1, "p", vec![(String::from("system"), String::from("ctx"))]);
        for i in 0..25 {
            a.push_user(format!("u{i}"));
            a.push_assistant(format!("a{i}"));
        }
        assert_eq!(a.history.len(), AGENT_HISTORY_MAX);
        assert_eq!(a.history[0].0, "system");
        // 51 entrées → 31 plus vieilles sorties : reste [system, a15…a24].
        assert_eq!(
            a.history[1],
            (String::from("assistant"), String::from("a15"))
        );
        assert_eq!(
            a.history[AGENT_HISTORY_MAX - 1],
            (String::from("assistant"), String::from("a24"))
        );
    }

    #[test]
    fn agent_history_sans_system_sort_par_la_tete() {
        let mut a = SubAgent::new(1, "p", user("u0"));
        for i in 1..30 {
            a.push_user(format!("u{i}"));
        }
        assert_eq!(a.history.len(), AGENT_HISTORY_MAX);
        assert_eq!(a.history[0].1, "u10");
    }

    #[test]
    fn transcript_montre_tours_et_ignore_system() {
        let mut a = SubAgent::new(1, "fais ça", user("fais ça"));
        a.history
            .insert(0, (String::from("system"), String::from("contexte")));
        a.push_assistant(String::from("voilà"));
        a.push_user(String::from("et ça ?"));
        let t = a.transcript();
        assert!(t.starts_with("#1 running — fais ça"), "{t}");
        assert!(t.contains("\nyou: fais ça\n"), "{t}");
        assert!(t.contains("\nagent: voilà\n"), "{t}");
        assert!(t.contains("\nyou: et ça ?"), "{t}");
        assert!(!t.contains("contexte"), "{t}");
    }
}
