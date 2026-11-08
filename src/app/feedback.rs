//! Retours visibles : messages système dans le chat courant + barre
//! de progression Ghostty (déportés hors `app.rs`, budget 500 lignes).

use anyhow::Result;

use super::App;
use crate::chat::Role;
use crate::ghostty;

impl App {
    /// Barre de progression Ghostty : suit l'attente du chat courant.
    pub(super) fn sync_progress(&mut self) -> Result<()> {
        let busy = self.sessions.count() > 0 && self.sessions.current().sending;
        if busy != self.busy {
            self.busy = busy;
            ghostty::progress(busy)?;
        }
        Ok(())
    }

    /// Ajoute un message système au chat courant (perdu si aucun chat).
    pub(crate) fn push_system(&mut self, msg: String) {
        let idx = self.sessions.index();
        let Some(sess) = self.sessions.get_mut(idx) else {
            return; // dernier chat fermé : rien où afficher
        };
        sess.chat.push(Role::System, msg);
    }
}
