//! Handler `/remember` (`impl App` déporté).
//!
//! `/remember <texte>` ajoute une ligne à la mémoire projet
//! (`.kiro/MEMORY.md`, créé si absent) ; la mémoire globale ne s'édite
//! qu'à la main. Prise en compte au prochain pin (redémarrage).

use crate::app::App;

impl App {
    /// `/remember <texte>` : mémorise dans le projet (succès/erreur affichés).
    pub(crate) fn cmd_remember(&mut self, text: String) {
        match crate::kiro::memory::remember(&self.t5.kiro_project, &text) {
            Ok(()) => self.push_system(String::from("remembered to .kiro/MEMORY.md")),
            Err(err) => self.push_system(format!("cannot remember: {err}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_ecrit_la_memoire_projet() {
        let dir = std::env::temp_dir().join(format!("kh-remember-cmds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = crate::provider::ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        let mut app = App::from_config(cfg).unwrap();
        // Chemin injecté (pas d'env : hermétique au parallélisme).
        // Un seul remember sur mémoire absente : aucun snapshot réel écrit.
        app.t5.kiro_project = dir.clone();
        app.run_command("/remember the user likes terse replies");
        let announced = last_text(&app);
        assert_eq!(announced, "remembered to .kiro/MEMORY.md");
        assert_eq!(
            std::fs::read_to_string(dir.join(".kiro/MEMORY.md")).unwrap(),
            "the user likes terse replies\n"
        );
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
