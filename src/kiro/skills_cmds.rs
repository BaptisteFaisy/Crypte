//! Handlers `/skills` + `/skill` (`impl App` déporté).
//!
//! `/skills` liste (nom + description) ; `/skill <nom> [texte]` préfixe
//! le corps du skill à la saisie courante — en tête du prochain envoi,
//! transitoire (la saisie est vidée à l'envoi), annoncé.

use crate::app::App;

impl App {
    /// `/skills` : nom + description de chaque skill (projet + global).
    pub(crate) fn cmd_skills(&mut self) {
        let skills =
            crate::kiro::skills::load_all_in(&self.t5.kiro_project, self.t5.kiro_global.as_deref());
        let lines = crate::kiro::skills::lines_of(&skills);
        if lines.is_empty() {
            self.push_system(String::from("no skills — add .kiro/skills/*/SKILL.md"));
        } else {
            self.push_system(format!("skills:\n{}", lines.join("\n")));
        }
    }

    /// `/skill <nom> [texte]` : arme le skill (corps préfixé à la saisie,
    /// `[texte]` + brouillon courant à la suite) ; inconnu → erreur + aide.
    pub(crate) fn cmd_skill(&mut self, name: String, text: Option<String>) {
        let skills =
            crate::kiro::skills::load_all_in(&self.t5.kiro_project, self.t5.kiro_global.as_deref());
        let Some(skill) = crate::kiro::skills::find(&skills, &name) else {
            self.push_system(format!("unknown skill: {name} — /skills to list"));
            return;
        };
        let mut parts = vec![skill.body.clone()];
        if let Some(text) = text.filter(|t| !t.trim().is_empty()) {
            parts.push(text);
        }
        let draft = std::mem::take(&mut self.sessions.current_mut().chat.input);
        if !draft.trim().is_empty() {
            parts.push(draft);
        }
        let chat = &mut self.sessions.current_mut().chat;
        chat.input = parts.join("\n\n");
        chat.cursor = chat.input.chars().count();
        self.push_system(format!("skill '{name}' armed — send to use (transient)"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_liste_skill_arme_et_inconnu_signale() {
        let dir = std::env::temp_dir().join(format!("kh-skill-cmds-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kiro/skills/terse")).unwrap();
        std::fs::write(
            dir.join(".kiro/skills/terse/SKILL.md"),
            "---\nname: terse\ndescription: Short replies.\n---\nBe brief.",
        )
        .unwrap();
        let cfg = crate::provider::ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        let mut app = App::from_config(cfg).unwrap();
        // Chemins injectés (pas d'env : hermétique au parallélisme).
        app.t5.kiro_project = dir.clone();
        app.t5.kiro_global = Some(dir.join("empty"));
        app.run_command("/skills");
        let listed = last_text(&app);
        assert!(listed.contains("terse — Short replies."), "{listed}");
        app.sessions.current_mut().chat.input_insert("draft");
        app.run_command("/skill terse hello");
        assert_eq!(
            app.sessions.current().chat.input,
            "Be brief.\n\nhello\n\ndraft"
        );
        let armed = last_text(&app);
        assert!(armed.contains("skill 'terse' armed"), "{armed}");
        app.run_command("/skill zzz");
        let unknown = last_text(&app);
        assert!(unknown.contains("unknown skill: zzz"), "{unknown}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Dernière ligne affichée du chat courant.
    fn last_text(app: &App) -> String {
        app.sessions
            .get(0)
            .unwrap()
            .chat
            .messages()
            .last()
            .map(|msg| msg.text.clone())
            .unwrap_or_default()
    }
}
