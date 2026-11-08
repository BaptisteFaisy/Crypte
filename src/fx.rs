//! Effets horreur en overlay : vignette chaude fixe, brume, éclair
//! d'effroi, niveau de peur.
//!
//! Tout est en truecolor (Ghostty). La brume ne teinte que le fond, et
//! rien ne touche aux zones d'images (`protect`) : texte et pixel art
//! restent nets.
//! [`Fx::paint_background`] passe en premier, [`Fx::draw`] en dernier.

use ratatui::{
    buffer::{Buffer, Cell},
    layout::Rect,
    style::Color,
};

use crate::theme::{self, BONE, CRYPT, DRIED, SPECTRE, VOID};

/// Brume : paliers de densité (limite le repeint) et teinte max.
const FOG_STEPS: f32 = 10.0;
const FOG_MAX: f32 = 0.16;
/// Peur gagnée par tick en effroi, perdue par tick au calme.
const FEAR_UP: u8 = 7;
const FEAR_DOWN: u8 = 2;
/// Frames d'éclair au début d'un effroi.
const FLASH_FRAMES: u8 = 4;
/// Paliers de lumière de la vignette (limite le repeint du fond).
const LIGHT_STEPS: f32 = 20.0;

/// Machine à effets.
#[derive(Debug)]
pub struct Fx {
    frame: u64,
    fear: u8,
    flash: u8,
    spooked: bool,
}

impl Fx {
    /// Effets neufs : frame 0, peur 0.
    pub fn new() -> Self {
        Self {
            frame: 0,
            fear: 0,
            flash: 0,
            spooked: false,
        }
    }

    /// Un tick. En effroi la peur monte vers 100 et un éclair part au
    /// premier tick ; au calme la peur redescend.
    pub fn tick(&mut self, spooked: bool) {
        self.frame = self.frame.wrapping_add(1);
        if spooked && !self.spooked {
            self.flash = FLASH_FRAMES;
        } else {
            self.flash = self.flash.saturating_sub(1);
        }
        self.spooked = spooked;
        self.fear = if spooked {
            self.fear.saturating_add(FEAR_UP).min(100)
        } else {
            self.fear.saturating_sub(FEAR_DOWN)
        };
    }

    /// Fond de la crypte : vignette chaude fixe centrée en haut ; rougit
    /// avec la peur. À appeler **avant** les widgets.
    pub fn paint_background(&self, area: Rect, buf: &mut Buffer) {
        let glow = theme::lerp(CRYPT, DRIED, f32::from(self.fear) / 160.0);
        let (cx, cy) = (f32::from(area.width) * 0.5, f32::from(area.height) * 0.3);
        let reach = f32::from(area.width.max(area.height * 2)).max(1.0) * 0.62;
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let dx = f32::from(x - area.x) - cx;
                let dy = (f32::from(y - area.y) - cy) * 2.0;
                let d = ((dx * dx + dy * dy).sqrt() / reach).min(1.0);
                let light = ((1.0 - d * d) * LIGHT_STEPS).round() / LIGHT_STEPS;
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_bg(theme::lerp(VOID, glow, light));
                }
            }
        }
    }

    /// Overlay : brume et éclair, hors des zones `protect` (images).
    /// À appeler **après** les widgets.
    pub fn draw(&mut self, area: Rect, buf: &mut Buffer, protect: &[Rect]) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let mut layer = Layer { buf, protect };
        self.draw_fog(area, &mut layer);
        if self.flash > 0 {
            self.draw_flash(area, &mut layer);
        }
    }

    /// Brume basse : nappes fluides qui teintent le **fond** (jamais le
    /// texte), plus épaisses vers le bas, dérivant lentement. Épargne les
    /// 4 dernières rangées (saisie, statut).
    fn draw_fog(&self, area: Rect, layer: &mut Layer) {
        let bottom = area.bottom().saturating_sub(4);
        let top = bottom.saturating_sub(area.height * 2 / 5);
        let t = self.frame as f32 * 0.04;
        for y in top..bottom {
            let depth = f32::from(y - top) / f32::from((bottom - top).max(1));
            for x in area.left()..area.right() {
                let (fx, fy) = (f32::from(x), f32::from(y));
                let wave = (fx * 0.13 + t).sin() + (fx * 0.05 - fy * 0.6 - t * 0.7).sin();
                let density = ((wave * 0.25 + 0.5) * depth * FOG_STEPS).round() / FOG_STEPS;
                if density <= 0.0 {
                    continue;
                }
                if let Some(cell) = layer.cell(x, y) {
                    cell.set_bg(theme::lerp(bg_of(cell), SPECTRE, density * FOG_MAX));
                }
            }
        }
    }

    /// Éclair : tout le fond blanchit puis retombe en 4 frames.
    fn draw_flash(&self, area: Rect, layer: &mut Layer) {
        let k = f32::from(self.flash) / f32::from(FLASH_FRAMES) * 0.55;
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if let Some(cell) = layer.cell(x, y) {
                    cell.set_bg(theme::lerp(bg_of(cell), BONE, k));
                }
            }
        }
    }
}

/// Buffer vu par l'overlay : cellules hors images et hors `skip` seulement.
struct Layer<'a> {
    buf: &'a mut Buffer,
    protect: &'a [Rect],
}

impl Layer<'_> {
    /// Cellule modifiable à `(x, y)` (ni image, ni `skip`).
    fn cell(&mut self, x: u16, y: u16) -> Option<&mut Cell> {
        let pos = ratatui::layout::Position::new(x, y);
        if self.protect.iter().any(|r| r.contains(pos)) {
            return None;
        }
        self.buf.cell_mut(pos).filter(|cell| !cell.skip)
    }
}

impl Default for Fx {
    fn default() -> Self {
        Self::new()
    }
}

/// Fond RGB d'une cellule (le vide de la crypte si non peint).
fn bg_of(cell: &Cell) -> Color {
    match cell.bg {
        c @ Color::Rgb(..) => c,
        _ => VOID,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fear_rises_while_spooked_and_clamps_at_100() {
        let mut fx = Fx::new();
        for _ in 0..50 {
            fx.tick(true);
        }
        assert_eq!(fx.fear, 100);
    }

    #[test]
    fn fear_decays_while_calm_and_clamps_at_0() {
        let mut fx = Fx::new();
        for _ in 0..50 {
            fx.tick(true);
        }
        for _ in 0..100 {
            fx.tick(false);
        }
        assert_eq!(fx.fear, 0);
    }

    #[test]
    fn draw_never_panics_on_any_area() {
        let mut fx = Fx::new();
        for _ in 0..8 {
            fx.tick(true);
        }
        for (w, h) in [(0, 0), (1, 1), (10, 5), (80, 24), (120, 40)] {
            let area = Rect::new(0, 0, w, h);
            let mut buf = Buffer::empty(area);
            fx.paint_background(area, &mut buf);
            fx.draw(area, &mut buf, &[]);
        }
    }

    #[test]
    fn overlay_leaves_no_glyph_behind() {
        let mut fx = Fx::new();
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        for _ in 0..30 {
            fx.tick(true);
            fx.draw(area, &mut buf, &[]);
        }
        assert!(
            buf.content.iter().all(|c| c.symbol() == " "),
            "un effet dessine encore un glyphe"
        );
    }

    #[test]
    fn overlay_never_touches_text() {
        let mut fx = Fx::new();
        let area = Rect::new(0, 0, 40, 20);
        let mut buf = Buffer::empty(area);
        let text = "x".repeat(40);
        for y in 0..20 {
            buf.set_string(0, y, &text, ratatui::style::Style::new());
        }
        for _ in 0..30 {
            fx.tick(true);
            fx.draw(area, &mut buf, &[]);
        }
        assert!(buf.content.iter().all(|c| c.symbol() == "x"));
    }

    #[test]
    fn overlay_never_touches_images() {
        let mut fx = Fx::new();
        let area = Rect::new(0, 0, 40, 20);
        let image = Rect::new(5, 5, 10, 12);
        let mut buf = Buffer::empty(area);
        fx.paint_background(area, &mut buf);
        let before = buf.clone();
        for _ in 0..30 {
            fx.tick(true);
            fx.draw(area, &mut buf, &[image]);
        }
        for pos in image.positions() {
            assert_eq!(buf[pos], before[pos], "image touchée en {pos:?}");
        }
    }
}
