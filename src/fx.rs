//! Horror overlay effects: static noise, blood drips, fear level.
//!
//! Deterministic rendering: every effect derives from the internal frame
//! counter (plus [`rand`] spawns in [`Fx::tick`]). Same frame => same picture.
//! Call [`Fx::draw`] last, after all other widgets, as a screen overlay.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

/// Dried blood (drips trail, borders).
const BLOOD: Color = Color::Rgb(139, 0, 0);
/// Fresh blood (drip heads, fear alert).
const FRESH_BLOOD: Color = Color::Rgb(220, 20, 60);
/// Ash grey (static, secondary).
const ASH: Color = Color::Rgb(107, 107, 117);
/// Candlelight (flicker tint, fear warning).
const CANDLE: Color = Color::Rgb(255, 179, 71);

/// Glyphs used for the static / snow overlay.
const STATIC_GLYPHS: [char; 5] = ['\u{2591}', '\u{2592}', '\u{2593}', '\u{2020}', '\u{B7}'];
/// Max simultaneous blood drips (calm / spook).
const MAX_DROPS: usize = 6;
const MAX_DROPS_SPOOK: usize = 14;
/// Fear gained per tick while spooked, lost per tick while calm.
const FEAR_UP: u8 = 7;
const FEAR_DOWN: u8 = 2;

/// One blood drip falling from the top. `y` is the head row (0 = top).
#[derive(Debug, Clone, Copy)]
struct Drop {
    x: u16,
    y: u16,
    len: u16,
}

/// Horror effects state machine.
#[derive(Debug)]
pub struct Fx {
    frame: u64,
    fear: u8,
    drops: Vec<Drop>,
    width: u16,
    height: u16,
}

impl Fx {
    /// Fresh effects: frame 0, fear 0, no drips.
    pub fn new() -> Self {
        Self {
            frame: 0,
            fear: 0,
            drops: Vec::new(),
            width: 80,
            height: 24,
        }
    }

    /// Current fear level (0-100).
    pub fn fear(&self) -> u8 {
        self.fear
    }

    /// Current frame counter (drives all deterministic effects).
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Advance one tick. While `spooked` the fear level climbs toward 100,
    /// drips multiply and static thickens; otherwise fear decays toward 0.
    pub fn tick(&mut self, spooked: bool) {
        self.frame = self.frame.wrapping_add(1);
        if spooked {
            self.fear = self.fear.saturating_add(FEAR_UP).min(100);
        } else {
            self.fear = self.fear.saturating_sub(FEAR_DOWN);
        }
        self.spawn_drop(spooked);
        if self.frame % 2 == 0 {
            for drop in &mut self.drops {
                drop.y = drop.y.saturating_add(1);
            }
        }
        let bottom = self.height.saturating_add(MAX_DROPS as u16);
        self.drops.retain(|drop| drop.y < bottom);
    }

    /// Maybe spawn one drip at the top edge, using `rand` for position.
    fn spawn_drop(&mut self, spooked: bool) {
        let limit = if spooked { MAX_DROPS_SPOOK } else { MAX_DROPS };
        if self.drops.len() >= limit {
            return;
        }
        let chance = if spooked { 60 } else { 15 };
        let roll: u64 = rand::random();
        if roll % 100 >= chance {
            return;
        }
        let span = self.width.max(1) as u64;
        self.drops.push(Drop {
            x: ((roll >> 8) % span) as u16,
            y: 0,
            len: (2 + (roll >> 24) % 4) as u16,
        });
    }

    /// Draw the overlay onto `buf` inside `area`. Remembers the area size
    /// so [`Fx::tick`] can spawn drips within bounds.
    pub fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        self.width = area.width;
        self.height = area.height;
        self.draw_static(area, buf);
        self.draw_drops(area, buf);
        self.draw_fear(area, buf);
    }

    /// Static / snow: scattered dim glyphs, denser as fear rises.
    /// Never touches the last 4 rows (input / status zone).
    fn draw_static(&self, area: Rect, buf: &mut Buffer) {
        let safe_height = area.height.saturating_sub(4).max(1) as u64;
        let width = area.width.max(1) as u64;
        let count = 10 + u64::from(self.fear) / 5;
        let hot = self.fear >= 80;
        for i in 0..count {
            let hash = splitmix(self.frame.wrapping_add(i.wrapping_mul(0x9E37_79B9)));
            let x = area.x.saturating_add((hash % width) as u16);
            let y = area.y.saturating_add(((hash >> 16) % safe_height) as u16);
            let glyph = STATIC_GLYPHS[((hash >> 32) % STATIC_GLYPHS.len() as u64) as usize];
            if let Some(cell) = buf.cell_mut((x, y)) {
                let mut style = Style::new().fg(ASH).add_modifier(Modifier::DIM);
                if hot && i % 4 == 0 {
                    style = Style::new().fg(CANDLE).add_modifier(Modifier::BOLD);
                }
                cell.set_symbol(&glyph.to_string()).set_style(style);
            }
        }
    }

    /// Blood drips: bright head `●` with a dim `│` trail above it.
    fn draw_drops(&self, area: Rect, buf: &mut Buffer) {
        let head_style = Style::new().fg(FRESH_BLOOD).add_modifier(Modifier::BOLD);
        let trail_style = Style::new().fg(BLOOD).add_modifier(Modifier::DIM);
        for drop in &self.drops {
            let x = area.x.saturating_add(drop.x % area.width.max(1));
            for k in 0..drop.len {
                let Some(y) = area.y.checked_add(drop.y.saturating_sub(k)) else {
                    continue;
                };
                if y >= area.y.saturating_add(area.height) {
                    continue;
                }
                if let Some(cell) = buf.cell_mut((x, y)) {
                    if k == 0 {
                        cell.set_symbol("\u{25CF}").set_style(head_style);
                    } else {
                        cell.set_symbol("\u{2502}").set_style(trail_style);
                    }
                }
            }
        }
    }

    /// Fear meter, top-right corner: `FEAR 042`, coloured by level.
    fn draw_fear(&self, area: Rect, buf: &mut Buffer) {
        if area.width < 12 {
            return;
        }
        let label = format!("FEAR {:03}", self.fear);
        let colour = if self.fear >= 80 {
            FRESH_BLOOD
        } else if self.fear >= 40 {
            CANDLE
        } else {
            ASH
        };
        let style = Style::new().fg(colour).add_modifier(Modifier::BOLD);
        let x0 = area
            .x
            .saturating_add(area.width)
            .saturating_sub(label.len() as u16);
        for (i, ch) in label.chars().enumerate() {
            if let Some(cell) = buf.cell_mut((x0.saturating_add(i as u16), area.y)) {
                cell.set_symbol(&ch.to_string()).set_style(style);
            }
        }
    }
}

impl Default for Fx {
    fn default() -> Self {
        Self::new()
    }
}

/// SplitMix64: tiny deterministic hash for static placement.
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
            fx.draw(area, &mut buf);
        }
    }
}
