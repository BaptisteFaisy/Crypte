//! Handler `/kiro` (extrait, limite 500 lignes — voir `cmds_web.rs`).
//!
//! `impl App` déporté : statut `.kiro/` (steering, specs, hooks, MCP).

use crate::app::App;

impl App {
    /// `/kiro` : ce que Crypte a chargé du `.kiro/` projet + `~/.kiro/`.
    pub(crate) fn cmd_kiro(&mut self) {
        let project = crate::kiro::project_dir();
        let home = crate::kiro::global_dir();
        self.push_system(crate::kiro::status::text(&project, home.as_deref()));
    }
}
