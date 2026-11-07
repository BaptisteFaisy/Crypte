//! Touches propres aux écrans plein cadre (bestiaire F6) ; celles du hub
//! vivent avec son écran (`app/esprits.rs`).

use crossterm::event::KeyCode;

use super::{App, Screen};

impl App {
    /// Touches de l'écran bestiaire : flèches, F6/F2 (sortie), Échap/q.
    pub(super) fn on_bestiary_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => false,
            KeyCode::Left => self.cmd(|s| s.galerie.step(-1, 0)),
            KeyCode::Right => self.cmd(|s| s.galerie.step(1, 0)),
            KeyCode::Up => self.cmd(|s| s.galerie.step(0, -1)),
            KeyCode::Down => self.cmd(|s| s.galerie.step(0, 1)),
            KeyCode::F(2) => self.cmd(|s| s.screen = Screen::Tokens),
            KeyCode::F(5) => self.cmd(|s| s.spook()),
            KeyCode::F(6) => self.cmd(|s| s.screen = Screen::Chat),
            KeyCode::F(7) => self.cmd(|s| s.open_hub()),
            _ => true,
        }
    }
}
