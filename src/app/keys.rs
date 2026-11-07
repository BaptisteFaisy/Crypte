//! Clavier : dispatch général (`on_key`, chat + almanach) et touches
//! propres aux écrans plein cadre (bestiaire F6, almanach F8) ;
//! celles du hub vivent avec son écran (`app/esprits.rs`).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, Screen};

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
        let empty = self.sessions.current().chat.input.is_empty();
        match self.screen {
            Screen::Bestiary => return self.on_bestiary_key(key.code),
            Screen::Hub => return self.on_hub_key(key.code),
            _ => {}
        }
        match key.code {
            KeyCode::Char('q') if !ctrl && empty => false,
            KeyCode::Char('n' | 'N') if ctrl => self.cmd(|s| s.add_chat()),
            KeyCode::Char(c) if !ctrl => self.cmd(|s| s.sessions.current_mut().chat.input_push(c)),
            KeyCode::Backspace => self.cmd(|s| s.sessions.current_mut().chat.input_pop()),
            KeyCode::Enter => self.cmd(|s| s.send()),
            KeyCode::F(5) => self.cmd(|s| s.spook()),
            KeyCode::F(6) => self.cmd(|s| s.screen = s.screen.toggle(Screen::Bestiary)),
            KeyCode::F(7) => self.cmd(|s| s.open_hub()),
            KeyCode::F(8) => self.cmd(|s| s.show_almanac()),
            KeyCode::Left if ctrl => self.cmd(|s| s.resize_right(2)),
            KeyCode::Right if ctrl => self.cmd(|s| s.resize_right(-2)),
            _ => true,
        }
    }

    /// Touches de l'écran bestiaire : flèches, F6/F7/F8 (sortie), q.
    pub(super) fn on_bestiary_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char('q') => false,
            KeyCode::Left => self.cmd(|s| s.galerie.step(-1, 0)),
            KeyCode::Right => self.cmd(|s| s.galerie.step(1, 0)),
            KeyCode::Up => self.cmd(|s| s.galerie.step(0, -1)),
            KeyCode::Down => self.cmd(|s| s.galerie.step(0, 1)),
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

    /// Ctrl+←/→ : élargit (+2) / rétrécit (−2) la colonne de droite.
    pub(crate) fn resize_right(&mut self, delta: i16) {
        self.right_delta = (self.right_delta + delta).clamp(-64, 64);
    }
}
