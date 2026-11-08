//! Recettes workflows Kiro (`.kiro/workflows/*.workflow.json`).
//!
//! Format officiel (kiro.dev/docs/workflows) : `name`, `steps`,
//! `description`, `inputs`, types `step`/`sequence`/`repeat`/
//! `parallel`/`watch`, gabarits `{{…}}`. JSON uniquement (pas de
//! YAML sans dépendance) ; projet prime sur global, JSON prime
//! sur YAML. `validate` porte les contrôles doc ; `linear_steps`
//! extrait le sous-ensemble exécutable ici (étapes plates, pas de
//! `repeat`/`parallel`/`watch`, pas de conditions).

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Nœuds `step` max par recette (doc).
const MAX_STEPS: usize = 50;
/// Profondeur max des conteneurs (doc).
const MAX_DEPTH: usize = 8;

/// Agents embarqués valides (doc) ; le reste vient de `.kiro/agents/`.
const BUNDLED_AGENTS: &[&str] = &[
    "wf-planner",
    "wf-coder",
    "wf-design",
    "wf-design-reviewer",
    "wf-review-aggregator",
    "wf-pr-submitter",
    "wf-pr-responder",
    "wf-auto-researcher",
    "semantic_reviewer",
];

/// Recette chargée (champs inconnus ignorés par serde).
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    /// Nom requis (vide = invalide).
    #[serde(default)]
    pub name: String,
    /// Description affichée par `/workflow list`.
    #[serde(default)]
    pub description: String,
    /// Entrées `nom → indication` (indications non contraintes).
    #[serde(default)]
    pub inputs: HashMap<String, String>,
    /// Étapes de tête (séquence implicite).
    #[serde(default)]
    pub steps: Vec<Node>,
    /// Modèle par défaut (lu, ignoré : modèle courant).
    #[serde(default)]
    pub model_id: Option<String>,
    /// Effort par défaut (lu, ignoré : thinking courant).
    #[serde(default)]
    pub effort_level: Option<String>,
}

/// Nœud de recette (champs optionnels par type, inconnus ignorés).
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    /// `step`, `sequence`, `repeat`, `parallel`, `watch`, autre.
    #[serde(rename = "type", default)]
    pub kind: String,
    /// Id unique dans la recette (requis par la doc).
    #[serde(default)]
    pub id: Option<String>,
    /// Étape : agent porteur (`wf-*` ou `.kiro/agents/`).
    #[serde(default)]
    pub agent: Option<String>,
    /// Étape : consigne (gabarits `{{…}}`).
    #[serde(default)]
    pub prompt: Option<String>,
    /// Enfants (`sequence`/`repeat`, étapes du `repeat`).
    #[serde(default)]
    pub steps: Vec<Node>,
    /// Branches (`parallel`).
    #[serde(default)]
    pub branches: Vec<Node>,
    /// Artefacts `nom → fichier` (étape).
    #[serde(default)]
    pub artifacts: HashMap<String, String>,
    /// Itérations max (`repeat`).
    #[serde(default)]
    pub max_iterations: Option<u64>,
    /// Dépassement (`repeat`) : `abort`/`continue`/`pause`.
    #[serde(default)]
    pub on_max_iterations: Option<String>,
    /// Politique de jointure (`parallel`).
    #[serde(default)]
    pub join_policy: Option<String>,
    /// Conditions d'arrêt (exclusives, `repeat`).
    #[serde(default)]
    pub stop_condition: Option<serde_json::Value>,
    /// Condition textuelle (`repeat`).
    #[serde(default)]
    pub stop_when: Option<String>,
    /// Condition de fin (étape).
    #[serde(default)]
    pub completion: Option<serde_json::Value>,
}

/// Recette trouvée sur disque (erreur = JSON invalide ou YAML).
#[derive(Debug, Clone)]
pub struct Found {
    /// Radical du fichier (sans `.workflow.json`).
    pub stem: String,
    /// Recette, ou raison (`invalid json …`, `yaml unsupported`).
    pub recipe: Result<Recipe, String>,
}

/// Charge projet + global (`*.workflow.json`, projet gagne à radical
/// égal ; radicaux YAML signalés non supportés). Triés par radical.
pub fn load_all_in(project: &Path, global: Option<&Path>) -> Vec<Found> {
    let mut by_stem: HashMap<String, Found> = HashMap::new();
    if let Some(global) = global {
        for found in load_dir(&global.join("workflows")) {
            by_stem.insert(found.stem.clone(), found);
        }
    }
    for found in load_dir(&project.join(".kiro/workflows")) {
        by_stem.insert(found.stem.clone(), found);
    }
    let mut out: Vec<Found> = by_stem.into_values().collect();
    out.sort_by(|a, b| a.stem.cmp(&b.stem));
    out
}

/// Charge un dossier (JSON parsés, YAML marqués, reste ignoré).
fn load_dir(dir: &Path) -> Vec<Found> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if let Some(stem) = name.strip_suffix(".workflow.json") {
            let recipe = std::fs::read_to_string(&path)
                .map_err(|err| format!("unreadable: {err}"))
                .and_then(|raw| {
                    serde_json::from_str::<Recipe>(&raw)
                        .map_err(|err| format!("invalid json: {err}"))
                });
            out.push(Found {
                stem: stem.to_string(),
                recipe,
            });
        } else if name.ends_with(".workflow.yaml") || name.ends_with(".workflow.yml") {
            let stem = name
                .rsplit_once('.')
                .and_then(|(base, _)| base.strip_suffix(".workflow"))
                .unwrap_or(name);
            out.push(Found {
                stem: stem.to_string(),
                recipe: Err(String::from("yaml unsupported (json only)")),
            });
        }
    }
    out
}

/// Gravité d'un lint `validate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
}

/// Remarque `validate` (erreurs bloquent `/workflow run`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lint {
    pub level: Level,
    pub text: String,
}

/// Valide une recette (contrôles doc : ids, caps, gabarits, agents ;
/// config `watch` non vérifiée, comme `validate_workflow`).
pub fn validate(recipe: &Recipe, custom_agents: &HashSet<String>) -> Vec<Lint> {
    let mut check = Check {
        recipe,
        custom_agents,
        seen: HashSet::new(),
        count: 0,
        lints: Vec::new(),
    };
    if recipe.name.trim().is_empty() {
        check.err("missing recipe name");
    }
    if recipe.steps.is_empty() {
        check.err("no steps");
    }
    if non_auto(&recipe.model_id) {
        check.warn("modelId ignored here (current model used)");
    }
    if non_auto(&recipe.effort_level) {
        check.warn("effortLevel ignored here (current thinking used)");
    }
    for (idx, node) in recipe.steps.iter().enumerate() {
        check.node(node, 1, idx == 0);
    }
    if check.count > MAX_STEPS {
        let count = check.count;
        check.err(format!("{count} steps (max {MAX_STEPS})"));
    }
    check.lints
}

/// Défaut explicite (renseigné, non `auto` = hérité du parent).
fn non_auto(value: &Option<String>) -> bool {
    value
        .as_deref()
        .is_some_and(|v| !v.trim().is_empty() && v != "auto")
}

/// État du parcours de validation (évite 8 paramètres).
struct Check<'a> {
    recipe: &'a Recipe,
    custom_agents: &'a HashSet<String>,
    seen: HashSet<String>,
    count: usize,
    lints: Vec<Lint>,
}

impl Check<'_> {
    /// Lint erreur (texte statique ou formaté).
    fn err(&mut self, text: impl Into<String>) {
        self.lints.push(Lint {
            level: Level::Error,
            text: text.into(),
        });
    }

    /// Lint avertissement.
    fn warn(&mut self, text: impl Into<String>) {
        self.lints.push(Lint {
            level: Level::Warning,
            text: text.into(),
        });
    }

    /// Contrôle récursif (profondeur, ids, types, gabarits, agents).
    fn node(&mut self, node: &Node, depth: usize, first: bool) {
        let id = node.id.clone().unwrap_or_else(|| String::from("?"));
        if depth > MAX_DEPTH {
            self.err(format!("depth over {MAX_DEPTH} at '{id}'"));
            return;
        }
        if let Some(own) = node.id.as_deref() {
            if !self.seen.insert(own.to_string()) {
                self.err(format!("duplicate id '{own}'"));
            }
        } else {
            self.err("node without id");
        }
        match node.kind.as_str() {
            "step" => self.step(node, &id, first),
            "sequence" => {
                for (idx, child) in node.steps.iter().enumerate() {
                    self.node(child, depth + 1, first && idx == 0);
                }
            }
            "repeat" => {
                if node.max_iterations.is_none() {
                    self.err("repeat without maxIterations");
                }
                if let Some(on) = node.on_max_iterations.as_deref() {
                    if !["abort", "continue", "pause"].contains(&on) {
                        self.err(format!("bad onMaxIterations '{on}'"));
                    }
                }
                if node.stop_condition.is_some() && node.stop_when.is_some() {
                    self.err("repeat with both stopCondition and stopWhen");
                }
                for (idx, child) in node.steps.iter().enumerate() {
                    self.node(child, depth + 1, first && idx == 0);
                }
            }
            "parallel" => {
                match node.join_policy.as_deref().map(str::trim) {
                    None | Some("") => self.err("parallel without joinPolicy"),
                    Some(policy) if !["all", "allSettled", "any"].contains(&policy) => {
                        self.err(format!("bad joinPolicy '{policy}'"));
                    }
                    Some(_) => {}
                }
                for child in &node.branches {
                    forbid_previous(child, &mut self.lints);
                    self.node(child, depth + 1, false);
                }
            }
            // Config non vérifiée (comme `validate_workflow`).
            "watch" => {}
            other => self.err(format!("unknown node type '{other}'")),
        }
    }

    /// Contrôle d'étape (agent connu, consigne, gabarits placés).
    fn step(&mut self, node: &Node, id: &str, first: bool) {
        self.count += 1;
        match node.agent.as_deref().map(str::trim) {
            None | Some("") => self.err(format!("step '{id}' without agent")),
            Some(agent) => {
                if !BUNDLED_AGENTS.contains(&agent) && !self.custom_agents.contains(agent) {
                    self.err(format!("unknown agent '{agent}'"));
                }
            }
        }
        match node.prompt.as_deref().map(str::trim) {
            None | Some("") => self.err(format!("step '{id}' without prompt")),
            Some(_) => self.templates(node.prompt.as_deref().unwrap_or(""), first),
        }
    }

    /// Gabarits d'une consigne : `{{previous.output}}` interdit en
    /// premier, `{{bare}}` déclaré dans `inputs` (sinon warning).
    fn templates(&mut self, prompt: &str, first: bool) {
        for raw in template_refs(prompt) {
            if raw == "previous.output" {
                if first {
                    self.err("{{previous.output}} in first node");
                }
                continue;
            }
            if let Some(id) = strip_output(&raw) {
                if !self.seen.contains(id) {
                    self.err(format!("{{{{{raw}}}}} refers to non-prior '{id}'"));
                }
                continue;
            }
            if raw.starts_with("artifacts.") {
                continue;
            }
            if !self.recipe.inputs.contains_key(raw.as_str()) {
                self.warn(format!(
                    "{{{{{raw}}}}} not declared in inputs (stays literal)"
                ));
            }
        }
    }
}

/// `{{previous.output}}` interdit dans les branches parallèles (doc).
fn forbid_previous(node: &Node, lints: &mut Vec<Lint>) {
    let prompt = node.prompt.as_deref().unwrap_or("");
    if template_refs(prompt)
        .iter()
        .any(|raw| raw == "previous.output")
    {
        lints.push(Lint {
            level: Level::Error,
            text: String::from("{{previous.output}} inside parallel branches"),
        });
    }
    for child in node.steps.iter().chain(node.branches.iter()) {
        forbid_previous(child, lints);
    }
}

/// Id producteur d'une réf `{{id.output}}` (`steps.` toléré).
fn strip_output(raw: &str) -> Option<&str> {
    raw.strip_suffix(".output")
        .map(|rest| rest.strip_prefix("steps.").unwrap_or(rest))
}

/// Références `{{…}}` d'un texte (brutes, sans les accolades).
fn template_refs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            break;
        };
        let raw = after[..end].trim();
        if !raw.is_empty() {
            out.push(raw.to_string());
        }
        rest = &after[end + 2..];
    }
    out
}

/// Contexte d'expansion des gabarits (run).
pub struct Ctx<'a> {
    /// Entrées `run` (`{{nom}}`).
    pub inputs: &'a HashMap<String, String>,
    /// Sorties captées (`{{id.output}}`, `{{artifacts.nom}}`).
    pub outputs: &'a HashMap<String, String>,
    /// Dernière sortie (`{{previous.output}}`).
    pub previous: Option<&'a str>,
}

/// Développe les gabarits (inconnus gardés littéraux, doc).
pub fn expand(template: &str, ctx: &Ctx<'_>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let raw = after[..end].trim().to_string();
        out.push_str(&resolve(&raw, ctx));
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Résout une référence (littéral `{{…}}` si inconnue).
fn resolve(raw: &str, ctx: &Ctx<'_>) -> String {
    if raw == "previous.output" {
        return ctx.previous.unwrap_or("").to_string();
    }
    if let Some(id) = strip_output(raw) {
        return ctx.outputs.get(id).cloned().unwrap_or_default();
    }
    if let Some(name) = raw.strip_prefix("artifacts.") {
        // Producteur inconnu ou pas encore passé : littéral visible
        // (pas de contrôle d'ordre ici, contrairement aux sorties).
        if let Some(out) = ctx.outputs.get(name) {
            return out.clone();
        }
    } else if let Some(value) = ctx.inputs.get(raw) {
        return value.clone();
    }
    format!("{{{{{raw}}}}}")
}

/// Étape linéaire exécutable (`step` plat ou aplati).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearStep {
    pub id: String,
    pub agent: String,
    pub prompt: String,
}

/// Extrait les étapes exécutables ici (aplatit les `sequence`) +
/// table `artefact → étape productrice` (émulation texte : pas
/// d'écriture disque, l'artefact vaut la sortie productrice).
/// `repeat`/`parallel`/`watch`/inconnu/conditions → refus explicite.
pub fn linear_steps(recipe: &Recipe) -> Result<(Vec<LinearStep>, HashMap<String, String>), String> {
    let mut out = Vec::new();
    let mut producers = HashMap::new();
    flatten(&recipe.steps, &mut out, &mut producers)?;
    if out.is_empty() {
        return Err(String::from("no runnable steps"));
    }
    Ok((out, producers))
}

/// Aplatissement (remplit `artefact → étape productrice`).
fn flatten(
    nodes: &[Node],
    out: &mut Vec<LinearStep>,
    producers: &mut HashMap<String, String>,
) -> Result<(), String> {
    for node in nodes {
        match node.kind.as_str() {
            "step" => {
                let id = node.id.clone().unwrap_or_default();
                if node.completion.is_some() {
                    return Err(format!(
                        "step '{id}' has completion conditions (unsupported)"
                    ));
                }
                if node.stop_condition.is_some() || node.stop_when.is_some() {
                    return Err(format!("step '{id}' has stop conditions (unsupported)"));
                }
                for name in node.artifacts.keys() {
                    producers.insert(name.clone(), id.clone());
                }
                out.push(LinearStep {
                    id,
                    agent: node.agent.clone().unwrap_or_default(),
                    prompt: node.prompt.clone().unwrap_or_default(),
                });
            }
            "sequence" => flatten(&node.steps, out, producers)?,
            other => {
                return Err(format!("'{other}' nodes don't run here (flat steps only)"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
