//! Effets horreur en overlay : vignette à la bougie, brume, static,
//! gouttes de sang, éclair d'effroi, niveau de peur.
//!
//! Tout est en truecolor (Ghostty). Le static et le sang ne touchent que
//! les cellules **vides**, la brume ne teinte que le fond, et rien ne
//! touche aux zones d'images (`protect`) : texte et pixel art restent nets.
//! [`Fx::paint_background`] passe en premier, [`Fx::draw`] en dernier.

use ratatui::{
    buffer::{Buffer, Cell},
    layout::Rect,
    style::{Color, Modifier},
};

use crate::theme::{self, ASH, BLOOD, BONE, CANDLE, CRYPT, DRIED, SPECTRE, VOID};

/// Glyphes du static (croix, points, poussière).
const STATIC_GLYPHS: [&str; 5] = ["†", "·", "⁘", "˙", "∴"];
/// Brume : paliers de densité (limite le repeint) et teinte max.
const FOG_STEPS: f32 = 10.0;
const FOG_MAX: f32 = 0.16;
/// Gouttes simultanées max (calme / effroi).
const MAX_DROPS: usize = 6;
const MAX_DROPS_SPOOK: usize = 14;
/// Peur gagnée par tick en effroi, perdue par tick au calme.
const FEAR_UP: u8 = 7;
const FEAR_DOWN: u8 = 2;
/// Frames d'éclair au début d'un effroi.
const FLASH_FRAMES: u8 = 4;
/// Paliers de lumière de la vignette (limite le repeint du fond).
const LIGHT_STEPS: f32 = 20.0;

/// Une goutte de sang qui tombe du haut. `y` = rangée de la tête.
#[derive(Debug, Clone, Copy)]
struct Drop {
    x: u16,
    y: u16,
    len: u16,
}

/// Machine à effets.
#[derive(Debug)]
pub struct Fx {
    frame: u64,
    fear: u8,
    drops: Vec<Drop>,
    width: u16,
    height: u16,
    flash: u8,
    spooked: bool,
    candle: f32,
}

impl Fx {
    /// Effets neufs : frame 0, peur 0, aucune goutte.
    pub fn new() -> Self {
        Self {
            frame: 0,
            fear: 0,
            drops: Vec::new(),
            width: 80,
            height: 24,
            flash: 0,
            spooked: false,
            candle: 1.0,
        }
    }

    /// Niveau de peur (0-100).
    pub fn fear(&self) -> u8 {
        self.fear
    }

    /// Un tick. En effroi la peur monte vers 100, les gouttes se multiplient
    /// et un éclair part au premier tick ; au calme la peur redescend.
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
        self.flicker();
        self.spawn_drop(spooked);
        if self.frame % 2 == 0 {
            for drop in &mut self.drops {
                drop.y = drop.y.saturating_add(1);
            }
        }
        let bottom = self.height.saturating_add(MAX_DROPS as u16);
        self.drops.retain(|drop| drop.y < bottom);
    }

    /// Flamme de bougie : la lumière change de palier de temps en temps.
    fn flicker(&mut self) {
        let roll: u64 = rand::random();
        if roll % 100 < 18 {
            self.candle = [0.86, 0.93, 1.0, 1.0][(roll >> 8) as usize % 4];
        }
    }

    /// Peut-être une goutte en haut de l'écran (position aléatoire).
    fn spawn_drop(&mut self, spooked: bool) {
        let limit = if spooked { MAX_DROPS_SPOOK } else { MAX_DROPS };
        let chance = if spooked { 60 } else { 15 };
        let roll: u64 = rand::random();
        if self.drops.len() >= limit || roll % 100 >= chance {
            return;
        }
        self.drops.push(Drop {
            x: ((roll >> 8) % self.width.max(1) as u64) as u16,
            y: 0,
            len: (2 + (roll >> 24) % 4) as u16,
        });
    }

    /// Fond de la crypte : vignette chaude centrée en haut, à la bougie ;
    /// rougit avec la peur. À appeler **avant** les widgets.
    pub fn paint_background(&self, area: Rect, buf: &mut Buffer) {
        let glow = theme::lerp(CRYPT, DRIED, f32::from(self.fear) / 160.0);
        let (cx, cy) = (f32::from(area.width) * 0.5, f32::from(area.height) * 0.3);
        let reach = f32::from(area.width.max(area.height * 2)).max(1.0) * 0.62;
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let dx = f32::from(x - area.x) - cx;
                let dy = (f32::from(y - area.y) - cy) * 2.0;
                let d = ((dx * dx + dy * dy).sqrt() / reach).min(1.0);
                let light = ((1.0 - d * d) * self.candle * LIGHT_STEPS).round() / LIGHT_STEPS;
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_bg(theme::lerp(VOID, glow, light));
                }
            }
        }
    }

    /// Overlay : brume, static, gouttes, éclair, hors des zones `protect`
    /// (images). À appeler **après** les widgets. Mémorise la taille pour
    /// placer les gouttes au prochain tick.
    pub fn draw(&mut self, area: Rect, buf: &mut Buffer, protect: &[Rect]) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        self.width = area.width;
        self.height = area.height;
        let mut layer = Layer { buf, protect };
        self.draw_fog(area, &mut layer);
        self.draw_static(area, &mut layer);
        self.draw_drops(area, &mut layer);
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

    /// Static : glyphes épars, plus denses avec la peur, bougie si panique.
    fn draw_static(&self, area: Rect, layer: &mut Layer) {
        let safe_height = area.height.saturating_sub(5).max(1) as u64;
        let width = area.width.max(1) as u64;
        let count = 10 + u64::from(self.fear) / 4;
        for i in 0..count {
            let hash = splitmix(self.frame.wrapping_add(i.wrapping_mul(0x9E37_79B9)));
            let x = area.x.saturating_add((hash % width) as u16);
            let y = area.y + 1 + ((hash >> 16) % safe_height) as u16;
            let glyph = STATIC_GLYPHS[((hash >> 32) % STATIC_GLYPHS.len() as u64) as usize];
            let hot = self.fear >= 80 && i % 4 == 0;
            if let Some(cell) = layer.blank(x, y) {
                let fg = if hot {
                    CANDLE
                } else {
                    theme::lerp(bg_of(cell), ASH, 0.55)
                };
                cell.set_symbol(glyph).set_fg(fg);
            }
        }
    }

    /// Gouttes : tête `●` sang frais, traînée `│` sang séché au-dessus.
    fn draw_drops(&self, area: Rect, layer: &mut Layer) {
        for drop in &self.drops {
            let x = area.x.saturating_add(drop.x % area.width.max(1));
            for k in 0..drop.len {
                let Some(y) = area.y.checked_add(drop.y.saturating_sub(k)) else {
                    continue;
                };
                if y >= area.bottom() || (k > 0 && drop.y < k) {
                    continue;
                }
                if let Some(cell) = layer.blank(x, y) {
                    let (glyph, fg) = if k == 0 {
                        ("●", BLOOD)
                    } else {
                        ("│", DRIED)
                    };
                    cell.set_symbol(glyph).set_fg(fg);
                    if k == 0 {
                        cell.modifier.insert(Modifier::BOLD);
                    }
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

    /// Cellule modifiable **et vide** (un espace) à `(x, y)`.
    fn blank(&mut self, x: u16, y: u16) -> Option<&mut Cell> {
        self.cell(x, y).filter(|cell| cell.symbol() == " ")
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

/// SplitMix64 : petit hash déterministe pour placer static et brume.
fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
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
        assert_eq!(fx.fear(), 100);
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
        assert_eq!(fx.fear(), 0);
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
