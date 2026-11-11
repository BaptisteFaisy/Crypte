//! Couleurs et étiquettes du monde 3D (via `theme.rs`).

use image::{Rgba, RgbaImage};

use super::proj::{project, CamParams, Vec3};
use super::FileState;
use crate::canvas::px;
use crate::hub::font;
use crate::theme;

/// Enveloppe d'un fichier lu : 0,3 s de montée, 0,6 s de descente (DESIGN §7).
pub const FADE_IN: f64 = 0.3;
pub const FADE_OUT: f64 = 0.6;

/// Blanc atténué vers le fond (texte et lignes secondaires).
pub fn ink(t: f32) -> Rgba<u8> {
    px(theme::lerp(
        theme::SPACE_BG,
        theme::SPACE_FG,
        t.clamp(0.0, 1.0),
    ))
}

/// Orange atténué vers le fond.
pub fn accent(t: f32) -> Rgba<u8> {
    px(theme::lerp(
        theme::SPACE_BG,
        theme::SPACE_ACCENT,
        t.clamp(0.0, 1.0),
    ))
}

/// Étiquettes au sol (police pixel 3×5 du hub, réutilisée).
pub fn label(
    cv: &mut RgbaImage,
    cam: &CamParams,
    at: Vec3,
    text: &str,
    w: u32,
    h: u32,
    c: Rgba<u8>,
    v: f32,
) {
    let Some((sx, sy, _)) = project(cam, at, w, h) else {
        return;
    };
    let x0 = sx as i64 - (font::width(text) / 2);
    let col = Rgba([c[0], c[1], c[2], 255]);
    if v > 0.15 {
        font::paint(cv, x0, sy as i64, text, 1, |_| col);
    }
}

/// Intensités 0..1 d'un fichier : lecture, écriture en cours, modification.
pub fn intensities(st: Option<&FileState>, now: f64) -> (f32, f32, f32) {
    let Some(st) = st else {
        return (0.0, 0.0, 0.0);
    };
    (
        envelope(st.read_at, now),
        envelope(st.write_at, now),
        if st.changed { 1.0 } else { 0.0 },
    )
}

/// Enveloppe d'un événement daté : montée [`FADE_IN`], descente [`FADE_OUT`].
pub fn envelope(at: Option<f64>, now: f64) -> f32 {
    let Some(at) = at else { return 0.0 };
    let dt = now - at;
    if dt < 0.0 {
        return 0.0;
    }
    if dt < FADE_IN {
        (dt / FADE_IN) as f32
    } else {
        (1.0 - (dt - FADE_IN) / FADE_OUT).max(0.0) as f32
    }
}
