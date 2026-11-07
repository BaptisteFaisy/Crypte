//! États d'avancement T2 (purs, testés) : génération (`SpecGen`, 3
//! étapes chaînées) et exécution (`SpecRun`, tâche par tâche via
//! sous-agents). Chaque tâche est déclenchée par une réponse agent ou
//! un `y` explicite : pas de boucle infinie.

use super::spec_prompts;

/// Génération `/spec new` en cours : étape 0, 1, 2 (requirements →
/// design → tasks) + docs déjà produits (nourrissent la suite).
#[derive(Debug, Clone)]
pub struct SpecGen {
    pub name: String,
    pub idea: String,
    pub step: u8,
    pub requirements: String,
    pub design: String,
}

impl SpecGen {
    /// Nouvelle génération (étape requirements, rien produit).
    pub fn new(name: &str, idea: &str) -> Self {
        Self {
            name: name.to_string(),
            idea: idea.to_string(),
            step: 0,
            requirements: String::new(),
            design: String::new(),
        }
    }

    /// Messages LLM de l'étape courante (système + user nourri).
    pub fn messages(&self) -> Vec<(String, String)> {
        let (system, user) = match self.step {
            0 => (
                spec_prompts::requirements_system(),
                spec_prompts::requirements_user(&self.idea),
            ),
            1 => (
                spec_prompts::design_system(),
                spec_prompts::design_user(&self.idea, &self.requirements),
            ),
            _ => (
                spec_prompts::tasks_system(),
                spec_prompts::tasks_user(&self.idea, &self.requirements, &self.design),
            ),
        };
        vec![
            (String::from("system"), system.to_string()),
            (String::from("user"), user),
        ]
    }

    /// Fichier écrit à l'étape courante.
    pub fn file_name(&self) -> &'static str {
        match self.step {
            0 => "requirements.md",
            1 => "design.md",
            _ => "tasks.md",
        }
    }

    /// Nom court de l'étape (messages de progression, en anglais).
    pub fn step_name(&self) -> &'static str {
        match self.step {
            0 => "requirements",
            1 => "design",
            _ => "tasks",
        }
    }

    /// Stocke le doc produit à l'étape courante (nourrit la suite ;
    /// les tâches partent direct au fichier, voir `drain_spec_new`).
    pub fn store(&mut self, text: String) {
        match self.step {
            0 => self.requirements = text,
            1 => self.design = text,
            _ => {}
        }
    }

    /// Passe à l'étape suivante ; faux si c'était la dernière.
    pub fn advance(&mut self) -> bool {
        self.step += 1;
        self.step < 3
    }

    /// Fichiers déjà écrits (message d'échec à mi-chaîne, en anglais).
    pub fn kept_text(&self) -> String {
        match self.step {
            0 => String::from("nothing"),
            1 => String::from("requirements.md"),
            _ => String::from("requirements.md, design.md"),
        }
    }
}

/// Exécution `/spec run` en cours : tâche `index` (0-based) sur
/// `total`, affichée sur `chat`, portée par l'agent `agent` (`None` =
/// en attente de confirmation `y/n`).
#[derive(Debug, Clone)]
pub struct SpecRun {
    pub name: String,
    pub chat: usize,
    pub index: usize,
    pub total: usize,
    pub agent: Option<usize>,
}

impl SpecRun {
    /// Nouveau run (sans agent : posé par `with_agent` au lancement).
    pub fn new(name: &str, chat: usize, index: usize, total: usize) -> Self {
        Self {
            name: name.to_string(),
            chat,
            index,
            total,
            agent: None,
        }
    }

    /// Même run, porté par l'agent `id` (sous-agent `/agent`).
    pub fn with_agent(mut self, id: usize) -> Self {
        self.agent = Some(id);
        self
    }

    /// Étiquette `2/5` (affichage, en anglais comme le reste de l'UI).
    pub fn label(&self) -> String {
        format!("{}/{}", self.index + 1, self.total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen_enchaine_les_trois_etapes_nourries() {
        let mut gen = SpecGen::new("auth", "login");
        assert_eq!((gen.file_name(), gen.step_name()), ("requirements.md", "requirements"));
        assert_eq!(gen.kept_text(), "nothing");
        assert!(gen.messages()[1].1.contains("login"));
        gen.store(String::from("R1"));
        assert!(gen.advance());
        assert_eq!((gen.file_name(), gen.step_name()), ("design.md", "design"));
        assert_eq!(gen.kept_text(), "requirements.md");
        assert!(gen.messages()[1].1.contains("R1"));
        gen.store(String::from("D1"));
        assert!(gen.advance());
        assert_eq!((gen.file_name(), gen.step_name()), ("tasks.md", "tasks"));
        assert!(gen.messages()[1].1.contains("D1"));
        assert!(!gen.advance());
    }

    #[test]
    fn run_etiquette_et_porteur() {
        let run = SpecRun::new("auth", 0, 1, 5);
        assert_eq!(run.label(), "2/5");
        assert_eq!(run.agent, None);
        assert_eq!(run.with_agent(3).agent, Some(3));
    }
}
