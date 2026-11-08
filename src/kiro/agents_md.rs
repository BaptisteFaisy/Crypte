//! Agents customs Kiro (`.kiro/agents/`, projet + global).
//!
//! Schéma officiel : `.json` ou `.md` à champs identiques (`name`,
//! `description`, `prompt`, `model`, `tools`, `excludedTools`,
//! `mcpServers`) ; en Markdown le corps est le prompt système. Nom =
//! `name` explicite sinon tige (`team/planner.md` → `team/planner`) ;
//! `.json` prime sur `.md` à tige égale ; le projet écrase le global.
//! `system` reste un alias legacy de `prompt`. Champs du harnais
//! (`resources`, `permissions`, ...) lus et ignorés. `tools` et
//! `mcpServers` sont lus et affichés (`/agents`) mais non exécutés :
//! les sous-agents n'ont pas (encore) d'outils — `model` seul prime
//! au spawn (passé tel quel au provider).

use std::collections::HashMap;
use std::path::Path;

use crate::app::App;

/// Agent custom : prompt système réutilisable, invoqué par son nom.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CustomAgent {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub model: Option<String>,
    pub tools: Vec<String>,
    pub excluded_tools: Vec<String>,
    pub mcp_servers: HashMap<String, crate::kiro::mcp::Server>,
}

/// Charge un dossier (`.json` + `.md` récursifs, triés par nom ;
/// `.json` prime sur `.md` à tige égale, illisibles ignorés).
pub fn load_dir(dir: &Path) -> Vec<CustomAgent> {
    let mut found: Vec<(String, String, std::path::PathBuf)> = Vec::new();
    collect(dir, dir, &mut found);
    found.sort();
    let mut done: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for (stem, ext, path) in found {
        if done.contains(&stem) {
            continue;
        }
        done.push(stem.clone());
        let raw = std::fs::read_to_string(&path).ok();
        let agent = raw.and_then(|raw| {
            if ext == "json" {
                crate::kiro::agents_parse::parse_json(&stem, &raw)
            } else {
                Some(crate::kiro::agents_parse::parse_md(&stem, &raw))
            }
        });
        if let Some(agent) = agent {
            out.push(agent);
        }
    }
    out
}

/// Collecte récursive `(tige, extension, chemin)` (`team/planner.md`
/// → tige `team/planner`, `/` même sous Windows ; trié par appelant).
fn collect(dir: &Path, root: &Path, out: &mut Vec<(String, String, std::path::PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|a| a.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, root, out);
        } else if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
            if ext == "json" || ext == "md" {
                if let Ok(rel) = path.strip_prefix(root) {
                    let stem = rel.with_extension("").to_string_lossy().replace('\\', "/");
                    out.push((stem, ext.to_string(), path));
                }
            }
        }
    }
}

/// Charge global puis projet (chemins injectés).
pub fn load_all_in(project: &Path, global: Option<&Path>) -> Vec<CustomAgent> {
    let mut out: Vec<CustomAgent> = Vec::new();
    if let Some(global) = global {
        out.extend(load_dir(&global.join("agents")));
    }
    for agent in load_dir(&project.join(".kiro/agents")) {
        if let Some(slot) = out.iter_mut().find(|each| each.name == agent.name) {
            *slot = agent;
        } else {
            out.push(agent);
        }
    }
    out
}

/// Retrouve un custom par son nom (match exact).
pub fn find<'a>(agents: &'a [CustomAgent], name: &str) -> Option<&'a CustomAgent> {
    agents.iter().find(|each| each.name == name)
}

/// Lignes `/agents` des customs (`custom <nom> — <description>`, vide si aucun).
pub fn lines_of(agents: &[CustomAgent]) -> Vec<String> {
    agents.iter().map(line_of).collect()
}

/// Customs de l'`App` (chemins résolus à la construction, voir `T5State`).
pub fn of_app(app: &App) -> Vec<CustomAgent> {
    load_all_in(&app.t5.kiro_project, app.t5.kiro_global.as_deref())
}

/// Lignes `/agents` des customs de l'`App` (appel 1-ligne pour `cmds.rs`).
pub fn lines_of_app(app: &App) -> Vec<String> {
    lines_of(&of_app(app))
}

/// Ligne d'un custom (description ou `(no description)`, en anglais ;
/// `model`/`tools`/`mcp` ajoutés s'ils sont déclarés).
fn line_of(agent: &CustomAgent) -> String {
    let mut line = if agent.description.trim().is_empty() {
        format!("custom {} — (no description)", agent.name)
    } else {
        format!("custom {} — {}", agent.name, agent.description)
    };
    let mut bits = Vec::new();
    if let Some(model) = agent
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
    {
        bits.push(format!("model {model}"));
    }
    if !agent.tools.is_empty() || !agent.excluded_tools.is_empty() {
        let mut names = agent.tools.clone();
        names.extend(agent.excluded_tools.iter().map(|tool| format!("-{tool}")));
        bits.push(format!("tools {}", names.join(",")));
    }
    if !agent.mcp_servers.is_empty() {
        let mut servers: Vec<&str> = agent.mcp_servers.keys().map(String::as_str).collect();
        servers.sort();
        bits.push(format!("mcp {}", servers.join(",")));
    }
    if !bits.is_empty() {
        line.push_str(&format!(" [{}]", bits.join("; ")));
    }
    line
}

/// Résout `/agent <nom?> <texte>` : agent connu + reste non vide.
fn resolve(agents: &[CustomAgent], prompt: &str) -> (Option<CustomAgent>, String) {
    let mut words = prompt.splitn(2, char::is_whitespace);
    let (first, rest) = (
        words.next().unwrap_or(""),
        words.next().map(str::trim).unwrap_or(""),
    );
    if rest.is_empty() {
        return (None, prompt.to_string());
    }
    match find(agents, first) {
        Some(agent) => (Some(agent.clone()), rest.to_string()),
        None => (None, prompt.to_string()),
    }
}

/// Spawn résolu : messages + prompt + modèle (`None` = modèle courant).
#[derive(Debug, Clone)]
pub struct Spawn {
    pub messages: Vec<(String, String)>,
    pub prompt: String,
    pub model: Option<String>,
}

/// Messages d'un spawn `/agent` : système custom (si le premier mot est un
/// nom connu) + agents.md pinné + prompt (nom dépouillé) ; modèle = celui
/// de l'agent si déclaré, sinon le modèle courant (résolu par l'appelant).
pub fn spawn_messages(app: &App, agents_md: Option<&str>, prompt: &str) -> Spawn {
    let (found, prompt) = resolve(&of_app(app), prompt);
    let mut messages = Vec::with_capacity(3);
    if let Some(agent) = &found {
        messages.push((String::from("system"), agent.system_prompt.clone()));
    }
    if let Some(content) = agents_md {
        messages.push((String::from("system"), content.to_string()));
    }
    messages.push((String::from("user"), prompt.clone()));
    Spawn {
        messages,
        prompt,
        model: found.and_then(|agent| agent.model),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("kh-agents-md-{}-{name}", std::process::id()))
    }

    #[test]
    fn parse_frontmatter_nom_description_system_prefixe() {
        let agent = crate::kiro::agents_parse::parse_md(
            "rev",
            "---\nname: reviewer\ndescription: Reviews code.\nsystem: Be terse.\n---\nCheck clarity.",
        );
        assert_eq!(agent.name, "reviewer");
        assert_eq!(agent.description, "Reviews code.");
        assert_eq!(agent.system_prompt, "Be terse.\n\nCheck clarity.");
        let agent = crate::kiro::agents_parse::parse_md("brut", "# Corps seul.");
        assert_eq!(agent.name, "brut");
        assert_eq!(agent.description, "");
        assert_eq!(agent.system_prompt, "# Corps seul.");
        // `system` seul (corps vide) vaut prompt entier.
        let agent = crate::kiro::agents_parse::parse_md("s", "---\nsystem: Just this.\n---\n");
        assert_eq!(agent.system_prompt, "Just this.");
    }

    #[test]
    fn load_nested_et_json_prime_sur_md() {
        let dir = tmp_dir("nested");
        std::fs::create_dir_all(dir.join(".kiro/agents/team")).unwrap();
        std::fs::write(dir.join(".kiro/agents/team/planner.md"), "Plan it.").unwrap();
        std::fs::write(dir.join(".kiro/agents/duel.md"), "md body").unwrap();
        std::fs::write(
            dir.join(".kiro/agents/duel.json"),
            r#"{"prompt": "json body"}"#,
        )
        .unwrap();
        let agents = load_all_in(&dir, None);
        assert_eq!(
            find(&agents, "team/planner").unwrap().system_prompt,
            "Plan it."
        );
        assert_eq!(find(&agents, "duel").unwrap().system_prompt, "json body");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_projet_ecrase_global_a_nom_egal() {
        let dir = tmp_dir("merge");
        let (global, project) = (dir.join("home/.kiro"), dir.join("proj"));
        std::fs::create_dir_all(global.join("agents")).unwrap();
        std::fs::create_dir_all(project.join(".kiro/agents")).unwrap();
        std::fs::write(global.join("agents/a.md"), "global a").unwrap();
        std::fs::write(global.join("agents/b.md"), "global b").unwrap();
        std::fs::write(project.join(".kiro/agents/b.md"), "project b").unwrap();
        let agents = load_all_in(&project, Some(&global));
        assert_eq!(agents.len(), 2);
        assert_eq!(find(&agents, "a").unwrap().system_prompt, "global a");
        assert_eq!(find(&agents, "b").unwrap().system_prompt, "project b");
        assert!(find(&agents, "z").is_none());
        assert!(load_all_in(&dir.join("vide"), None).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn agents_liste_customs_et_spawn_depouille_le_nom() {
        let dir = tmp_dir("spawn");
        std::fs::create_dir_all(dir.join(".kiro/agents")).unwrap();
        std::fs::write(
            dir.join(".kiro/agents/sage.md"),
            "---\nname: sage\ndescription: Wise counsel.\nsystem: You are wise.\n---\nCounsel briefly.",
        )
        .unwrap();
        let cfg = crate::provider::ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        let mut app = crate::app::App::from_config(cfg).unwrap();
        // Chemins injectés (pas d'env : hermétique au parallélisme).
        app.t5.kiro_project = dir.clone();
        app.t5.kiro_global = Some(dir.join("empty"));
        app.run_command("/agents");
        let listed = last_text(&app);
        assert!(listed.contains("custom sage — Wise counsel."), "{listed}");
        app.run_command("/agent sage hello world");
        let agents = &app.sessions.get(0).unwrap().agents;
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].prompt, "hello world"); // nom dépouillé
        assert_eq!(
            agents[0].history,
            vec![(String::from("user"), String::from("hello world"))]
        );
        // Nom inconnu : comportement inchangé (prompt intact).
        app.run_command("/agent who hello");
        let agents = &app.sessions.get(0).unwrap().agents;
        assert_eq!(agents[1].prompt, "who hello");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Dernière ligne affichée du chat courant.
    fn last_text(app: &crate::app::App) -> String {
        app.sessions
            .get(0)
            .unwrap()
            .chat
            .messages()
            .last()
            .map(|msg| msg.text.clone())
            .unwrap_or_default()
    }

    #[test]
    fn ligne_custom_avec_ou_sans_description() {
        let agent = CustomAgent {
            name: String::from("rev"),
            description: String::from("Reviews."),
            system_prompt: String::from("x"),
            ..Default::default()
        };
        assert_eq!(line_of(&agent), "custom rev — Reviews.");
        let agent = CustomAgent {
            description: String::new(),
            ..agent
        };
        assert_eq!(line_of(&agent), "custom rev — (no description)");
    }

    #[test]
    fn ligne_custom_detaille_model_tools_mcp() {
        let mut mcp = std::collections::HashMap::new();
        mcp.insert(String::from("pg"), crate::kiro::mcp::Server::default());
        let agent = CustomAgent {
            name: String::from("rev"),
            description: String::from("Reviews."),
            system_prompt: String::from("x"),
            model: Some(String::from("sonnet")),
            tools: vec![String::from("read")],
            excluded_tools: vec![String::from("web")],
            mcp_servers: mcp,
        };
        assert_eq!(
            line_of(&agent),
            "custom rev — Reviews. [model sonnet; tools read,-web; mcp pg]"
        );
    }

    #[test]
    fn spawn_rend_le_model_declare() {
        let dir = tmp_dir("spawn-model");
        std::fs::create_dir_all(dir.join(".kiro/agents")).unwrap();
        std::fs::write(
            dir.join(".kiro/agents/pro.md"),
            "---\nmodel: beast\n---\nDo it.",
        )
        .unwrap();
        let cfg = crate::provider::ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        let mut app = crate::app::App::from_config(cfg).unwrap();
        app.t5.kiro_project = dir.clone();
        app.t5.kiro_global = Some(dir.join("empty"));
        let spawn = spawn_messages(&app, None, "pro write code");
        assert_eq!(spawn.model.as_deref(), Some("beast"));
        assert_eq!(spawn.prompt, "write code");
        assert_eq!(spawn.messages.len(), 2);
        let spawn = spawn_messages(&app, None, "nobody here");
        assert!(spawn.model.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
