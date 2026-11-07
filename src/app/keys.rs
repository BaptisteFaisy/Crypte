//! Clavier : dispatch général (`on_key`), défilement, colonne ;
//! celles du hub vivent avec son écran (`app/esprits.rs`).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, Screen};
use crate::chat::SCROLL_PAGE;

impl App {
    /// Traite une touche ; renvoie false pour quitter.
    /// Confirmation outil ou flow `/connect` : touches dédiées (pas de quit).
    pub(super) fn on_key(&mut self, key: KeyEvent) -> bool {
        if self.tool_run.as_ref().is_some_and(|run| run.confirming) {
            return self.on_tool_key(key);
        }
        if self.connect_flow.is_some() {
            return self.on_flow_key(key);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let empty = self.sessions.current().chat.input.is_empty();
        match self.screen {
            Screen::Hub => return self.on_hub_key(key.code),
            _ => {}
        }
        match key.code {
            KeyCode::Char('q') if !ctrl && empty => false,
            KeyCode::Char('n' | 'N') if ctrl => self.cmd(|s| s.add_chat()),
            KeyCode::Char('w' | 'W') if ctrl => self.cmd(|s| s.close_chat(s.sessions.index())),
            KeyCode::Char(c) if !ctrl => self.cmd(|s| s.sessions.current_mut().chat.input_push(c)),
            KeyCode::Backspace => self.cmd(|s| s.sessions.current_mut().chat.input_pop()),
            KeyCode::Enter => self.cmd(|s| s.send()),
            KeyCode::PageUp => self.cmd(|s| s.scroll_chat(-SCROLL_PAGE)),
            KeyCode::PageDown => self.cmd(|s| s.scroll_chat(SCROLL_PAGE)),
            KeyCode::Up if shift => self.cmd(|s| s.scroll_chat(-1)),
            KeyCode::Down if shift => self.cmd(|s| s.scroll_chat(1)),
            KeyCode::End => self.cmd(|s| s.sessions.current().chat.scroll.to_bottom()),
            KeyCode::F(5) => self.cmd(|s| s.spook()),
            KeyCode::F(7) => self.cmd(|s| s.open_hub()),
            KeyCode::Left if ctrl => self.cmd(|s| s.resize_right(2)),
            KeyCode::Right if ctrl => self.cmd(|s| s.resize_right(-2)),
            _ => true,
        }
    }

    /// Défile l'historique du chat courant (`delta` < 0 = remonter).
    pub(crate) fn scroll_chat(&mut self, delta: isize) {
        self.sessions.current().chat.scroll.by(delta);
    }

    /// Ctrl+←/→ : élargit (+2) / rétrécit (−2) la colonne de droite.
    pub(crate) fn resize_right(&mut self, delta: i16) {
        self.right_delta = (self.right_delta + delta).clamp(-64, 64);
    }
}
