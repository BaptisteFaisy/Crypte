//! Agents customs (`.kiro/agents/*.md`, projet + global).
//!
//! Format (assumé nôtre — le schéma Kiro exact n'est pas garanti) :
//!
//! ```md
//! ---
//! name: reviewer
//! description: Reviews Rust code for clarity.
//! system: You are a terse reviewer.
//! ---
//! <corps = prompt système ; `system` préfixé au corps s'il est présent>
//! ```
//!
//! Sans frontmatter : nom = tige du fichier, pas de description, corps
//! seul. À nom égal, le projet écrase le global. `/agents` liste les
//! customs ; `/agent <nom> <texte>` spawne via le mécanisme `/agent`
//! existant avec le prompt custom en système (nom seul ou inconnu =
//! comportement inchangé).

use std::path::Path;

use crate::app::App;

/// Agent custom : prompt système réutilisable, invoqué par son nom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomAgent {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
}

/// Parse un fichier (`stem` = nom sans extension, défaut du `name`).
pub fn parse(stem: &str, raw: &str) -> CustomAgent {
    let (front, body) = crate::kiro::frontmatter::split(raw);
    let mut agent = CustomAgent {
        name: stem.to_string(),
        description: String::new(),
        system_prompt: body.to_string(),
    };
    if let Some(front) = front {
        if let Some(name) = crate::kiro::frontmatter::value(front, "name") {
            if !name.trim().is_empty() {
                agent.name = name;
            }
        }
        if let Some(desc) = crate::kiro::frontmatter::value(front, "description") {
            agent.description = desc;
        }
        if let Some(system) = crate::kiro::frontmatter::value(front, "system") {
            if !system.trim().is_empty() {
                agent.system_prompt = if body.trim().is_empty() {
                    system
                } else {
                    format!("{system}\n\n{body}")
                };
            }
        }
    }
    agent
}

/// Charge un dossier (`.md` triés, illisibles ignorés).
pub fn load_dir(dir: &Path) -> Vec<CustomAgent> {
    crate::kiro::frontmatter::load_md_files(dir, parse)
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

/// Ligne d'un custom (description ou `(no description)`, en anglais).
fn line_of(agent: &CustomAgent) -> String {
    if agent.description.trim().is_empty() {
        format!("custom {} — (no description)", agent.name)
    } else {
        format!("custom {} — {}", agent.name, agent.description)
    }
}

/// Scinde `/agent <nom?> <texte>` : premier mot connu + reste non vide →
/// (prompt custom, reste) ; sinon (aucun, prompt inchangé).
pub fn split_custom_in(prompt: &str, agents: &[CustomAgent]) -> (Option<String>, String) {
    let mut words = prompt.splitn(2, char::is_whitespace);
    let (first, rest) = (
        words.next().unwrap_or(""),
        words.next().map(str::trim).unwrap_or(""),
    );
    if rest.is_empty() {
        return (None, prompt.to_string());
    }
    match find(agents, first) {
        Some(agent) => (Some(agent.system_prompt.clone()), rest.to_string()),
        None => (None, prompt.to_string()),
    }
}

/// `split_custom_in` sur les customs de l'`App` (appel 1-ligne pour `cmds.rs`).
pub fn split_of_app(app: &App, prompt: &str) -> (Option<String>, String) {
    split_custom_in(prompt, &of_app(app))
}

/// Messages d'un spawn `/agent` : système custom (si le premier mot est un
/// nom connu) + agents.md pinné + prompt (nom dépouillé). Rend (messages
/// provider, prompt affiché).
pub fn spawn_messages(
    app: &App,
    agents_md: Option<&str>,
    prompt: &str,
) -> (Vec<(String, String)>, String) {
    let (custom, prompt) = split_of_app(app, prompt);
    let mut messages = Vec::with_capacity(3);
    if let Some(system) = custom {
        messages.push((String::from("system"), system));
    }
    if let Some(content) = agents_md {
        messages.push((String::from("system"), content.to_string()));
    }
    messages.push((String::from("user"), prompt.clone()));
    (messages, prompt)
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
        let agent = parse(
            "rev",
            "---\nname: reviewer\ndescription: Reviews code.\nsystem: Be terse.\n---\nCheck clarity.",
        );
        assert_eq!(agent.name, "reviewer");
        assert_eq!(agent.description, "Reviews code.");
        assert_eq!(agent.system_prompt, "Be terse.\n\nCheck clarity.");
        let agent = parse("brut", "# Corps seul.");
        assert_eq!(agent.name, "brut");
        assert_eq!(agent.description, "");
        assert_eq!(agent.system_prompt, "# Corps seul.");
        // `system` seul (corps vide) vaut prompt entier.
        let agent = parse("s", "---\nsystem: Just this.\n---\n");
        assert_eq!(agent.system_prompt, "Just this.");
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
        };
        assert_eq!(line_of(&agent), "custom rev — Reviews.");
        let agent = CustomAgent {
            description: String::new(),
            ..agent
        };
        assert_eq!(line_of(&agent), "custom rev — (no description)");
    }
}
