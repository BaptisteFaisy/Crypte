//! Handlers `/workflow` (`impl App` déporté).
//!
//! `/workflow` liste les recettes, `show` détaille, `validate`
//! applique les contrôles doc, `run` exécute le sous-ensemble
//! linéaire (étapes plates chaînées via sous-agents, sorties
//! captées, gabarits développés ; `repeat`/`parallel`/`watch`
//! refusés explicitement).

use std::collections::{HashMap, HashSet};

use crate::app::App;
use crate::chat::Role;
use crate::events::Event;
use crate::kiro::workflow_run::WorkflowRun;
use crate::kiro::workflows::Found;

/// Aide `/workflow` (aussi sur sous-commande inconnue).
const WORKFLOW_USAGE: &str =
    "usage: /workflow [list] | show <name> | validate <name> | run <name> [k=v ...]";

impl App {
    /// `/workflow ...` : `list` (défaut), `show`, `validate`, `run`.
    pub(crate) fn cmd_workflow(&mut self, args: Vec<String>) {
        let mut words = args.iter().map(String::as_str);
        match words.next() {
            None | Some("list") => self.flow_list(),
            Some("show") => match words.next() {
                Some(name) => self.flow_show(name),
                None => self.push_system(String::from("usage: /workflow show <name>")),
            },
            Some("validate") => match words.next() {
                Some(name) => self.flow_validate(name),
                None => self.push_system(String::from("usage: /workflow validate <name>")),
            },
            Some("run") => self.flow_run(words.collect()),
            Some("help") => self.push_system(String::from(WORKFLOW_USAGE)),
            Some(other) => {
                self.push_system(format!("unknown /workflow {other} — {WORKFLOW_USAGE}"))
            }
        }
    }

    /// `/workflow list` : recettes (nom — description — étapes).
    fn flow_list(&mut self) {
        let found = self.flow_recipes();
        if found.is_empty() {
            self.push_system(String::from(
                "no workflows — add .kiro/workflows/*.workflow.json",
            ));
            return;
        }
        let mut lines = Vec::new();
        for entry in &found {
            match &entry.recipe {
                Ok(recipe) => {
                    let desc = if recipe.description.trim().is_empty() {
                        String::from("(no description)")
                    } else {
                        recipe.description.clone()
                    };
                    lines.push(format!(
                        "{} — {desc} — {} steps",
                        entry.stem,
                        recipe.steps.len()
                    ));
                }
                Err(reason) => lines.push(format!("{} — {reason}", entry.stem)),
            }
        }
        self.push_system(format!("workflows:\n{}", lines.join("\n")));
    }

    /// `/workflow show` : entrées + squelette des étapes.
    fn flow_show(&mut self, name: &str) {
        let found = self.flow_recipes();
        let Some(entry) = find_recipe(&found, name) else {
            self.push_system(format!("unknown workflow: {name} — /workflow to list"));
            return;
        };
        let Ok(recipe) = &entry.recipe else {
            self.push_system(format!(
                "workflow {name}: {}",
                entry.recipe.as_ref().unwrap_err()
            ));
            return;
        };
        let mut lines = vec![format!("{} — {}", entry.stem, recipe.description)];
        if recipe.inputs.is_empty() {
            lines.push(String::from("inputs: none"));
        } else {
            let mut keys: Vec<&str> = recipe.inputs.keys().map(String::as_str).collect();
            keys.sort_unstable();
            lines.push(format!("inputs: {}", keys.join(", ")));
        }
        for node in &recipe.steps {
            lines.push(format!(
                "  {} '{}' ({})",
                node.kind,
                node.id.as_deref().unwrap_or("?"),
                node.agent.as_deref().unwrap_or("-")
            ));
        }
        self.push_system(lines.join("\n"));
    }

    /// `/workflow validate` : erreurs bloquantes + warnings doc.
    fn flow_validate(&mut self, name: &str) {
        let found = self.flow_recipes();
        let Some(entry) = find_recipe(&found, name) else {
            self.push_system(format!("unknown workflow: {name} — /workflow to list"));
            return;
        };
        let Ok(recipe) = &entry.recipe else {
            self.push_system(format!(
                "workflow {name}: {}",
                entry.recipe.as_ref().unwrap_err()
            ));
            return;
        };
        let customs: HashSet<String> = self
            .flow_agents()
            .iter()
            .map(|agent| agent.name.clone())
            .collect();
        let lints = crate::kiro::workflows::validate(recipe, &customs);
        let (errors, warns): (Vec<_>, Vec<_>) = lints
            .iter()
            .partition(|lint| lint.level == crate::kiro::workflows::Level::Error);
        if errors.is_empty() {
            let mut text = format!("workflow {name}: valid");
            for warn in warns {
                text.push_str(&format!("\n  warning: {}", warn.text));
            }
            self.push_system(text);
        } else {
            let mut lines = vec![format!("workflow {name}: invalid")];
            for err in errors.iter().take(5) {
                lines.push(format!("  error: {}", err.text));
            }
            self.push_system(lines.join("\n"));
        }
    }

    /// `/workflow run` : contrôles puis chaînage (annoncé pas à pas).
    fn flow_run(&mut self, words: Vec<&str>) {
        let Some(name) = words.first() else {
            self.push_system(String::from("usage: /workflow run <name> [k=v ...]"));
            return;
        };
        let found = self.flow_recipes();
        let Some(entry) = find_recipe(&found, name) else {
            self.push_system(format!("unknown workflow: {name} — /workflow to list"));
            return;
        };
        let Ok(recipe) = entry.recipe.clone() else {
            self.push_system(format!(
                "workflow {name}: {}",
                entry.recipe.as_ref().unwrap_err()
            ));
            return;
        };
        let customs = self.flow_agents();
        let names: HashSet<String> = customs.iter().map(|a| a.name.clone()).collect();
        let lints = crate::kiro::workflows::validate(&recipe, &names);
        let errors: Vec<_> = lints
            .iter()
            .filter(|lint| lint.level == crate::kiro::workflows::Level::Error)
            .take(3)
            .map(|lint| lint.text.clone())
            .collect();
        if !errors.is_empty() {
            self.push_system(format!("workflow {name}: invalid — {}", errors.join(" ; ")));
            return;
        }
        let (steps, producers) = match crate::kiro::workflows::linear_steps(&recipe) {
            Ok(pair) => pair,
            Err(reason) => {
                self.push_system(format!("workflow {name}: {reason}"));
                return;
            }
        };
        let (inputs, malformed) = split_inputs(&words[1..]);
        if let Some(bad) = malformed {
            self.push_system(format!(
                "workflow {name}: bad input '{bad}' (want key=value)"
            ));
            return;
        }
        let missing: Vec<&str> = recipe
            .inputs
            .keys()
            .filter(|key| !inputs.contains_key(*key))
            .map(String::as_str)
            .collect();
        if !missing.is_empty() {
            self.push_system(format!(
                "workflow {name}: missing inputs: {}",
                missing.join(", ")
            ));
            return;
        }
        let prompts: HashMap<String, String> = customs
            .iter()
            .map(|agent| (agent.name.clone(), agent.system_prompt.clone()))
            .collect();
        let chat = self.sessions.index();
        let run = WorkflowRun::new(&recipe.name, chat, steps, producers, inputs, prompts);
        self.push_flow_line(
            chat,
            format!(
                "workflow {} started ({} steps)",
                run.recipe,
                run.progress().1
            ),
        );
        self.launch_flow_step(run);
    }

    /// Lance l'étape courante (sous-agent outillé texte seul).
    fn launch_flow_step(&mut self, run: WorkflowRun) {
        let Some((system, user)) = run.prompt_for_current() else {
            self.push_flow_line(
                run.chat,
                format!("workflow {}: unknown agent — run stopped", run.recipe),
            );
            return;
        };
        let chat = run.chat;
        let label = run.label();
        let agent = run
            .current()
            .map(|step| step.agent.clone())
            .unwrap_or_default();
        let history = vec![
            (String::from("system"), system),
            (String::from("user"), user.clone()),
        ];
        let Some(agent_id) = self.sessions.add_agent(chat, &user, history.clone()) else {
            return;
        };
        self.sessions
            .attach_workspace(agent_id, crate::workspace::base().as_deref());
        self.events
            .emit(&Event::agent_spawned(agent_id, chat, chat, &user));
        let recipe = run.recipe.clone();
        self.sessions.workflow_runs.insert(agent_id, run);
        self.push_flow_line(chat, format!("workflow {recipe}: {label} ({agent})"));
        self.hooks_fire_agent_spawn(chat, &user);
        self.launch_agent(chat, agent_id, chat, history, None);
    }

    /// Étape finie (réponse agent) : sortie captée, suivante ou fin.
    /// Hors workflow : rien.
    pub(crate) fn workflow_agent_done(&mut self, agent_id: usize, text: &str) {
        let Some(mut run) = self.sessions.workflow_runs.remove(&agent_id) else {
            return;
        };
        run.record_output(text);
        self.push_flow_line(
            run.chat,
            format!("workflow {}: {} done", run.recipe, run.label()),
        );
        if !run.advance() {
            let (_, total) = run.progress();
            self.push_flow_line(
                run.chat,
                format!("workflow {} complete ({total}/{total})", run.recipe),
            );
            return;
        }
        self.launch_flow_step(run);
    }

    /// Étape échouée (agent en erreur) : run stoppé. Hors workflow : rien.
    pub(crate) fn workflow_agent_failed(&mut self, agent_id: usize) {
        if let Some(run) = self.sessions.workflow_runs.remove(&agent_id) {
            self.push_flow_line(
                run.chat,
                format!(
                    "workflow {}: {} failed — run stopped",
                    run.recipe,
                    run.label()
                ),
            );
        }
    }

    /// Recettes projet + global (voir `t5`, comme `/skills`).
    fn flow_recipes(&self) -> Vec<Found> {
        crate::kiro::workflows::load_all_in(&self.t5.kiro_project, self.t5.kiro_global.as_deref())
    }

    /// Agents customs projet + global (noms + consignes).
    fn flow_agents(&self) -> Vec<crate::kiro::agents_md::CustomAgent> {
        crate::kiro::agents_md::load_all_in(&self.t5.kiro_project, self.t5.kiro_global.as_deref())
    }

    /// Ligne workflow dans le chat (inconnu : ignorée).
    fn push_flow_line(&mut self, chat: usize, line: String) {
        if let Some(sess) = self.sessions.get_mut(chat) {
            sess.chat.push(Role::System, line);
        }
    }
}

/// Recette par radical ou nom (`None` = inconnue).
fn find_recipe<'a>(found: &'a [Found], name: &str) -> Option<&'a Found> {
    found.iter().find(|entry| entry.stem == name).or_else(|| {
        found
            .iter()
            .find(|entry| entry.recipe.as_ref().map(|recipe| recipe.name.as_str()) == Ok(name))
    })
}

/// Découpe `k=v...` : la valeur mange les mots suivants jusqu'à la
/// prochaine clé (`Malformed` = mot nu sans clé devant).
pub(crate) fn split_inputs(words: &[&str]) -> (HashMap<String, String>, Option<String>) {
    let mut inputs = HashMap::new();
    let mut key: Option<String> = None;
    for word in words {
        match word.split_once('=') {
            Some((name, value)) if !name.trim().is_empty() => {
                key = Some(name.trim().to_string());
                inputs.insert(name.trim().to_string(), value.trim().to_string());
            }
            _ => match &key {
                Some(active) => {
                    let entry = inputs.entry(active.clone()).or_default();
                    if !entry.is_empty() {
                        entry.push(' ');
                    }
                    entry.push_str(word.trim());
                }
                None => return (inputs, Some((*word).to_string())),
            },
        }
    }
    (inputs, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recette jouet (plan + exécution, entrée `task`).
    const RECIPE: &str = r#"{"name": "demo", "description": "Démo.",
        "inputs": {"task": "prompt"},
        "steps": [
          {"type": "step", "id": "plan", "agent": "wf-planner",
           "prompt": "Plan: {{task}}"},
          {"type": "step", "id": "do", "agent": "wf-coder",
           "prompt": "Go: {{plan.output}}"}]}"#;

    /// App + `.kiro/workflows/w.workflow.json` (+ `r` non linéaire).
    fn fixture(tag: &str) -> (App, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("kh-wf-cmds-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".kiro/workflows")).unwrap();
        std::fs::write(dir.join(".kiro/workflows/w.workflow.json"), RECIPE).unwrap();
        std::fs::write(
            dir.join(".kiro/workflows/r.workflow.json"),
            r#"{"name": "r", "steps": [{"type": "repeat", "id": "x",
               "maxIterations": 3, "steps": []}]}"#,
        )
        .unwrap();
        let cfg = crate::provider::ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        let mut app = App::from_config(cfg).unwrap();
        app.t5.kiro_project = dir.clone();
        app.t5.kiro_global = None;
        (app, dir)
    }

    /// Tout le chat courant (asserts `contains` multi-lignes).
    fn all_text(app: &App) -> String {
        app.sessions
            .get(0)
            .unwrap()
            .chat
            .messages()
            .iter()
            .map(|msg| msg.text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `list`/`show`/`validate` affichent la recette jouet.
    #[test]
    fn list_show_validate_jouet() {
        let (mut app, dir) = fixture("lsv");
        app.run_command("/workflow");
        assert!(
            all_text(&app).contains("w — Démo. — 2 steps"),
            "{}",
            all_text(&app)
        );
        app.run_command("/workflow show w");
        assert!(
            all_text(&app).contains("inputs: task"),
            "{}",
            all_text(&app)
        );
        app.run_command("/workflow validate w");
        assert!(all_text(&app).contains("valid"), "{}", all_text(&app));
        app.run_command("/workflow validate nope");
        assert!(
            all_text(&app).contains("unknown workflow"),
            "{}",
            all_text(&app)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `run` refuse : inconnue, entrées manquantes, non linéaire.
    #[tokio::test]
    async fn run_refus_clairs() {
        let (mut app, dir) = fixture("refus");
        app.run_command("/workflow run nope task=x");
        assert!(
            all_text(&app).contains("unknown workflow"),
            "{}",
            all_text(&app)
        );
        app.run_command("/workflow run w");
        assert!(
            all_text(&app).contains("missing inputs: task"),
            "{}",
            all_text(&app)
        );
        app.run_command("/workflow run r");
        assert!(
            all_text(&app).contains("'repeat' nodes don't run here"),
            "{}",
            all_text(&app)
        );
        app.run_command("/workflow run w task=hello world");
        assert!(all_text(&app).contains("started"), "{}", all_text(&app));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Valeurs multi-mots : mangées jusqu'à la prochaine clé.
    #[test]
    fn split_inputs_mots_et_malforme() {
        let (inputs, bad) = split_inputs(&["task=hello", "world", "n=1"]);
        assert_eq!(inputs.get("task").map(String::as_str), Some("hello world"));
        assert_eq!(inputs.get("n").map(String::as_str), Some("1"));
        assert!(bad.is_none());
        let (_, bad) = split_inputs(&["nu"]);
        assert_eq!(bad.as_deref(), Some("nu"));
    }

    /// Run complet : 2 étapes chaînées par réponses simulées.
    #[tokio::test]
    async fn run_chaine_deux_etapes_puis_finit() {
        let (mut app, dir) = fixture("chaine");
        app.run_command("/workflow run w task=hello");
        assert_eq!(app.sessions.workflow_runs.len(), 1);
        assert!(
            all_text(&app).contains("step 1/2 'plan'"),
            "{}",
            all_text(&app)
        );
        app.workflow_agent_done(1, "PLAN");
        assert_eq!(app.sessions.workflow_runs.len(), 1);
        assert!(
            all_text(&app).contains("step 2/2 'do'"),
            "{}",
            all_text(&app)
        );
        app.workflow_agent_done(2, "DONE");
        assert!(app.sessions.workflow_runs.is_empty());
        assert!(
            all_text(&app).contains("complete (2/2)"),
            "{}",
            all_text(&app)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Échec : run stoppé, tâche suivante jamais lancée.
    #[tokio::test]
    async fn run_echec_stoppe() {
        let (mut app, dir) = fixture("echec");
        app.run_command("/workflow run w task=hello");
        app.workflow_agent_failed(1);
        assert!(app.sessions.workflow_runs.is_empty());
        assert!(
            all_text(&app).contains("failed — run stopped"),
            "{}",
            all_text(&app)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
