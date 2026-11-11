//! Dalles des fichiers : boîtes translucides, arêtes, halos au sol.

use image::{Rgba, RgbaImage};

use super::layout::{SLAB, SLAB_H, SLAB_PITCH};
use super::paint::{accent, ink};
use super::proj::{project, world_to_px, CamParams, Vec3};
use super::raster;
use crate::canvas::px;
use crate::theme;

/// Halo radial au sol sous un fichier (orange si actif/modifié, blanc si lu).
pub fn ground_glow(
    cv: &mut RgbaImage,
    cam: &CamParams,
    w: u32,
    h: u32,
    base: Vec3,
    read: f32,
    active: f32,
    changed: f32,
    v: f32,
) {
    let warm = 0.45 * active + 0.3 * changed;
    let cool = 0.28 * read;
    if warm < 0.01 && cool < 0.01 {
        return;
    }
    let Some((sx, sy, d)) = project(cam, base, w, h) else {
        return;
    };
    let r = world_to_px(cam, SLAB * 1.5, d, h).max(2.0);
    if warm > cool {
        raster::glow(cv, sx, sy, r, accent(1.0), warm * v);
    } else {
        raster::glow(cv, sx, sy, r, ink(1.0), cool * v);
    }
}

/// Une dalle : dessus éclairé, arêtes fines, montants verticaux.
pub fn slab(
    cv: &mut RgbaImage,
    cam: &CamParams,
    w: u32,
    h: u32,
    base: Vec3,
    i: u32,
    read: f32,
    active: f32,
    changed: f32,
    v: f32,
) {
    let y0 = i as f32 * SLAB_PITCH;
    let (Some(top), Some(bot)) = (
        box_face(cam, base, y0 + SLAB_H, w, h),
        box_face(cam, base, y0, w, h),
    ) else {
        return;
    };
    let edge_a = (0.3 + 0.55 * read + 0.7 * active + 0.25 * changed).min(1.0) * v;
    let warm = (active + 0.6 * changed).min(1.0);
    let edge = mix_edge(warm);
    raster::quad(
        cv,
        top,
        top_color(warm),
        (0.025 + 0.16 * read + 0.24 * active + 0.08 * changed) * v,
    );
    raster::quad_edges(cv, top, edge, edge_a);
    for k in 0..4 {
        raster::line(
            cv,
            top[k].0,
            top[k].1,
            bot[k].0,
            bot[k].1,
            edge,
            edge_a * 0.55,
        );
    }
}

/// Couleur des arêtes : blanc → orange selon l'activité (DESIGN §6.2).
pub fn mix_edge(warm: f32) -> Rgba<u8> {
    if warm <= 0.02 {
        ink(0.95)
    } else {
        px(theme::lerp(
            theme::SPACE_FG,
            theme::SPACE_ACCENT,
            warm.min(1.0),
        ))
    }
}

/// Couleur du dessus de la dalle : orange dès qu'elle est active ou modifiée.
pub fn top_color(warm: f32) -> Rgba<u8> {
    if warm > 0.02 {
        accent(1.0)
    } else {
        ink(0.95)
    }
}

/// Face supérieure d'une dalle (4 coins projetés, dans l'ordre).
pub fn box_face(cam: &CamParams, base: Vec3, y: f32, w: u32, h: u32) -> Option<[(f32, f32); 4]> {
    let s = SLAB / 2.0;
    let corners = [
        Vec3::new(base.x - s, y, base.z - s),
        Vec3::new(base.x + s, y, base.z - s),
        Vec3::new(base.x + s, y, base.z + s),
        Vec3::new(base.x - s, y, base.z + s),
    ];
    let mut out = [(0.0_f32, 0.0_f32); 4];
    for (i, c) in corners.iter().enumerate() {
        let (sx, sy, _) = project(cam, *c, w, h)?;
        out[i] = (sx, sy);
    }
    Some(out)
}

/// Rectangle au sol projeté (plaques de dossiers).
pub fn ground_quad(
    cam: &CamParams,
    origin: Vec3,
    qw: f32,
    qd: f32,
    w: u32,
    h: u32,
) -> Option<[(f32, f32); 4]> {
    let c = [
        origin,
        origin + Vec3::new(qw, 0.0, 0.0),
        origin + Vec3::new(qw, 0.0, qd),
        origin + Vec3::new(0.0, 0.0, qd),
    ];
    let mut out = [(0.0_f32, 0.0_f32); 4];
    for (i, p) in c.iter().enumerate() {
        let (sx, sy, _) = project(cam, *p, w, h)?;
        out[i] = (sx, sy);
    }
    Some(out)
}
