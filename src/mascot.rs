//! Mascotte Kiro fantôme : figée dans sa zone, s'effraie sur demande.
//!
//! Contrat : `Mascot::new` → `draw` (mémorise la zone d'affichage) → `tick`
//! (gère le retour de `Spook`) → `spook` (`Spook` pendant 2 s, via F5).

use std::time::{Duration, Instant};

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::Paragraph,
    Frame,
};

/// Durée de l'état `Spook` après [`Mascot::spook`].
const SPOOK_DURATION: Duration = Duration::from_secs(2);
/// Encombrement max des frames (le spook fait 12x8).
const W: u16 = 12;
const H: u16 = 8;

/// Haut du fantôme, commun aux deux frames de marche (zéro duplication).
const HEAD: [&str; 6] = [
    "    ____    ",
    "   /    \\   ",
    "  | o  o |  ",
    "  |  __  |  ",
    "  | |  | |  ",
    "  | |  | |  ",
];
/// Frange basse du fantôme immobile.
const FRINGE: &str = "   \\/\\/\\/   ";
/// Frame effrayée : yeux creux, bouche zigzag, bras levés, ectoplasme.
const SPOOK: [&str; 8] = [
    " \\  __  //  ",
    "  \\/O  O\\/  ",
    "  | O  O |  ",
    "  |  __  |  ",
    "  | /\\/\\ |  ",
    "  | |  | |  ",
    "  |/ .  .|  ",
    "  ~  .  ~   ",
];

const ECTO_FG: Color = Color::Rgb(0x8A, 0xFF, 0x80);
const HALO_FG: Color = Color::Rgb(0xB3, 0x88, 0xFF);

/// État de la mascotte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MascotState {
    /// Immobile (frame unique, teint halo).
    #[default]
    Idle,
    /// Effrayée pendant [`SPOOK_DURATION`], frame spook.
    Spook,
}

/// Fantôme Kiro : position bornée à la zone reçue dans [`Mascot::draw`].
#[derive(Debug)]
pub struct Mascot {
    state: MascotState,
    x: u16,
    y: u16,
    bounds: Rect,
    spooked_at: Option<Instant>,
}

impl Mascot {
    /// Nouvelle mascotte immobile, en haut à gauche de sa future zone.
    pub fn new() -> Self {
        Self {
            state: MascotState::Idle,
            x: 0,
            y: 0,
            bounds: Rect::default(),
            spooked_at: None,
        }
    }

    /// État courant (pour `app.rs` : overlay, statut…).
    pub fn state(&self) -> MascotState {
        self.state
    }

    /// Position absolue du coin haut-gauche du fantôme.
    #[allow(dead_code)]
    pub fn pos(&self) -> (u16, u16) {
        (self.x, self.y)
    }

    /// Déclenche l'état `Spook` pour 2 s (retour en `Idle` au `tick`).
    pub fn spook(&mut self) {
        self.state = MascotState::Spook;
        self.spooked_at = Some(Instant::now());
    }

    /// Gère le retour de `Spook` vers `Idle` ; sinon ne fait rien.
    pub fn tick(&mut self) {
        if self.state == MascotState::Spook && self.spook_expired() {
            self.state = MascotState::Idle;
            self.spooked_at = None;
        }
    }

    /// Mémorise `area` comme zone d'affichage puis dessine le fantôme dedans.
    pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
        self.bounds = area;
        self.clamp();
        let (lines, style) = self.current_frame();
        let h = (lines.len() as u16).min(area.height);
        let rect = area.intersection(Rect::new(self.x, self.y, W.min(area.width), h));
        if rect.is_empty() {
            return;
        }
        frame.render_widget(Paragraph::new(lines).style(style), rect);
    }

    fn spook_expired(&self) -> bool {
        self.spooked_at
            .map(|t| t.elapsed() >= SPOOK_DURATION)
            .unwrap_or(true)
    }

    /// Re-borne la position pour que le fantôme tienne dans `bounds`.
    fn clamp(&mut self) {
        let max_x = self
            .bounds
            .x
            .saturating_add(self.bounds.width.saturating_sub(W));
        let max_y = self
            .bounds
            .y
            .saturating_add(self.bounds.height.saturating_sub(H));
        self.x = self.x.clamp(self.bounds.x, max_x);
        self.y = self.y.clamp(self.bounds.y, max_y);
    }

    /// Lignes + style de la frame courante selon l'état.
    fn current_frame(&self) -> (Vec<Line<'static>>, Style) {
        let lines: Vec<Line> = match self.state {
            MascotState::Spook => SPOOK.iter().map(|s| Line::from(*s)).collect(),
            MascotState::Idle => idle_lines(),
        };
        let style = match self.state {
            MascotState::Spook => Style::new().fg(ECTO_FG).add_modifier(Modifier::BOLD),
            MascotState::Idle => Style::new().fg(HALO_FG),
        };
        (lines, style)
    }
}

impl Default for Mascot {
    fn default() -> Self {
        Self::new()
    }
}

/// Tête commune + frange : la frame unique du fantôme immobile.
fn idle_lines() -> Vec<Line<'static>> {
    HEAD.iter()
        .map(|s| Line::from(*s))
        .chain(std::iter::once(Line::from(FRINGE)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn mascot_stays_bounded() {
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let mut ghost = Mascot::new();
        let area = Rect::new(4, 2, 40, 12);
        term.draw(|f| ghost.draw(f, area)).unwrap();
        let start = ghost.pos();
        ghost.spook();
        for _ in 0..1000 {
            ghost.tick();
            term.draw(|f| ghost.draw(f, area)).unwrap();
            let (x, y) = ghost.pos();
            assert!(
                x >= area.x && x + W <= area.x + area.width,
                "x={x} hors zone"
            );
            assert!(
                y >= area.y && y + H <= area.y + area.height,
                "y={y} hors zone"
            );
        }
        assert_eq!(ghost.pos(), start, "le fantome doit rester immobile");
    }
}
