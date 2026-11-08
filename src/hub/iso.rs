//! Crypte en vue isométrique (dalles 2:1) : projection dalles → pixels et
//! décor pixel art — sol dallé éclairé à la bougie, deux murs de briques,
//! fenêtre gothique au clair de lune, toile d'araignée, tombes, bougies.

use image::{Rgba, RgbaImage};
use ratatui::style::Color;

use crate::theme;

/// Tombe vue de trois quarts (`S` pierre, `U` ombre, `K` gravure).
const TOMB: [&str; 8] = [
    ".SSS.", "SSSSU", "SKSSU", "KKKSU", "SKSSU", "SSSSU", "SSSSU", "UUUUU",
];

/// Géométrie de la crypte dans le canevas.
#[derive(Debug, Clone, Copy)]
pub struct Room {
    /// Côté en dalles.
    pub n: f32,
    tw: f32,
    th: f32,
    ox: f32,
    oy: f32,
    wall: f32,
}

impl Room {
    /// Plus grande crypte (3 à 14 dalles) qui tient dans `cw`×`ch` pixels.
    pub fn fit(cw: u32, ch: u32) -> Room {
        let big = cw >= 200 && ch >= 110;
        let (tw, th) = if big { (16.0, 8.0) } else { (8.0, 4.0) };
        let wall = th * 4.0;
        let fit_w = (cw as f32 - 4.0) / tw;
        let fit_h = (ch as f32 - wall - 8.0) / th;
        let n = fit_w.min(fit_h).floor().clamp(3.0, 14.0);
        let ox = (cw as f32 / 2.0).floor();
        let oy = ((ch as f32 - n * th - wall) / 2.0).floor().max(0.0) + wall;
        Room {
            n,
            tw,
            th,
            ox,
            oy,
            wall,
        }
    }

    /// Hauteur d'une dalle en pixels (échelle verticale des effets).
    pub fn tile_h(&self) -> f32 {
        self.th
    }

    /// Point au sol (dalles) → pixel du canevas.
    pub fn project(&self, wx: f32, wy: f32) -> (f32, f32) {
        (
            self.ox + (wx - wy) * self.tw / 2.0,
            self.oy + (wx + wy) * self.th / 2.0,
        )
    }

    /// Pixel du canevas → point au sol (dalles).
    fn unproject(&self, sx: f32, sy: f32) -> (f32, f32) {
        let a = (sx - self.ox) / (self.tw / 2.0);
        let b = (sy - self.oy) / (self.th / 2.0);
        ((a + b) / 2.0, (b - a) / 2.0)
    }

    /// Dessine le décor complet (avant les esprits).
    pub fn draw(&self, cv: &mut RgbaImage, tick: u64) {
        self.floor(cv);
        self.walls(cv);
        self.window(cv);
        self.cobweb(cv);
        for k in [0.3, 0.7] {
            let (x, y) = self.project(0.55, self.n * k);
            stamp(cv, &TOMB, x as i64 - 2, y as i64 - 8);
        }
        let flame = (tick / 3) % 4 < 2;
        for (wx, wy) in [(self.n * 0.5, 0.45), (self.n - 0.6, 0.45)] {
            let (x, y) = self.project(wx, wy);
            candle(cv, x as i64, y as i64, flame);
        }
    }

    /// Sol : damier de pierres, joints sombres, lumière au centre et
    /// halos chauds autour des bougies.
    fn floor(&self, cv: &mut RgbaImage) {
        let grout = 0.9 / self.th;
        let lights = [(self.n * 0.5, 0.45), (self.n - 0.6, 0.45)];
        for y in 0..cv.height() {
            for x in 0..cv.width() {
                let (wx, wy) = self.unproject(x as f32 + 0.5, y as f32 + 0.5);
                if wx < 0.0 || wy < 0.0 || wx >= self.n || wy >= self.n {
                    continue;
                }
                let joint = wx.fract() < grout || wy.fract() < grout;
                let even = (wx as i32 + wy as i32) % 2 == 0;
                let stone = match (joint, even) {
                    (true, _) => theme::lerp(theme::VOID, theme::UMBER, 0.2),
                    (false, true) => theme::lerp(theme::VOID, theme::UMBER, 0.62),
                    (false, false) => theme::lerp(theme::VOID, theme::UMBER, 0.5),
                };
                let c = self.n * 0.55;
                let dark = (((wx - c).powi(2) + (wy - c).powi(2)).sqrt() / self.n).min(1.0);
                let mut colour = theme::lerp(stone, theme::VOID, dark * 0.4);
                for (lx, ly) in lights {
                    let d = ((wx - lx).powi(2) + (wy - ly).powi(2)).sqrt();
                    colour = theme::lerp(colour, theme::CANDLE_DARK, (0.35 - d * 0.18).max(0.0));
                }
                cv.put_pixel(x, y, px(colour));
            }
        }
    }

    /// Murs du fond : briques en quinconce, chaperon sable, pied sombre.
    fn walls(&self, cv: &mut RgbaImage) {
        let half = self.n * self.tw / 2.0;
        let slope = self.th / self.tw;
        for u in 0..half as i64 {
            for v in 0..self.wall as i64 {
                let left = (
                    self.ox as i64 - u - 1,
                    self.oy as i64 + (u as f32 * slope) as i64 - v - 1,
                );
                let right = (
                    self.ox as i64 + u,
                    self.oy as i64 + (u as f32 * slope) as i64 - v - 1,
                );
                put(cv, left.0, left.1, self.brick(u, v, theme::UMBER));
                let shade = theme::lerp(theme::UMBER, theme::VOID, 0.3);
                put(cv, right.0, right.1, self.brick(u, v, shade));
            }
        }
    }

    /// Couleur d'un pixel de mur à (`u` le long du mur, `v` en hauteur).
    fn brick(&self, u: i64, v: i64, base: Color) -> Rgba<u8> {
        let top = self.wall as i64 - 1;
        let course = v / 4;
        let joint = v % 4 == 0 || (u + course % 2 * 4) % 8 == 0;
        let colour = if v == top {
            theme::lerp(theme::SAND, theme::UMBER, 0.55)
        } else if joint {
            theme::lerp(base, theme::VOID, 0.55)
        } else if v < 3 {
            theme::lerp(base, theme::VOID, 0.35)
        } else {
            base
        };
        px(colour)
    }

    /// Fenêtre ogivale au milieu du mur droit : nuit violette, lune,
    /// meneaux noirs.
    fn window(&self, cv: &mut RgbaImage) {
        let half = self.n * self.tw / 2.0;
        let slope = self.th / self.tw;
        let w = (self.tw / 2.0).max(4.0) as i64;
        let (u0, v0, h) = (
            (half / 2.0) as i64 - w / 2,
            (self.wall * 0.3) as i64,
            (self.wall * 0.55) as i64,
        );
        for du in 0..w {
            for dv in 0..h {
                let arch = dv > h - w / 2 && (du - w / 2).abs() > w / 2 - (h - dv);
                if arch {
                    continue;
                }
                let mullion = du == w / 2 || dv == h / 2;
                let moon = (du - w / 4).abs() <= 1 && (dv - h * 3 / 4).abs() <= 1;
                let colour = match (mullion, moon) {
                    (true, _) => theme::INK,
                    (false, true) => theme::CANDLE,
                    _ => theme::SPECTRE_DARK,
                };
                let u = u0 + du;
                put(
                    cv,
                    self.ox as i64 + u,
                    self.oy as i64 + (u as f32 * slope) as i64 - v0 - dv - 1,
                    px(colour),
                );
            }
        }
    }

    /// Toile d'araignée dans l'angle du fond.
    fn cobweb(&self, cv: &mut RgbaImage) {
        let (x, y) = (self.ox as i64, self.oy as i64 - self.wall as i64);
        let silk = px(theme::lerp(theme::SAND, theme::VOID, 0.45));
        let reach = (self.wall * 0.45) as i64;
        for k in 0..reach {
            put(cv, x, y + k, silk);
            put(cv, x - k, y + k / 2, silk);
            put(cv, x + k, y + k / 2, silk);
        }
        for r in [reach / 3, reach * 2 / 3] {
            for d in -r..=r {
                put(cv, x + d, y + r / 2 + (r - d.abs()) / 2, silk);
            }
        }
    }
}

/// Bougie posée au sol en (`x`, `y`), flamme vacillante.
fn candle(cv: &mut RgbaImage, x: i64, y: i64, high: bool) {
    for dy in 1..=4 {
        put(cv, x, y - dy, px(theme::BONE));
    }
    let (tip, base) = if high {
        (theme::CANDLE, theme::PUMPKIN)
    } else {
        (theme::PUMPKIN, theme::CANDLE)
    };
    put(cv, x, y - 5, px(base));
    put(cv, x, y - 6, px(tip));
}

/// Colle une petite grille de palette (`S`, `U`, `K`, `.`) en (`x0`, `y0`).
fn stamp(cv: &mut RgbaImage, rows: &[&str], x0: i64, y0: i64) {
    for (dy, row) in rows.iter().enumerate() {
        for (dx, ch) in row.chars().enumerate() {
            let colour = match ch {
                'S' => theme::lerp(theme::SAND, theme::UMBER, 0.35),
                'U' => theme::UMBER,
                'K' => theme::INK,
                _ => continue,
            };
            put(cv, x0 + dx as i64, y0 + dy as i64, px(colour));
        }
    }
}

pub use crate::canvas::{put, px};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_round_trips_and_room_fits() {
        for (cw, ch) in [(60, 40), (273, 206), (400, 300)] {
            let room = Room::fit(cw, ch);
            let (x, y) = room.project(room.n, room.n);
            assert!(y <= ch as f32, "sol hors canevas ({cw}x{ch})");
            let (left, _) = room.project(0.0, room.n);
            let (right, _) = room.project(room.n, 0.0);
            assert!(left >= 0.0 && right <= cw as f32 && x >= 0.0);
            let (wx, wy) = room.unproject(room.project(2.5, 1.5).0, room.project(2.5, 1.5).1);
            assert!((wx - 2.5).abs() < 1e-4 && (wy - 1.5).abs() < 1e-4);
        }
    }

    #[test]
    fn decor_paints_floor_and_walls() {
        let room = Room::fit(273, 206);
        let mut cv = RgbaImage::from_pixel(273, 206, Rgba([0, 0, 0, 255]));
        room.draw(&mut cv, 3);
        let lit = cv.pixels().filter(|p| p.0 != [0, 0, 0, 255]).count();
        assert!(lit > 273 * 206 / 4, "crypte presque vide ({lit} px)");
    }
}
