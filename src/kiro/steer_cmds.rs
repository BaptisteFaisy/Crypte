//! Handler `/steer` (`impl App` déporté).
//!
//! `/steer` liste les fichiers `manual` + `auto` ;
//! `/steer <nom>` arme un fichier `manual` (corps préfixé à la
//! saisie, transitoire comme `/skill`, annoncé). Les `always` /
//! `fileMatch` / `auto` s'appliquent seuls (pin, contexte, demande).

use crate::app::App;
use crate::kiro::steering::Inclusion;

impl App {
    /// `/steer [nom]` : liste `manual`/`auto`, ou arme un `manual`.
    pub(crate) fn cmd_steer(&mut self, name: Option<String>) {
        let files = crate::kiro::steering::load_all_in(
            &self.t5.kiro_project,
            self.t5.kiro_global.as_deref(),
        );
        let Some(name) = name else {
            self.push_system(list_text(&files));
            return;
        };
        let Some(file) = crate::kiro::steering::find_manual(&files, &name) else {
            self.push_system(unknown_text(&files, &name));
            return;
        };
        let draft = std::mem::take(&mut self.sessions.current_mut().chat.input);
        let armed = if draft.trim().is_empty() {
            file.body.clone()
        } else {
            format!("{}\n\n{draft}", file.body)
        };
        self.sessions.current_mut().chat.input = armed;
        self.push_system(format!("steering '{name}' armed — send to use (transient)"));
    }
}

/// Liste `/steer` : `manual` + `auto` (nom — inclusion — description).
fn list_text(files: &[crate::kiro::steering::Steering]) -> String {
    let mut lines = Vec::new();
    for file in files {
        match file.inclusion {
            Inclusion::Manual | Inclusion::Auto => {
                let desc = if file.description.trim().is_empty() {
                    String::from("(no description)")
                } else {
                    file.description.clone()
                };
                lines.push(format!(
                    "{} — {} — {desc}",
                    file.name,
                    file.inclusion.as_str()
                ));
            }
            Inclusion::Always | Inclusion::FileMatch => {}
        }
    }
    if lines.is_empty() {
        return String::from("no manual/auto steering — add .kiro/steering/*.md");
    }
    lines.sort();
    format!("steerable:\n{}", lines.join("\n"))
}

/// Erreur `/steer <nom>` : inconnu, ou déjà automatique.
fn unknown_text(files: &[crate::kiro::steering::Steering], name: &str) -> String {
    if files.iter().any(|file| file.name == name) {
        return format!("'{name}' is not manual (always/fileMatch/auto apply on their own)");
    }
    format!("unknown steering: {name} — /steer to list")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// App + dossier `.kiro/steering` (manuel `m`, auto `a`, unique/test).
    fn fixture(tag: &str) -> (App, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("kh-steer-cmds-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".kiro/steering")).unwrap();
        std::fs::write(
            dir.join(".kiro/steering/m.md"),
            "---\ninclusion: manual\ndescription: Aide ponctuelle.\n---\nCorps manuel.",
        )
        .unwrap();
        std::fs::write(
            dir.join(".kiro/steering/a.md"),
            "---\ninclusion: auto\ndescription: Règles Rust.\n---\nCorps auto.",
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

    /// `/steer` liste manuel + auto (pas les `always`).
    #[test]
    fn steer_nu_liste_manuel_et_auto() {
        let (mut app, dir) = fixture("liste");
        app.run_command("/steer");
        let text = last_text(&app);
        assert!(text.contains("m — manual — Aide ponctuelle."), "{text}");
        assert!(text.contains("a — auto — Règles Rust."), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `/steer m` arme le corps dans la saisie ; inconnu signalé.
    #[test]
    fn steer_nom_arme_et_inconnu_signale() {
        let (mut app, dir) = fixture("arme");
        app.run_command("/steer m");
        assert!(last_text(&app).contains("armed"), "{}", last_text(&app));
        let input = app.sessions.get(0).unwrap().chat.input.clone();
        assert!(input.starts_with("Corps manuel."), "{input}");
        app.run_command("/steer x");
        assert!(
            last_text(&app).contains("unknown steering: x"),
            "{}",
            last_text(&app)
        );
        app.run_command("/steer a");
        assert!(
            last_text(&app).contains("not manual"),
            "{}",
            last_text(&app)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
