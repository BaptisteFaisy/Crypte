//! Handler `/powers` (`impl App` déporté).
//!
//! `/powers` liste les plugins installés (nom — description —
//! mots-clés — skills) ; l'activation est automatique à l'envoi
//! (mots-clés dans la demande, skills injectés, annoncé).

use crate::app::App;

impl App {
    /// `/powers` : plugins `~/.kiro/powers/installed/` (global seul).
    pub(crate) fn cmd_powers(&mut self) {
        let powers = crate::kiro::powers::load_all(self.t5.kiro_global.as_deref());
        if powers.is_empty() {
            self.push_system(String::from("no powers — install with Kiro IDE/CLI"));
            return;
        }
        let mut lines = Vec::new();
        for power in &powers {
            let head = if power.version.trim().is_empty() {
                power.name.clone()
            } else {
                format!("{} {}", power.name, power.version)
            };
            let mut bits = vec![head];
            bits.push(if power.description.trim().is_empty() {
                String::from("(no description)")
            } else {
                power.description.clone()
            });
            if !power.keywords.is_empty() {
                bits.push(format!("keywords: {}", power.keywords.join(", ")));
            }
            bits.push(format!("{} skill(s)", power.skills.len()));
            if power.has_mcp {
                bits.push(String::from("mcp: phase 2"));
            }
            if power.legacy {
                bits.push(String::from("legacy POWER.md"));
            }
            lines.push(bits.join(" — "));
        }
        self.push_system(format!("powers:\n{}", lines.join("\n")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// App + un power moderne installé (global jouet).
    fn fixture() -> (App, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("kh-powers-cmds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let power = dir.join("powers/installed/db");
        std::fs::create_dir_all(power.join("skills/indexes")).unwrap();
        std::fs::write(
            power.join("plugin.json"),
            r#"{"name": "db", "description": "Bases.", "keywords": ["postgres"]}"#,
        )
        .unwrap();
        std::fs::write(power.join("skills/indexes/SKILL.md"), "Corps.").unwrap();
        let cfg = crate::provider::ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        let mut app = App::from_config(cfg).unwrap();
        app.t5.kiro_global = Some(dir.clone());
        (app, dir)
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

    /// `/powers` liste le plugin (nom, mots-clés, skills).
    #[test]
    fn powers_liste_plugin() {
        let (mut app, dir) = fixture();
        app.run_command("/powers");
        let text = last_text(&app);
        assert!(text.contains("db — Bases."), "{text}");
        assert!(text.contains("keywords: postgres"), "{text}");
        assert!(text.contains("1 skill(s)"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Sans powers : piste d'installation.
    #[test]
    fn powers_vide_piste() {
        let cfg = crate::provider::ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        let mut app = App::from_config(cfg).unwrap();
        app.t5.kiro_global = None;
        app.run_command("/powers");
        assert!(last_text(&app).contains("no powers"), "{}", last_text(&app));
    }
}
