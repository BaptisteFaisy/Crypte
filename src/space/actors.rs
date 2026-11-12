//! Fantômes et faisceaux : le fantôme principal vole vers les fichiers.

use std::path::PathBuf;

use image::{Rgba, RgbaImage};

use super::layout::{Layout, SLAB, SLAB_H, SLAB_PITCH};
use super::paint::{accent, ink};
use super::proj::{eye_dist, project, world_to_px, CamParams, Vec3};
use super::raster;
use super::{KiroState, SceneState};
use crate::canvas::px;
use crate::theme;

/// Le fantôme flotte 46 unités au-dessus du fichier (DESIGN §6.1).
const HOVER: f32 = 46.0;
/// Largeur du fantôme principal (DESIGN §6.1).
const GHOST_W: f32 = 60.0;
/// Amplitude et vitesse du flottement (DESIGN §6.3 : 5 unités, 2,6 rad/s).
const BOB: f32 = 5.0;
const BOB_W: f64 = 2.6;
/// Hauteur du perchoir de Kiro au-dessus du sol.
const PERCH_Y: f32 = 90.0;
/// Largeur de Kiro (DESIGN §6.1).
const KIRO_W: f32 = 36.0;
/// Poussée de Kiro dans le vide (comme un sous-agent, DESIGN §7).
const PUSH_X: f32 = 260.0;
const PUSH_UP: f32 = 200.0;

/// Kiro sur son perchoir près du fichier jugé (DESIGN §6.1 : fantôme 36,
/// deux cornes orange, dalle-perchoir 90) ; poussé dans le vide à la fin.
pub fn kiro(
    cv: &mut RgbaImage,
    cam: &CamParams,
    w: u32,
    h: u32,
    k: &KiroState,
    layout: &Layout,
    now: f64,
    far: f32,
) {
    let base = perch_base(k, layout);
    let v = raster::fog(eye_dist(cam, base), far * 0.12, far);
    if v <= 0.03 {
        return;
    }
    let pushed = k.done_at.map(|d| now - d).filter(|t| *t >= 0.0);
    match pushed {
        None => {
            perch_slab(cv, cam, w, h, base, v);
            let y = PERCH_Y + (now * BOB_W).sin() as f32 * BOB;
            kiro_body(cv, cam, w, h, base + Vec3::new(0.0, y, 0.0), v);
        }
        Some(t) => {
            let y = PERCH_Y + PUSH_UP * t as f32 - 450.0 * (t as f32) * (t as f32);
            if y < -150.0 {
                return;
            }
            let x = base.x + PUSH_X * t as f32;
            kiro_body(cv, cam, w, h, Vec3::new(x, y.max(-40.0), base.z), v);
        }
    }
}

/// Position du perchoir : à côté du fichier jugé, sinon au centre.
fn perch_base(k: &KiroState, layout: &Layout) -> Vec3 {
    k.file
        .as_ref()
        .and_then(|f| layout.file(f))
        .map(|b| b.base + Vec3::new(SLAB * 1.3, 0.0, 0.0))
        .unwrap_or_else(|| layout.center())
}

/// La dalle-perchoir sous Kiro.
fn perch_slab(cv: &mut RgbaImage, cam: &CamParams, w: u32, h: u32, base: Vec3, v: f32) {
    let Some(q) = crate::space::slabs::box_face(cam, base, PERCH_Y - 20.0, w, h) else {
        return;
    };
    raster::quad(cv, q, ink(0.9), 0.05 * v);
    raster::quad_edges(cv, q, ink(0.9), 0.3 * v);
}

/// Kiro : petit fantôme + deux cornes orange.
fn kiro_body(cv: &mut RgbaImage, cam: &CamParams, w: u32, h: u32, pos: Vec3, v: f32) {
    let Some((sx, sy, d)) = project(cam, pos, w, h) else {
        return;
    };
    let r = world_to_px(cam, KIRO_W, d, h).max(3.0);
    ghost(cv, cam, w, h, pos, KIRO_W);
    for s in [-1.0_f32, 1.0] {
        raster::line(
            cv,
            sx + s * r * 0.22,
            sy - r * 0.5,
            sx + s * r * 0.42,
            sy - r * 0.95,
            accent(1.0),
            0.9 * v,
        );
    }
}

/// Le fantôme principal au-dessus du fichier qu'il travaille, et son faisceau.
pub fn main_ghost(
    cv: &mut RgbaImage,
    cam: &CamParams,
    w: u32,
    h: u32,
    state: &SceneState,
    layout: &Layout,
    now: f64,
) {
    let Some(path) = &state.main_file else {
        return;
    };
    let Some(b) = layout.file(path) else {
        return;
    };
    let top_y = file_top_y(state, path);
    let bob = (now * BOB_W).sin() as f32 * BOB;
    let pos = b.base + Vec3::new(0.0, top_y + HOVER + bob, 0.0);
    ghost(cv, cam, w, h, pos, GHOST_W);
    let target = b.base + Vec3::new(0.0, top_y, 0.0);
    let (c, a) = if state.main_wrote {
        (accent(1.0), 0.85)
    } else {
        (ink(1.0), 0.55)
    };
    beam(cv, cam, w, h, pos, target, c, a);
    if let Some((tx, ty, td)) = project(cam, target, w, h) {
        let r = world_to_px(cam, 70.0, td, h).max(3.0);
        raster::glow(cv, tx, ty, r, ink(1.0), 0.2);
    }
}

/// Hauteur du sommet de la pile d'un fichier.
pub fn file_top_y(state: &SceneState, path: &PathBuf) -> f32 {
    state
        .files
        .get(path)
        .map(|s| s.slabs as f32 * SLAB_PITCH + SLAB_H)
        .unwrap_or(SLAB_H)
}

/// Silhouette de fantôme : dôme lumineux, corps, deux yeux.
pub fn ghost(cv: &mut RgbaImage, cam: &CamParams, w: u32, h: u32, pos: Vec3, size: f32) {
    let Some((sx, sy, d)) = project(cam, pos, w, h) else {
        return;
    };
    let r = world_to_px(cam, size, d, h).max(3.0);
    raster::glow(cv, sx, sy - r * 0.25, r * 0.9, ink(1.0), 0.85);
    raster::quad(
        cv,
        [
            (sx - r * 0.42, sy - r * 0.1),
            (sx + r * 0.42, sy - r * 0.1),
            (sx + r * 0.34, sy + r * 0.62),
            (sx - r * 0.34, sy + r * 0.62),
        ],
        ink(1.0),
        0.95,
    );
    for s in [-1.0_f32, 1.0] {
        raster::dot(
            cv,
            sx + s * r * 0.16,
            sy - r * 0.18,
            (r * 0.09).max(0.7),
            px(theme::SPACE_BG),
            1.0,
        );
    }
}

/// Faisceau du fantôme vers le fichier (blanc en lecture, orange en écriture).
pub fn beam(
    cv: &mut RgbaImage,
    cam: &CamParams,
    w: u32,
    h: u32,
    from: Vec3,
    to: Vec3,
    c: Rgba<u8>,
    a: f32,
) {
    let (Some(f), Some(t)) = (project(cam, from, w, h), project(cam, to, w, h)) else {
        return;
    };
    raster::line(cv, f.0, f.1, t.0, t.1, c, a);
}
