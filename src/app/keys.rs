//! Touches propres aux écrans plein cadre (bestiaire F6, almanach F8) ;
//! celles du hub vivent avec son écran (`app/esprits.rs`).

use crossterm::event::KeyCode;

use super::{App, Screen};

impl App {
    /// Touches de l'écran bestiaire : flèches, F6/F2/F7/F8 (sortie), Échap/q.
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
            KeyCode::F(8) => self.cmd(|s| s.show_almanac()),
            _ => true,
        }
    }

    /// F8 : almanach (relu à l'ouverture : d'autres cryptes ont pu
    /// écrire), ou retour au chat s'il est déjà affiché.
    pub(crate) fn show_almanac(&mut self) {
        self.screen = self.screen.toggle(Screen::Almanac);
        if self.screen == Screen::Almanac {
            self.almanac.refresh();
        }
    }
}
