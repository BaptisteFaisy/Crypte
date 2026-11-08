//! Handlers `/checkpoints` + `/rewind` (`impl App` déporté).
//!
//! `/checkpoints` liste les snapshots (plus récent d'abord, numérotés) ;
//! `/rewind <n|nom>` restaure (`n` = rang affiché, `nom` = basename exact
//! ou fragment du chemin d'origine).

use crate::app::App;

impl App {
    /// `/checkpoints` : snapshots numérotés (plus récent d'abord).
    pub(crate) fn cmd_checkpoints(&mut self) {
        let snaps = crate::kiro::checkpoint::list();
        if snaps.is_empty() {
            self.push_system(String::from("no checkpoints"));
            return;
        }
        let mut lines = vec![String::from("checkpoints:")];
        for (i, snap) in snaps.iter().enumerate() {
            let file = snap
                .file
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default();
            lines.push(format!("  {}. {} ({file})", i + 1, snap.original.display()));
        }
        self.push_system(lines.join("\n"));
    }

    /// `/rewind <n|nom>` : restaure le snapshot (succès ou erreur affichés).
    pub(crate) fn cmd_rewind(&mut self, target: String) {
        match crate::kiro::checkpoint::rewind(&target) {
            Ok(msg) | Err(msg) => self.push_system(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoints_liste_rewind_restaure_et_erreurs() {
        // Seul test à toucher `KH_CHECKPOINT_DIR` (pas de course).
        let dir = std::env::temp_dir().join(format!("kh-checkpoint-cmds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let saved = std::env::var("KH_CHECKPOINT_DIR").ok();
        std::env::set_var("KH_CHECKPOINT_DIR", dir.join("ckpt"));
        let cfg = crate::provider::ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        let mut app = App::from_config(cfg).unwrap();
        let target = dir.join("doc.md");
        crate::kiro::checkpoint::write_tracked(&target, "v1").unwrap();
        crate::kiro::checkpoint::write_tracked(&target, "v2").unwrap();
        app.run_command("/checkpoints");
        let listed = last_text(&app);
        assert!(
            listed.contains("1. ") && listed.contains("doc.md"),
            "{listed}"
        );
        app.run_command("/rewind 1");
        let done = last_text(&app);
        assert!(done.starts_with("restored "), "{done}");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v1");
        app.run_command("/rewind zzz");
        let missing = last_text(&app);
        assert!(
            missing.contains("no checkpoint matching 'zzz'"),
            "{missing}"
        );
        match saved {
            Some(val) => std::env::set_var("KH_CHECKPOINT_DIR", val),
            None => std::env::remove_var("KH_CHECKPOINT_DIR"),
        }
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
