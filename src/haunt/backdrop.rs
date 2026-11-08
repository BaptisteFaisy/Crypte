//! Décor « 32 bits » derrière et devant le niveau, aux couleurs du thème :
//! ciel tramé (Bayer 4×4) du noir au violet sang, étoiles qui scintillent,
//! lune de sang, collines et chapelles en silhouette, arbres morts et
//! grille de fer en parallaxe, brume ; éclairage du niveau (Kiro,
//! citrouilles, lanternes, flammes), brume basse, vignette.

use image::{Rgba, RgbaImage};
use ratatui::style::Color;

use super::sprites::Sprites;
use super::{DT, SUB, VIEW_H, VIEW_W};
use crate::canvas::{blit, px, Ink};
use crate::theme;

/// Matrice de Bayer 4×4 (seuils de tramage, /16).
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
/// Bandes de couleur du ciel.
const BANDS: usize = 8;
/// Ligne d'horizon du dégradé (pixels).
const HORIZON: f32 = 150.0;
/// Lumière ambiante du niveau (le reste vient des sources).
const AMBIENT: f32 = 0.42;

/// Source de lumière (écran) : centre, rayon, intensité.
#[derive(Debug, Clone, Copy)]
pub struct Light {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub k: f32,
}

/// Ciel complet : dégradé tramé, étoiles, lune, collines, arbres, grille.
pub fn sky(cv: &mut RgbaImage, sprites: &Sprites, cam: f32, step: u64, flash: bool) {
    gradient(cv, flash);
    stars(cv, cam, step);
    moon(cv, cam);
    hills(cv, cam);
    trees(cv, sprites, cam);
    fence(cv, cam);
    fog(cv, cam, step, 112..150, 0.10, theme::SPECTRE);
}

/// Dégradé vertical en bandes tramées ; l'éclair le blanchit.
fn gradient(cv: &mut RgbaImage, flash: bool) {
    let stops = [
        (0.0, theme::VOID),
        (0.55, theme::lerp(theme::VOID, theme::SPECTRE_DARK, 0.6)),
        (1.0, theme::lerp(theme::SPECTRE_DARK, theme::DRIED, 0.55)),
    ];
    let bands: Vec<Rgba<u8>> = (0..BANDS)
        .map(|i| {
            let t = i as f32 / (BANDS - 1) as f32;
            let c = if t <= stops[1].0 {
                theme::lerp(stops[0].1, stops[1].1, t / stops[1].0)
            } else {
                theme::lerp(
                    stops[1].1,
                    stops[2].1,
                    (t - stops[1].0) / (1.0 - stops[1].0),
                )
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
        for x in 0..VIEW_W {
            let edge = f32::from(BAYER[(y % 4) as usize][(x % 4) as usize]) / 16.0;
            let band = if frac > edge {
                (lo + 1).min(BANDS - 1)
            } else {
                lo
            };
            cv.put_pixel(x, y, bands[band]);
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

/// Étoiles fixes (parallaxe 5 %), quelques-unes scintillent.
fn stars(cv: &mut RgbaImage, cam: f32, step: u64) {
    let shift = (cam * 0.05) as i64;
    for y in 12..110 {
        for x in 0..VIEW_W as i64 {
            let h = hash(x + shift, y);
            if h % 431 != 0 {
                continue;
            }
            let lit = (step / u64::from(6 * SUB) + h % 7) % 9 != 0;
            let c = if lit { theme::BONE } else { theme::ASH };
            cv.put_pixel(x as u32, y as u32, px(c));
        }
    }
}

/// Lune de sang : disque ombré, cratères, halo tramé (parallaxe 3 %).
fn moon(cv: &mut RgbaImage, cam: f32) {
    let (cx, cy, r) = (250.0 - cam * 0.03, 46.0, 19.0);
    for dy in -28..=28i64 {
        for dx in -28..=28i64 {
            let (x, y) = (cx as i64 + dx, cy as i64 + dy);
            if x < 0 || y < 0 || x >= VIEW_W as i64 || y >= VIEW_H as i64 {
                continue;
            }
            let d = ((dx * dx + dy * dy) as f32).sqrt();
            let c = if d <= r {
                let shade = ((dx + dy) as f32 / (2.0 * r) + 0.5).clamp(0.0, 1.0);
                let crater = hash(dx / 3, dy / 3) % 5 == 0;
                let base = theme::lerp(theme::BLOOD, theme::DRIED, shade * 0.8);
                if crater {
                    theme::lerp(base, theme::DRIED, 0.6)
                } else {
                    base
                }
            } else {
                let glow = 1.0 - (d - r) / 9.0;
                let edge = f32::from(BAYER[(y % 4) as usize][(x % 4) as usize]) / 16.0;
                if glow <= edge * 1.6 {
                    continue;
                }
                let under = cv.get_pixel(x as u32, y as u32).0;
                theme::lerp(Color::Rgb(under[0], under[1], under[2]), theme::DRIED, 0.35)
            };
            cv.put_pixel(x as u32, y as u32, px(c));
        }
    }
}

/// Collines lointaines et chapelles (parallaxe 20 %), fenêtres à la bougie.
fn hills(cv: &mut RgbaImage, cam: f32) {
    let ink = px(theme::lerp(theme::VOID, theme::SPECTRE_DARK, 0.42));
    let window = px(theme::CANDLE_DARK);
    for x in 0..VIEW_W as i64 {
        let wx = x as f32 + cam * 0.2;
        let h = 126.0 + (wx * 0.013).sin() * 9.0 + (wx * 0.031 + 1.0).sin() * 5.0;
        let u = (wx as i64).rem_euclid(310);
        let roof = match u {
            120..=151 => h - 18.0 - ((6 - (u - 135).abs()).max(0) * 3) as f32,
            _ => h,
        };
        for y in (roof.max(0.0) as u32)..VIEW_H {
            cv.put_pixel(x as u32, y, ink);
        }
        if u == 135 {
            for y in [roof - 4.0, roof - 3.0, roof - 2.0] {
                cv.put_pixel(x as u32, y.max(0.0) as u32, ink);
            }
        }
        if matches!(u, 126 | 127 | 144 | 145) {
            cv.put_pixel(x as u32, (h - 10.0) as u32, window);
            cv.put_pixel(x as u32, (h - 9.0) as u32, window);
        }
    }
}

/// Arbres morts (parallaxe 45 %) en silhouettes ×2.
fn trees(cv: &mut RgbaImage, sprites: &Sprites, cam: f32) {
    let ink = Ink {
        tint: Some(px(theme::lerp(theme::VOID, theme::SPECTRE_DARK, 0.25))),
        ..Ink::SOLID
    };
    let shift = cam * 0.45;
    let first = (shift / 150.0).floor() as i64 - 1;
    for k in first..first + 4 {
        let jitter = (hash(k, 7) % 70) as f32;
        let x = k as f32 * 150.0 + jitter - shift;
        let img = sprites.tree_big.get(if k % 2 == 0 { 1.0 } else { -1.0 });
        blit(cv, img, x as i64, 150 - img.height() as i64 + 6, ink);
    }
}

/// Grille de fer forgé (parallaxe 60 %).
fn fence(cv: &mut RgbaImage, cam: f32) {
    let ink = px(theme::lerp(theme::VOID, theme::SPECTRE_DARK, 0.18));
    let shift = (cam * 0.6) as i64;
    for x in 0..VIEW_W as i64 {
        let u = (x + shift).rem_euclid(7);
        for y in 128..148i64 {
            let rail = y == 133 || y == 143;
            let bar = u == 0 && y >= 130;
            let tip = u == 0 && y == 129 || (u == 6 || u == 1) && y == 130;
            if rail || bar || tip {
                cv.put_pixel(x as u32, y as u32, ink);
            }
        }
    }
}

/// Bande de brume : `rows` du canevas, opacité ondulante autour de `alpha`.
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
    for y in rows.filter(|y| *y < VIEW_H) {
        for x in 0..VIEW_W {
            let wx = x as f32 + cam * 0.7;
            let wave = (wx * 0.035 + t).sin() + (wx * 0.011 - t * 0.6 + y as f32 * 0.2).sin();
            let a = (alpha * (1.0 + wave * 0.45)).clamp(0.0, 1.0);
            let d = cv.get_pixel(x, y).0;
            let mix = |i: usize| (f32::from(d[i]) + (f32::from(c[i]) - f32::from(d[i])) * a) as u8;
            cv.put_pixel(x, y, Rgba([mix(0), mix(1), mix(2), 255]));
        }
    }
}

/// Éclaire le calque du niveau : ambiance sombre, halos des sources.
pub fn light(layer: &mut RgbaImage, lights: &[Light], flash: bool) {
    let ambient = if flash { 0.95 } else { AMBIENT };
    for (x, y, p) in layer.enumerate_pixels_mut() {
        if p.0[3] == 0 {
            continue;
        }
        let mut k = ambient;
        for l in lights {
            let (dx, dy) = (x as f32 - l.x, y as f32 - l.y);
            let d2 = (dx * dx + dy * dy) / (l.r * l.r);
            if d2 < 1.0 {
                k += l.k * (1.0 - d2);
            }
        }
        let k = k.min(1.15);
        for c in &mut p.0[..3] {
            *c = (f32::from(*c) * k).min(255.0) as u8;
        }
    }
}

/// Assombrit les bords (vignette, comme le shader Ghostty de la crypte).
pub fn vignette(cv: &mut RgbaImage) {
    let (hw, hh) = (VIEW_W as f32 / 2.0, VIEW_H as f32 / 2.0);
    for (x, y, p) in cv.enumerate_pixels_mut() {
        let (u, v) = ((x as f32 - hw) / (hw + 10.0), (y as f32 - hh) / (hh + 14.0));
        let d = u * u + v * v;
        if d <= 0.55 {
            continue;
        }
        let k = (1.0 - (d - 0.55) * 0.75).clamp(0.4, 1.0);
        for c in &mut p.0[..3] {
            *c = (f32::from(*c) * k) as u8;
        }
    }
}
