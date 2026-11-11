//! Écran Haunt (F9) : le jeu de `crate::haunt` dans un cadre, ses touches
//! et le protocole clavier Kitty (relâchements de touches, pour tenir
//! `a`/`d` enfoncées) activé le temps de jouer seulement.
//!
//! Touches : `a`/`d` (ou ←/→) gauche/droite, `w` (ou ↑) saut, `s` (ou ↓)
//! s'enfoncer, Espace = envoyer un token, Entrée = commencer, `p` pause,
//! Échap/F9 = retour au chat, F6 = boutique, F5 = éclair.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders},
    Frame,
};

use super::{App, Screen};
use crate::ghostty;
use crate::haunt::{self, Act};
use crate::theme::{self, ASH, BLOOD, BONE, CANDLE, PUMPKIN, UMBER};

/// Raccourcis du jeu rappelés dans la barre d'état.
pub(super) const HINTS: [(&str, &str); 3] = [
    ("F5", " thunder  "),
    ("Esc/F9", " back  "),
    ("F6", " shop  "),
];

/// Clavier du jeu : terminal réel branché (jamais d'écriture brute vers
/// stdout sans lui : tests, `--smoke`), protocole Kitty géré (interrogé
/// à la première entrée en jeu) et empilé (pendant le jeu).
#[derive(Debug, Default)]
pub(crate) struct Keyboard {
    pub(crate) live: bool,
    ok: Option<bool>,
    on: bool,
}

impl App {
    /// Attente maximale des événements : 1 ms en jeu (240 images/s),
    /// 10 ms ailleurs.
    pub(crate) fn poll_wait(&self) -> Duration {
        Duration::from_millis(if self.screen == Screen::Haunt {
            1
        } else if self.space_view.is_some() {
            4
        } else {
            10
        })
    }

    /// F9 : ouvre le jeu, ou revient au chat.
    pub(crate) fn open_haunt(&mut self) {
        let next = if self.screen == Screen::Haunt {
            Screen::Chat
        } else {
            Screen::Haunt
        };
        self.show(next);
    }

    /// Change d'écran : quitter le jeu le met en pause et rend le clavier
    /// normal ; y entrer active les relâchements de touches.
    pub(crate) fn show(&mut self, screen: Screen) {
        if self.screen == Screen::Haunt && screen != Screen::Haunt {
            self.haunt.pause();
            self.key_releases(false);
        }
        if screen == Screen::Haunt && self.screen != Screen::Haunt {
            self.key_releases(true);
        }
        self.screen = screen;
    }

    /// Touches du jeu (appuis, répétitions et relâchements).
    pub(crate) fn on_haunt_key(&mut self, key: KeyEvent) -> bool {
        let press = key.kind == KeyEventKind::Press;
        match key.code {
            KeyCode::Esc | KeyCode::F(9) if press => self.show(Screen::Chat),
            KeyCode::F(6) if press => self.show(Screen::Shop),
            KeyCode::F(5) if press => {
                self.spook();
                self.haunt.thunder();
            }
            code => {
                if let Some(act) = act_of(code) {
                    self.haunt.feed(act, key.kind);
                }
            }
        }
        true
    }

    /// Fait avancer le jeu du temps écoulé depuis `last` (remis à
    /// maintenant) s'il est à l'écran ; vrai = frame à redessiner.
    pub(crate) fn pump_haunt(&mut self, last: &mut Instant) -> bool {
        let dt = last.elapsed();
        *last = Instant::now();
        self.screen == Screen::Haunt && self.haunt.pump(dt)
    }

    /// Cadre + scène du jeu.
    pub(crate) fn draw_haunt(&mut self, frame: &mut Frame, area: Rect) {
        let key = Style::new().fg(PUMPKIN).add_modifier(Modifier::BOLD);
        let dim = Style::new().fg(ASH);
        let mut hints = vec![Span::raw(" ")];
        for (k, w) in [
            ("a d", " move  "),
            ("w", " jump  "),
            ("s", " sink  "),
            ("space", " tokens  "),
            ("p", " pause  "),
            ("Esc", " back "),
        ] {
            hints.push(Span::styled(k, key));
            hints.push(Span::styled(w, dim));
        }
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(theme::lerp(UMBER, BLOOD, 0.4)))
            .title(theme::title("haunt", BLOOD, CANDLE))
            .title(Line::styled(
                " level 1 · the boneyard ",
                Style::new().fg(BONE),
            ))
            .title_bottom(Line::from(hints));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let cell = self.bank.kitty_cell();
        haunt::draw::render(&mut self.haunt, inner, frame.buffer_mut(), cell);
        self.bank.protect(inner);
    }

    /// Relâchements de touches (protocole Kitty) le temps de jouer, si le
    /// terminal réel le gère (interrogé une fois) ; jamais hors terminal.
    fn key_releases(&mut self, on: bool) {
        if !self.keyboard.live || on == self.keyboard.on {
            return;
        }
        let supported = *self
            .keyboard
            .ok
            .get_or_insert_with(ghostty::supports_key_releases);
        if !supported {
            self.haunt.set_releases(false);
            return;
        }
        if ghostty::key_releases(on).is_ok() {
            self.keyboard.on = on;
            self.haunt.set_releases(on);
        }
    }

    /// En quittant l'application : clavier rendu tel qu'on l'a trouvé.
    pub(crate) fn release_keys(&mut self) {
        self.key_releases(false);
    }
}

/// Touche → action de jeu (`a`/`w`/`s`/`d` et flèches, Espace, Entrée, `p`).
fn act_of(code: KeyCode) -> Option<Act> {
    Some(match code {
        KeyCode::Char(c) => match c.to_ascii_lowercase() {
            'a' => Act::Left,
            'd' => Act::Right,
            'w' => Act::Up,
            's' => Act::Down,
            ' ' => Act::Fire,
            'p' => Act::Pause,
            _ => return None,
        },
        KeyCode::Left => Act::Left,
        KeyCode::Right => Act::Right,
        KeyCode::Up => Act::Up,
        KeyCode::Down => Act::Down,
        KeyCode::Enter => Act::Start,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderConfig;
    use crossterm::event::KeyModifiers;

    fn test_app() -> App {
        App::from_config(ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        })
        .unwrap()
    }

    #[test]
    fn wasd_and_arrows_map_to_moves() {
        assert_eq!(act_of(KeyCode::Char('a')), Some(Act::Left));
        assert_eq!(act_of(KeyCode::Char('D')), Some(Act::Right));
        assert_eq!(act_of(KeyCode::Char('w')), Some(Act::Up));
        assert_eq!(act_of(KeyCode::Char('s')), Some(Act::Down));
        assert_eq!(act_of(KeyCode::Char(' ')), Some(Act::Fire));
        assert_eq!(act_of(KeyCode::Up), Some(Act::Up));
        assert_eq!(act_of(KeyCode::Enter), Some(Act::Start));
        assert_eq!(act_of(KeyCode::Char('q')), None);
    }

    #[test]
    fn f9_opens_and_leaves_the_game_q_does_not_quit() {
        let mut app = test_app();
        let key = |c| KeyEvent::new(c, KeyModifiers::NONE);
        assert!(app.on_key(key(KeyCode::F(9))));
        assert_eq!(app.screen, Screen::Haunt);
        assert!(
            app.on_key(key(KeyCode::Char('q'))),
            "q ne quitte pas en jeu"
        );
        assert!(
            app.sessions.current().chat.input.is_empty(),
            "rien tapé au chat"
        );
        assert!(app.on_key(key(KeyCode::Esc)));
        assert_eq!(app.screen, Screen::Chat);
        assert!(app.on_key(key(KeyCode::F(9))));
        assert!(app.on_key(key(KeyCode::F(9))));
        assert_eq!(app.screen, Screen::Chat);
    }

    #[test]
    fn leaving_pauses_the_run() {
        let mut app = test_app();
        app.show(Screen::Haunt);
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        app.on_key(enter);
        let mut last = Instant::now() - Duration::from_millis(40);
        assert!(app.pump_haunt(&mut last));
        assert_eq!(app.haunt.phase, haunt::Phase::Play);
        app.show(Screen::Shop);
        assert_eq!(app.haunt.phase, haunt::Phase::Paused);
        assert!(
            !app.pump_haunt(&mut (Instant::now() - Duration::from_millis(40))),
            "figé hors écran"
        );
    }
}
