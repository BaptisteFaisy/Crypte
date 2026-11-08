//! Décor « 32 bits » derrière et devant le niveau (vue 960×576), aux
//! couleurs du thème : ciel en bandes tramées à leurs jointures, étoiles,
//! lune de sang (précalculés une fois : `Sky`), collines et chapelles,
//! arbres morts du bestiaire et grille de fer en parallaxe, brume ;
//! éclairage du niveau (Kiro, citrouilles, lanternes, flammes), vignette.
//! À 240 images/s, rien n'est recalculé pixel par pixel sans table.

use image::{Rgba, RgbaImage};
use ratatui::style::Color;

use super::sprites::Sprites;
use super::{DT, VIEW_H, VIEW_W};
use crate::canvas::{blit, px, Ink};
use crate::theme;

/// Bandes de couleur du ciel.
const BANDS: usize = 9;
/// Ligne d'horizon du dégradé (pixels).
const HORIZON: f32 = 450.0;
/// Lumière ambiante du niveau (le reste vient des sources).
const AMBIENT: f32 = 0.66;

/// Source de lumière (écran) : centre, rayon, intensité.
#[derive(Debug, Clone, Copy)]
pub struct Light {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub k: f32,
}

/// Ciel fixe précalculé (normal et blanchi par l'éclair), vignette
/// comprise : coins assombris une fois pour toutes (le shader Ghostty de
/// la crypte en ajoute une sur toute la fenêtre).
pub struct Sky {
    plain: RgbaImage,
    flash: RgbaImage,
}

impl Sky {
    pub fn new() -> Sky {
        let table = vignette_table();
        let (mut plain, mut flash) = (still(false), still(true));
        vignette(&mut plain, &table);
        vignette(&mut flash, &table);
        Sky { plain, flash }
    }

    /// Copie le ciel fixe dans `cv` (même taille).
    pub fn paint(&self, cv: &mut RgbaImage, flash: bool) {
        let src = if flash { &self.flash } else { &self.plain };
        let dst: &mut [u8] = cv;
        dst.copy_from_slice(src);
    }
}

/// Assombrit les bords selon la table (plage intacte par ligne).
fn vignette(cv: &mut RgbaImage, table: &[(u32, u32, Vec<u16>)]) {
    {
        let w = VIEW_W as usize;
        let raw: &mut [u8] = cv;
        for (y, (lo, hi, k)) in table.iter().enumerate() {
            let row = &mut raw[y * w * 4..(y + 1) * w * 4];
            for (x, px) in row.chunks_exact_mut(4).enumerate() {
                if (*lo as usize..*hi as usize).contains(&x) {
                    continue;
                }
                let f = u32::from(k[x]);
                for c in &mut px[..3] {
                    *c = ((u32::from(*c) * f) >> 8) as u8;
                }
            }
        }
    }
}

impl Default for Sky {
    fn default() -> Self {
        Self::new()
    }
}

/// Dégradé + étoiles + lune, une fois pour toutes.
fn still(flash: bool) -> RgbaImage {
    let mut cv = RgbaImage::new(VIEW_W, VIEW_H);
    gradient(&mut cv, flash);
    stars(&mut cv);
    moon(&mut cv);
    cv
}

/// Bandes du noir au violet sang ; jointures tramées en damier.
fn gradient(cv: &mut RgbaImage, flash: bool) {
    let top = theme::lerp(theme::VOID, theme::SPECTRE_DARK, 0.6);
    let low = theme::lerp(theme::SPECTRE_DARK, theme::DRIED, 0.55);
    let bands: Vec<Rgba<u8>> = (0..BANDS)
        .map(|i| {
            let t = i as f32 / (BANDS - 1) as f32;
            let c = if t < 0.55 {
                theme::lerp(theme::VOID, top, t / 0.55)
            } else {
                theme::lerp(top, low, (t - 0.55) / 0.45)
            };
            px(if flash {
                theme::lerp(c, theme::SPECTRE, 0.45)
            } else {
                c
            })
        })
        .collect();
    for y in 0..VIEW_H {
        let t = (y as f32 / HORIZON).min(1.0) * (BANDS - 1) as f32;
        let (lo, frac) = (t.floor() as usize, t.fract());
        let hi = (lo + 1).min(BANDS - 1);
        for x in 0..VIEW_W {
            let mixed = frac > 0.7 && (x + y) % 2 == 0 || frac > 0.85;
            cv.put_pixel(x, y, bands[if mixed { hi } else { lo }]);
        }
    }
}

/// Hachage entier → pseudo-aléatoire stable (étoiles, cratères, bulles).
pub fn hash(x: i64, y: i64) -> u64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 31;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^ (h >> 29)
}

/// Étoiles fixes, quelques-unes plus pâles.
fn stars(cv: &mut RgbaImage) {
    for y in 30..330 {
        for x in 0..VIEW_W {
            let h = hash(i64::from(x), i64::from(y));
            if h % 431 == 0 {
                let c = if h % 3 == 0 { theme::ASH } else { theme::BONE };
                cv.put_pixel(x, y, px(c));
            }
        }
    }
}

/// Lune de sang : disque ombré, cratères, halo tramé.
fn moon(cv: &mut RgbaImage) {
    let (cx, cy, r) = (760i64, 130i64, 52.0f32);
    for dy in -78..=78i64 {
        for dx in -78..=78i64 {
            let (x, y) = (cx + dx, cy + dy);
            let d = ((dx * dx + dy * dy) as f32).sqrt();
            let c = if d <= r {
                let shade = ((dx + dy) as f32 / (2.0 * r) + 0.5).clamp(0.0, 1.0);
                let base = theme::lerp(theme::BLOOD, theme::DRIED, shade * 0.8);
                if hash(dx / 7, dy / 7) % 5 == 0 {
                    theme::lerp(base, theme::DRIED, 0.6)
                } else {
                    base
                }
            } else if d - r < 14.0 && (x + y) % 2 == 0 {
                let under = cv.get_pixel(x as u32, y as u32).0;
                theme::lerp(Color::Rgb(under[0], under[1], under[2]), theme::DRIED, 0.4)
            } else {
                continue;
            };
            cv.put_pixel(x as u32, y as u32, px(c));
        }
    }
}

/// Plans en parallaxe : collines et chapelles (20 %), arbres morts
/// (45 %), grille de fer (60 %), brume derrière le niveau.
pub fn layers(cv: &mut RgbaImage, sprites: &Sprites, cam: f32, step: u64) {
    hills(cv, cam);
    trees(cv, sprites, cam);
    fence(cv, cam);
    fog(cv, cam, step, 336..450, 0.10, theme::SPECTRE);
}

/// Collines lointaines et chapelles, fenêtres à la bougie.
fn hills(cv: &mut RgbaImage, cam: f32) {
    let ink = px(theme::lerp(theme::VOID, theme::SPECTRE_DARK, 0.42));
    let window = px(theme::CANDLE_DARK);
    let w = VIEW_W as usize;
    let raw: &mut [u8] = cv;
    for x in 0..w {
        let wx = x as f32 + cam * 0.2;
        let h = 378.0 + (wx * 0.0043).sin() * 27.0 + (wx * 0.0103 + 1.0).sin() * 15.0;
        let u = (wx as i64).rem_euclid(930);
        let roof = match u {
            360..=455 => h - 54.0 - ((18 - (u - 405).abs()).max(0) * 3) as f32,
            _ => h,
        };
        let lit = matches!(u, 378..=381 | 432..=435);
        for y in (roof.max(0.0) as usize)..VIEW_H as usize {
            let i = (y * w + x) * 4;
            let window_row = lit && (h - 30.0..h - 24.0).contains(&(y as f32));
            let c = if window_row { window } else { ink };
            raw[i..i + 4].copy_from_slice(&c.0);
        }
    }
}

/// Arbres morts du bestiaire en silhouettes.
fn trees(cv: &mut RgbaImage, sprites: &Sprites, cam: f32) {
    let ink = Ink {
        tint: Some(px(theme::lerp(theme::VOID, theme::SPECTRE_DARK, 0.25))),
        ..Ink::SOLID
    };
    let shift = cam * 0.45;
    let first = (shift / 160.0).floor() as i64 - 1;
    for k in first..first + 8 {
        let x = k as f32 * 160.0 + (hash(k, 7) % 90) as f32 - shift;
        let img = sprites.far_tree.get(if k % 2 == 0 { 1.0 } else { -1.0 });
        blit(cv, img, x as i64, 452 - img.height() as i64, ink);
    }
}

/// Grille de fer forgé.
fn fence(cv: &mut RgbaImage, cam: f32) {
    let ink = px(theme::lerp(theme::VOID, theme::SPECTRE_DARK, 0.18));
    let shift = (cam * 0.6) as i64;
    for x in 0..VIEW_W as i64 {
        let u = (x + shift).rem_euclid(9);
        for y in 400..452i64 {
            let rail = y == 412 || y == 440;
            let bar = u == 0 && y >= 404;
            let tip = u == 0 && y == 402 || (u == 8 || u == 1) && y == 405;
            if rail || bar || tip {
                cv.put_pixel(x as u32, y as u32, ink);
            }
        }
    }
}

/// Bande de brume sur `rows`, opacité ondulante autour de `alpha` ; les
/// sinus sont tabulés par colonne et par ligne (sin(a+b) développé).
pub fn fog(
    cv: &mut RgbaImage,
    cam: f32,
    step: u64,
    rows: std::ops::Range<u32>,
    alpha: f32,
    colour: Color,
) {
    let c = px(colour).0;
    let t = step as f32 * DT * 0.03;
    let w = VIEW_W as usize;
    let cols: Vec<(f32, f32, f32)> = (0..w)
        .map(|x| {
            let wx = x as f32 + cam * 0.7;
            let (a, b) = (wx * 0.012 + t, wx * 0.0037 - t * 0.6);
            (a.sin(), b.sin(), b.cos())
        })
        .collect();
    let raw: &mut [u8] = cv;
    for y in rows.filter(|y| *y < VIEW_H) {
        let (sy, cy) = (y as f32 * 0.07).sin_cos();
        let row = &mut raw[y as usize * w * 4..(y as usize + 1) * w * 4];
        for (px, (s1, sb, cb)) in row.chunks_exact_mut(4).zip(&cols) {
            let wave = s1 + sb * cy + cb * sy;
            let a = ((alpha * (1.0 + wave * 0.45)).clamp(0.0, 1.0) * 256.0) as i32;
            for i in 0..3 {
                let d = i32::from(px[i]);
                px[i] = (d + (((i32::from(c[i]) - d) * a) >> 8)) as u8;
            }
        }
    }
}

/// Éclaire le calque du niveau : ambiance sombre, halos des sources.
/// `factors` (réutilisé d'une frame à l'autre) reçoit l'éclairement.
pub fn light(layer: &mut RgbaImage, lights: &[Light], flash: bool, factors: &mut Vec<f32>) {
    let (w, h) = (VIEW_W as i64, VIEW_H as i64);
    factors.clear();
    factors.resize((w * h) as usize, if flash { 0.95 } else { AMBIENT });
    for l in lights {
        let (x0, x1) = (((l.x - l.r) as i64).max(0), ((l.x + l.r) as i64).min(w - 1));
        let (y0, y1) = (((l.y - l.r) as i64).max(0), ((l.y + l.r) as i64).min(h - 1));
        for y in y0..=y1 {
            let dy = y as f32 - l.y;
            for x in x0..=x1 {
                let dx = x as f32 - l.x;
                let d2 = (dx * dx + dy * dy) / (l.r * l.r);
                if d2 < 1.0 {
                    factors[(y * w + x) as usize] += l.k * (1.0 - d2);
                }
            }
        }
    }
    let raw: &mut [u8] = layer;
    for (px, f) in raw.chunks_exact_mut(4).zip(factors.iter()) {
        if px[3] == 0 {
            continue;
        }
        let k = f.min(1.15);
        for c in &mut px[..3] {
            *c = (f32::from(*c) * k).min(255.0) as u8;
        }
    }
}

/// Vignette : pour chaque ligne, la plage intacte et les facteurs ×256.
fn vignette_table() -> Vec<(u32, u32, Vec<u16>)> {
    let (hw, hh) = (VIEW_W as f32 / 2.0, VIEW_H as f32 / 2.0);
    (0..VIEW_H)
        .map(|y| {
            let v = (y as f32 - hh) / (hh + 40.0);
            let k: Vec<u16> = (0..VIEW_W)
                .map(|x| {
                    let u = (x as f32 - hw) / (hw + 30.0);
                    let d = u * u + v * v;
                    let f = if d <= 0.55 {
                        1.0
                    } else {
                        (1.0 - (d - 0.55) * 0.75).clamp(0.4, 1.0)
                    };
                    (f * 256.0) as u16
                })
                .collect();
            let lo = k
                .iter()
                .position(|f| *f >= 256)
                .unwrap_or(VIEW_W as usize / 2);
            let hi = k.iter().rposition(|f| *f >= 256).map_or(lo, |p| p + 1);
            (lo as u32, hi as u32, k)
        })
        .collect()
}
