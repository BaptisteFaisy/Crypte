//! Mascotte Kiro, le fantôme gardien : immobile, centrée dans son panneau,
//! s'effraie sur demande (F5).
//!
//! Contrat : `Mascot::new` → `tick` (gère le retour de `Spook`) → `spook`
//! (`Spook` pendant 2 s) ; `draw` rend l'illustration (Kitty dans Ghostty,
//! demi-blocs ailleurs) et sa légende, toujours au même endroit.

use std::time::{Duration, Instant};

use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::Paragraph,
    Frame,
};

use crate::bestiary::{self, Creature, KIRO_SPOOKED};
use crate::gfx::ImageBank;
use crate::theme;

/// Durée de l'état `Spook` après [`Mascot::spook`].
const SPOOK_DURATION: Duration = Duration::from_secs(2);

/// État de la mascotte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MascotState {
    /// Veille (fantôme spectral).
    #[default]
    Idle,
    /// Effrayée pendant [`SPOOK_DURATION`] (ectoplasme, bras levés).
    Spook,
}

/// Fantôme Kiro.
#[derive(Debug, Default)]
pub struct Mascot {
    state: MascotState,
    spooked_at: Option<Instant>,
}

impl Mascot {
    /// Nouvelle mascotte en veille.
    pub fn new() -> Self {
        Self::default()
    }

    /// État courant (pour `app.rs` : overlay, statut…).
    pub fn state(&self) -> MascotState {
        self.state
    }

    /// Déclenche l'état `Spook` pour 2 s (retour en `Idle` au `tick`).
    pub fn spook(&mut self) {
        self.state = MascotState::Spook;
        self.spooked_at = Some(Instant::now());
    }

    /// Gère le retour de `Spook` vers `Idle` ; sinon ne fait rien.
    pub fn tick(&mut self) {
        let expired = self
            .spooked_at
            .map(|t| t.elapsed() >= SPOOK_DURATION)
            .unwrap_or(true);
        if self.state == MascotState::Spook && expired {
            self.state = MascotState::Idle;
            self.spooked_at = None;
        }
    }

    /// Illustration de l'état courant.
    pub fn creature(&self) -> &'static Creature {
        match self.state {
            MascotState::Idle => bestiary::get("fantome"),
            MascotState::Spook => &KIRO_SPOOKED,
        }
    }

    /// Kiro centré dans `area`, légende sur la dernière ligne.
    pub fn draw(&self, frame: &mut Frame, area: Rect, bank: &mut ImageBank) {
        let (art, caption) = ghost_rects(area);
        bank.draw(frame, art, self.creature());
        let (text, colour) = match self.state {
            MascotState::Idle => ("Kiro veille", theme::SPECTRE),
            MascotState::Spook => ("Kiro a peur !", theme::ECTO),
        };
        let style = Style::new().fg(colour).add_modifier(Modifier::BOLD);
        let line = Paragraph::new(Line::styled(text, style)).alignment(Alignment::Center);
        frame.render_widget(line, caption);
    }
}

/// Zones fixes (indépendantes de l'état) : image au-dessus, légende dessous.
fn ghost_rects(area: Rect) -> (Rect, Rect) {
    let art_h = area.height.saturating_sub(1);
    let art = Rect {
        height: art_h,
        ..area
    };
    let caption = Rect::new(area.x, area.y + art_h, area.width, area.height.min(1));
    (art, caption)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn spook_switches_art_but_never_moves() {
        let mut ghost = Mascot::new();
        let mut bank = ImageBank::headless();
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let area = Rect::new(4, 2, 30, 12);
        let calm = ghost_rects(area);
        assert_eq!(ghost.creature().slug, "fantome");
        ghost.spook();
        assert_eq!(ghost.state(), MascotState::Spook);
        assert_eq!(ghost.creature().slug, "kiro-effroi");
        for _ in 0..50 {
            ghost.tick();
            term.draw(|f| ghost.draw(f, area, &mut bank)).unwrap();
            assert_eq!(ghost_rects(area), calm, "le fantome doit rester immobile");
        }
        let (art, caption) = calm;
        assert!(area.contains(art.as_position()) && caption.bottom() <= area.bottom());
    }
}
