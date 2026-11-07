//! Clic souris : le bouton [+] de la sidebar crée un nouveau chat.
//!
//! La capture souris est activée par `main` (désactivée en sortie) ;
//! le rectangle du bouton est mémorisé à chaque frame (`App::plus_btn`).

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use crate::app::App;

impl App {
    /// Clic gauche sur [+] = nouveau chat (les autres clics sont ignorés).
    pub(crate) fn on_mouse(&mut self, mev: MouseEvent) {
        if mev.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }
        let at = Position::new(mev.column, mev.row);
        if self.plus_btn.is_some_and(|r| r.contains(at)) {
            self.add_chat();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderConfig;
    use crossterm::event::KeyModifiers;
    use ratatui::layout::Rect;

    /// App de test (clé factice, aucun appel réseau).
    fn test_app() -> App {
        App::from_config(ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        })
        .unwrap()
    }

    /// Clic gauche en `(col, row)`.
    fn click(col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: col,
            row: row,
            modifiers: KeyModifiers::empty(),
        }
    }

    #[test]
    fn clic_plus_cree_un_chat_clic_ailleurs_ignore() {
        let mut app = test_app();
        app.plus_btn = Some(Rect::new(1, 2, 16, 1));
        app.on_mouse(click(5, 2));
        assert_eq!(app.sessions.count(), 2);
        app.on_mouse(click(50, 20));
        assert_eq!(app.sessions.count(), 2);
        app.plus_btn = None;
        app.on_mouse(click(5, 2));
        assert_eq!(app.sessions.count(), 2);
    }

    #[test]
    fn seuls_les_clics_gauches_comptent() {
        let mut app = test_app();
        app.plus_btn = Some(Rect::new(1, 2, 16, 1));
        for kind in [
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Down(MouseButton::Middle),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Moved,
            MouseEventKind::ScrollUp,
        ] {
            app.on_mouse(MouseEvent {
                kind,
                column: 5,
                row: 2,
                modifiers: KeyModifiers::empty(),
            });
        }
        assert_eq!(app.sessions.count(), 1);
    }
}
