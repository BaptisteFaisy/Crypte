//! État d'un run workflow (pur, testé) : étapes linéaires chaînées
//! via sous-agents, sorties captées, gabarits développés à chaque
//! pas. Chaque étape est déclenchée par la réponse agent
//! précédente : pas de boucle infinie.

use std::collections::HashMap;

use super::workflows::{expand, Ctx, LinearStep};

/// Consignes des agents embarqués (courtes, maison — les customs
/// viennent de `.kiro/agents/`).
fn bundled_system(agent: &str) -> Option<&'static str> {
    match agent {
        "wf-planner" => Some("Plan only: analyse and write a plan, change nothing."),
        "wf-coder" => Some("Implement the task, then run the relevant checks."),
        "wf-design" => Some("Produce the design requested, no code."),
        "wf-design-reviewer" => Some("Review the design critically, list issues."),
        "wf-review-aggregator" => Some("Merge the reviews into one verdict."),
        "wf-pr-submitter" => Some("Summarize the change as a pull-request description."),
        "wf-pr-responder" => Some("Answer the review comments precisely."),
        "wf-auto-researcher" => Some("Research and report findings with sources."),
        "semantic_reviewer" => Some("Review the semantics, ignore style."),
        _ => None,
    }
}

/// Consigne système d'un agent (embarqué ou custom, `None` = inconnu).
pub fn agent_system(agent: &str, customs: &HashMap<String, String>) -> Option<String> {
    if let Some(prompt) = bundled_system(agent) {
        return Some(prompt.to_string());
    }
    customs.get(agent).cloned()
}

/// Run en cours : recette, chat, étapes, entrées, sorties captées.
#[derive(Debug, Clone)]
pub struct WorkflowRun {
    /// Nom de la recette (annonces).
    pub recipe: String,
    /// Chat propriétaire (annonces + livraison).
    pub chat: usize,
    steps: Vec<LinearStep>,
    /// Artefact → étape productrice (émulation texte).
    producers: HashMap<String, String>,
    inputs: HashMap<String, String>,
    /// Sorties captées (étapes + artefacts).
    outputs: HashMap<String, String>,
    /// Consignes customs `nom → prompt`.
    agents: HashMap<String, String>,
    idx: usize,
}

impl WorkflowRun {
    /// Nouveau run (étape 0, rien capté).
    pub fn new(
        recipe: &str,
        chat: usize,
        steps: Vec<LinearStep>,
        producers: HashMap<String, String>,
        inputs: HashMap<String, String>,
        agents: HashMap<String, String>,
    ) -> Self {
        Self {
            recipe: recipe.to_string(),
            chat,
            steps,
            producers,
            inputs,
            outputs: HashMap::new(),
            agents,
            idx: 0,
        }
    }

    /// Étape courante (`None` = terminé).
    pub fn current(&self) -> Option<&LinearStep> {
        self.steps.get(self.idx)
    }

    /// Messages LLM de l'étape courante (gabarits développés).
    pub fn prompt_for_current(&self) -> Option<(String, String)> {
        let step = self.current()?;
        let system = agent_system(&step.agent, &self.agents)?;
        let previous = match self.idx.checked_sub(1) {
            Some(prev) => self
                .steps
                .get(prev)
                .and_then(|s| self.outputs.get(&s.id).map(String::as_str)),
            None => None,
        };
        let ctx = Ctx {
            inputs: &self.inputs,
            outputs: &self.outputs,
            previous,
        };
        Some((system, expand(&step.prompt, &ctx)))
    }

    /// Enregistre la sortie de l'étape courante (étape + artefacts).
    pub fn record_output(&mut self, text: &str) {
        let Some(step) = self.steps.get(self.idx) else {
            return;
        };
        self.outputs.insert(step.id.clone(), text.to_string());
        let produced: Vec<String> = self
            .producers
            .iter()
            .filter(|(_, producer)| *producer == &step.id)
            .map(|(name, _)| name.clone())
            .collect();
        for name in produced {
            self.outputs.insert(name, text.to_string());
        }
    }

    /// Avance (`false` = run terminé).
    pub fn advance(&mut self) -> bool {
        self.idx += 1;
        self.idx < self.steps.len()
    }

    /// Progression (`finies`, `total`).
    pub fn progress(&self) -> (usize, usize) {
        (self.idx.min(self.steps.len()), self.steps.len())
    }

    /// Étiquette d'annonce (`step 2/3 'id'`).
    pub fn label(&self) -> String {
        let id = self.current().map(|s| s.id.as_str()).unwrap_or("?");
        format!("step {}/{} '{id}'", self.idx + 1, self.steps.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run jouet (2 étapes, 1 entrée, 1 artefact).
    fn toy() -> WorkflowRun {
        WorkflowRun::new(
            "w",
            0,
            vec![
                LinearStep {
                    id: String::from("plan"),
                    agent: String::from("wf-planner"),
                    prompt: String::from("Plan: {{task}}"),
                },
                LinearStep {
                    id: String::from("do"),
                    agent: String::from("wf-coder"),
                    prompt: String::from("Go: {{plan.output}} + {{artifacts.doc}}"),
                },
            ],
            HashMap::from([(String::from("doc"), String::from("plan"))]),
            HashMap::from([(String::from("task"), String::from("T"))]),
            HashMap::new(),
        )
    }

    /// Gabarits développés pas à pas (entrées, sorties, artefacts).
    #[test]
    fn prompt_et_sorties_chainent() {
        let mut run = toy();
        let (system, user) = run.prompt_for_current().unwrap();
        assert!(system.contains("Plan only"), "{system}");
        assert_eq!(user, "Plan: T");
        run.record_output("P");
        assert!(run.advance());
        let (_, user) = run.prompt_for_current().unwrap();
        assert_eq!(user, "Go: P + P");
        assert_eq!(run.label(), "step 2/2 'do'");
        run.record_output("D");
        assert!(!run.advance());
        assert!(run.prompt_for_current().is_none());
        assert_eq!(run.progress(), (2, 2));
    }

    /// Agents : embarqués + customs, inconnu refusé.
    #[test]
    fn agent_system_embarque_custom_inconnu() {
        let customs = HashMap::from([(String::from("moi"), String::from("Sois moi."))]);
        assert!(agent_system("wf-coder", &customs).is_some());
        assert_eq!(agent_system("moi", &customs).as_deref(), Some("Sois moi."));
        assert!(agent_system("fantome", &customs).is_none());
    }
}
