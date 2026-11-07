//! Prompts de génération des specs (purs, testés).
//!
//! `/spec new` enchaîne 3 appels LLM (requirements → design → tasks),
//! chaque étape nourrie des précédentes ; `/spec run` exécute chaque
//! tâche avec requirements + design en contexte (voir `task_user`).

/// Système : rédige un `requirements.md` (comportements testables,
/// sans implémentation, sans liste de tâches).
pub fn requirements_system() -> &'static str {
    "You write a requirements document for a software feature. Reply with \
     Markdown only: a short intro, then numbered requirements grouped by \
     area, each testable (observable behavior, no implementation). No code, \
     no task list."
}

/// Système : rédige un `design.md` (archi, décisions, cas limites).
pub fn design_system() -> &'static str {
    "You write a technical design document from approved requirements. \
     Reply with Markdown only: architecture overview, components and data \
     flow, key decisions with rationale, edge cases. No code unless a tiny \
     sketch clarifies, no task list."
}

/// Système : rédige un `tasks.md` (cases à cocher, ordre de dépendances).
pub fn tasks_system() -> &'static str {
    "You write an implementation task list from approved requirements and \
     design. Reply with Markdown only: a checklist of small tasks (`- [ ]` \
     plus one imperative sentence each, in dependency order), each doable \
     in one focused step. No code, no explanations outside the list."
}

/// Utilisateur : requirements à partir de l'idée.
pub fn requirements_user(idea: &str) -> String {
    format!("Idea:\n{idea}\n\nWrite requirements.md for this idea.")
}

/// Utilisateur : design nourri de l'idée + requirements.
pub fn design_user(idea: &str, requirements: &str) -> String {
    format!(
        "Idea:\n{idea}\n\nRequirements:\n{requirements}\n\nWrite design.md for these requirements."
    )
}

/// Utilisateur : tâches nourries de l'idée + requirements + design.
pub fn tasks_user(idea: &str, requirements: &str, design: &str) -> String {
    format!(
        "Idea:\n{idea}\n\nRequirements:\n{requirements}\n\nDesign:\n{design}\n\nWrite tasks.md for this spec."
    )
}

/// Utilisateur : exécute UNE tâche (requirements + design en contexte,
/// absents = section omise) ; ne fait que ça.
pub fn task_user(requirements: Option<&str>, design: Option<&str>, task: &str) -> String {
    let mut prompt = String::from(
        "You implement one task of an approved spec. Do the task, nothing \
         else; reply with a short summary of what changed.\n",
    );
    if let Some(req) = requirements.filter(|s| !s.trim().is_empty()) {
        prompt.push_str(&format!("\nRequirements:\n{req}\n"));
    }
    if let Some(des) = design.filter(|s| !s.trim().is_empty()) {
        prompt.push_str(&format!("\nDesign:\n{des}\n"));
    }
    prompt.push_str(&format!("\nTask:\n{task}\n"));
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemes_distincts_et_cibles() {
        let (req, des, tas) = (requirements_system(), design_system(), tasks_system());
        assert_ne!(req, des);
        assert_ne!(des, tas);
        assert!(req.contains("requirements") && req.contains("Markdown only"));
        assert!(des.contains("design") && des.contains("Markdown only"));
        assert!(tas.contains("`- [ ]`") && tas.contains("Markdown only"));
    }

    #[test]
    fn etapes_enchainees_nourries_des_precedentes() {
        let user = requirements_user("ghost chat");
        assert!(user.contains("ghost chat") && user.contains("requirements.md"));
        let user = design_user("ghost chat", "R1: scare");
        assert!(user.contains("ghost chat") && user.contains("R1: scare"));
        assert!(user.contains("design.md"));
        let user = tasks_user("ghost chat", "R1: scare", "D1: fog");
        assert!(user.contains("R1: scare") && user.contains("D1: fog"));
        assert!(user.contains("tasks.md"));
    }

    #[test]
    fn tache_contexte_optionnel() {
        let full = task_user(Some("R1"), Some("D1"), "code it");
        assert!(full.contains("R1") && full.contains("D1") && full.contains("code it"));
        let bare = task_user(None, Some("  "), "code it");
        assert!(bare.contains("code it"));
        assert!(!bare.contains("Requirements:") && !bare.contains("Design:"));
    }
}
