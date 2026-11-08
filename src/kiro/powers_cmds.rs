//! Handler `/powers` (`impl App` déporté).
//!
//! `/powers` liste les plugins installés (nom — description —
//! mots-clés — skills — MCP) ; l'activation est automatique à l'envoi
//! (mots-clés dans la demande, skills injectés + serveurs MCP ajoutés
//! au parc, annoncé).

use crate::app::App;
use crate::kiro::mcp_pool::PowerAdd;
use crate::kiro::powers::Power;

impl App {
    /// Active le MCP des powers matchés (ajout parc, annoncé une fois ;
    /// silence si déjà actif ou verrou pris — réessayé au prochain envoi).
    pub(crate) fn activate_power_mcp(&mut self, matched: &[Power]) {
        for power in matched {
            if power.mcp_servers.is_empty() {
                continue;
            }
            let PowerAdd::Added(added) = self.mcp.try_add_power(power) else {
                continue;
            };
            let mut line = format!("power {}: mcp {}", power.name, added.fresh.join(", "));
            if added.fresh.is_empty() {
                line = format!("power {}: mcp already configured", power.name);
            }
            if !added.dupes.is_empty() {
                line.push_str(&format!(
                    " (already configured: {})",
                    added.dupes.join(", ")
                ));
            }
            self.push_system(line);
        }
    }

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
            if !power.mcp_servers.is_empty() {
                let names: Vec<&str> = power
                    .mcp_servers
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect();
                bits.push(format!("mcp: {}", names.join(", ")));
            } else if power.has_mcp {
                bits.push(String::from("mcp: none"));
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

    /// App + un power moderne installé (global jouet, unique/test).
    fn fixture(tag: &str) -> (App, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("kh-powers-cmds-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let power = dir.join("powers/installed/db");
        std::fs::create_dir_all(power.join("skills/indexes")).unwrap();
        std::fs::write(
            power.join("plugin.json"),
            r#"{"name": "db", "description": "Bases.", "keywords": ["postgres"]}"#,
        )
        .unwrap();
        std::fs::write(power.join("skills/indexes/SKILL.md"), "Corps.").unwrap();
        std::fs::write(
            power.join("mcp.json"),
            r#"{"mcpServers": {"pg": {"command": "kh-nope-binaire"}}}"#,
        )
        .unwrap();
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
        let (mut app, dir) = fixture("liste");
        app.run_command("/powers");
        let text = last_text(&app);
        assert!(text.contains("db — Bases."), "{text}");
        assert!(text.contains("keywords: postgres"), "{text}");
        assert!(text.contains("1 skill(s)"), "{text}");
        assert!(text.contains("mcp: pg"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Activation MCP : annonce une fois, origine au statut, silence ensuite.
    #[test]
    fn activation_mcp_annonce_une_fois_origine() {
        let (mut app, dir) = fixture("active");
        app.mcp = crate::kiro::mcp_pool::McpState::for_config(crate::kiro::mcp::Config::default());
        let powers = crate::kiro::powers::load_all(app.t5.kiro_global.as_deref());
        app.activate_power_mcp(&powers);
        assert!(
            last_text(&app).contains("power db: mcp pg"),
            "{}",
            last_text(&app)
        );
        assert!(app
            .mcp
            .status()
            .contains("pg (power db, stdio, not connected)"));
        let before = last_text(&app);
        app.activate_power_mcp(&powers);
        assert_eq!(last_text(&app), before);
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
