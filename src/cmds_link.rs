//! Handlers `/link` et `/unlink` (extraits de `cmds.rs`, limite 500 lignes).
//!
//! `impl App` déporté : lit/écrit l'état via les accès `pub(crate)` de [`App`].

use crate::app::App;
use crate::bus::{LinkEnd, LinkError};

impl App {
    /// `/link <a> <b>` : chaque réponse de `a` est retransmise à `b`
    /// (déclenche sa réponse ; 8 relais max, voir `/links`). Extrémités
    /// validées (chat existant, agent connu).
    pub(crate) fn cmd_link(&mut self, from: LinkEnd, to: LinkEnd) {
        if let Err(msg) = self.check_link_end(from) {
            self.push_system(msg);
            return;
        }
        if let Err(msg) = self.check_link_end(to) {
            self.push_system(msg);
            return;
        }
        match self.bus.link(from, to) {
            Ok(()) => self.push_system(format!("linked: {} → {}", from.label(), to.label())),
            Err(LinkError::Same) => {
                self.push_system(String::from("a link to itself is useless"));
            }
            Err(LinkError::Exists) => self.push_system(String::from("link already present")),
        }
    }

    /// `/unlink <a> <b>` : retire le lien (note s'il n'existait pas).
    pub(crate) fn cmd_unlink(&mut self, from: LinkEnd, to: LinkEnd) {
        if self.bus.unlink(from, to) {
            self.push_system(format!("unlinked: {} → {}", from.label(), to.label()));
        } else {
            self.push_system(String::from("no link in that direction"));
        }
    }

    /// Valide une extrémité de lien (chat existant, agent connu).
    fn check_link_end(&self, end: LinkEnd) -> Result<(), String> {
        match end {
            LinkEnd::Chat(idx) if self.sessions.get(idx).is_some() => Ok(()),
            LinkEnd::Chat(idx) => Err(format!("chat {} not found", idx + 1)),
            LinkEnd::Agent(id) if self.sessions.has_agent(id) => Ok(()),
            LinkEnd::Agent(id) => Err(format!("agent#{id} unknown")),
        }
    }
}
