//! Handlers `/checkpoints` + `/rewind` (`impl App` déporté).
//!
//! `/checkpoints` liste les snapshots (plus récent d'abord, numérotés) ;
//! `/rewind <n|nom>` restaure (`n` = rang affiché, `nom` = basename exact
//! ou fragment du chemin d'origine).

use crate::app::App;

impl App {
    /// `/checkpoints` : snapshots numérotés (plus récent d'abord).
    pub(crate) fn cmd_checkpoints(&mut self) {
        let snaps = self
            .t5
            .checkpoint_dir
            .as_deref()
            .map(crate::kiro::checkpoint::list_in)
            .unwrap_or_default();
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
        match self.t5.checkpoint_dir.as_deref() {
            Some(ckpt) => match crate::kiro::checkpoint::rewind_in(ckpt, &target) {
                Ok(msg) | Err(msg) => self.push_system(msg),
            },
            None => self.push_system(String::from("no checkpoints")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoints_liste_rewind_restaure_et_erreurs() {
        // Dossier injecté via `T5State` (zéro env, hermétique).
        let dir = std::env::temp_dir().join(format!("kh-checkpoint-cmds-{}", std::process::id()));
        let ckpt = dir.join("ckpt");
        std::fs::create_dir_all(&ckpt).unwrap();
        let cfg = crate::provider::ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        let mut app = App::from_config(cfg).unwrap();
        app.t5.checkpoint_dir = Some(ckpt.clone());
        let target = dir.join("doc.md");
        crate::kiro::checkpoint::write_tracked_in(&ckpt, &target, "v1").unwrap();
        crate::kiro::checkpoint::write_tracked_in(&ckpt, &target, "v2").unwrap();
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
