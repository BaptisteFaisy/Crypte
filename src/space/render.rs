//! Composition de la scène 3D : sol, plaques de dossiers, piles de dalles,
//! fantôme principal. Voir `paint`, `slabs`, `actors` pour les détails.

use std::path::PathBuf;

use image::{Rgba, RgbaImage};

use super::actors::{kiro, main_ghost};
use super::cam::{CamGoal, Camera};
use super::collapse;
use super::falls;
use super::layout::{FileBox, Layout, SLAB_H, SLAB_PITCH};
use super::paint::{accent, ink, intensities, label};
use super::proj::{eye_dist, painter_order, project, world_to_px, CamParams, Vec3};
use super::raster;
use super::scan::ProjectTree;
use super::slabs::{ground_glow, ground_quad, slab};
use super::{FileState, SceneState};
use crate::canvas::px;
use crate::theme;

/// Brouillard : portée lointaine au repos, au travail, en vue hub (DESIGN §6.4).
const FOG_REST: f32 = 1500.0;
const FOG_WORK: f32 = 4000.0;
const FOG_HUB: f32 = 26000.0;
/// Pas de la grille de points au sol. Le design dit 28 ; ×4 pour tenir 30 fps.
const DOT_STEP: f32 = 112.0;
/// Demi-étendue du sol dessiné autour de la cible.
const FLOOR_EXTENT: f32 = 2600.0;

/// Assombrissement du panneau de chat (cuit dans l'image).
const PANEL_DIM: f32 = 0.55;

/// Le monde 3D : disposition + caméra. L'application en possède un.
pub struct Space {
    layout: Layout,
    cam: Camera,
    /// Chutes et impacts des sous-agents.
    falls: falls::Falls,
    /// `at` de la dernière vague de rewind déjà secouée.
    last_rewind: Option<f64>,
}

impl Space {
    /// Construit le monde d'un projet.
    pub fn new(tree: &ProjectTree) -> Self {
        let layout = Layout::build(tree);
        let cam = Camera::new(&layout);
        Self {
            layout,
            cam,
            falls: falls::Falls::default(),
            last_rewind: None,
        }
    }

    /// Disposition du monde (positions des dossiers et fichiers).
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Une image : avance la caméra puis peint tout le cadre.
    ///
    /// `panel` = rectangle du panneau de chat en pixels `[x, y, w, h]` : le
    /// voile semi-transparent y est cuit, car les cellules du terminal sont
    /// opaques (le texte ratatui vient par-dessus).
    pub fn frame(
        &mut self,
        cv: &mut RgbaImage,
        state: &SceneState,
        now: f64,
        dt: f32,
        panel: Option<[i64; 4]>,
    ) {
        let (w, h) = (cv.width(), cv.height());
        if let Some(big) = self.falls.sync(state, &self.layout, now) {
            self.cam.impact(big);
        }
        let goal = CamGoal::from_state(state, &self.layout);
        self.cam.step(goal, dt);
        if let Some(rw) = &state.rewind {
            if self.last_rewind != Some(rw.at) {
                self.last_rewind = Some(rw.at);
                self.cam.rewind_punch();
                self.cam.impact(true);
            }
        }
        let cam = self.cam.effective(now);
        let fog = fog_far(state, cam.dist);
        raster_clear(cv, px(theme::SPACE_BG));
        floor(cv, &cam, w, h, goal.target, fog);
        plates(cv, &cam, w, h, &self.layout, fog);
        files(cv, &cam, w, h, state, &self.layout, fog, now);
        main_ghost(cv, &cam, w, h, state, &self.layout, now);
        if let Some(k) = &state.kiro {
            kiro(cv, &cam, w, h, k, &self.layout, now, fog);
        }
        self.falls
            .draw(cv, &cam, w, h, state, &self.layout, now, fog);
        if let Some(p) = panel {
            raster::blur_rect(cv, p[0], p[1], p[2], p[3], 2);
            raster::dim_rect(cv, p[0], p[1], p[2], p[3], PANEL_DIM);
        }
        raster::vignette(cv, 0.55);
    }
}

/// Portée du brouillard selon l'activité (le near en est 12 %, DESIGN §6.4).
fn fog_far(state: &SceneState, dist: f32) -> f32 {
    let base = if state.hub {
        FOG_HUB
    } else if state.main_file.is_some() {
        FOG_WORK
    } else {
        FOG_REST
    };
    // Ne jamais descendre sous ~2,2× la distance de caméra, sinon le sol
    // disparaît entièrement dans le brouillard.
    base.max(dist * 2.2)
}

/// Visibilité d'un point après brouillard : 1 = net, 0 = fondu.
fn vis(cam: &CamParams, p: Vec3, far: f32) -> f32 {
    raster::fog(eye_dist(cam, p), far * 0.12, far)
}

/// Fond uniforme.
fn raster_clear(cv: &mut RgbaImage, c: Rgba<u8>) {
    for p in cv.pixels_mut() {
        *p = c;
    }
}

/// Grille de points au sol, centrée sur la cible de la caméra.
fn floor(cv: &mut RgbaImage, cam: &CamParams, w: u32, h: u32, target: Vec3, far: f32) {
    let n = (FLOOR_EXTENT / DOT_STEP) as i32;
    let cx = (target.x / DOT_STEP).round() * DOT_STEP;
    let cz = (target.z / DOT_STEP).round() * DOT_STEP;
    for iz in -n..=n {
        for ix in -n..=n {
            let p = Vec3::new(cx + ix as f32 * DOT_STEP, 0.0, cz + iz as f32 * DOT_STEP);
            let v = vis(cam, p, far);
            if v <= 0.02 {
                continue;
            }
            if let Some((sx, sy, d)) = project(cam, p, w, h) {
                let r = world_to_px(cam, 6.0, d, h);
                if r < 1.1 {
                    raster::over(cv, sx as i64, sy as i64, ink(0.9), 0.3 * v);
                } else {
                    raster::dot(cv, sx, sy, r.min(3.0), ink(0.9), 0.3 * v);
                }
            }
        }
    }
}

/// Plaques des dossiers : contour fin et chemin écrit au sol.
fn plates(cv: &mut RgbaImage, cam: &CamParams, w: u32, h: u32, layout: &Layout, far: f32) {
    for dir in &layout.dirs {
        let v = vis(cam, dir.origin, far);
        if v <= 0.03 {
            continue;
        }
        let Some(q) = ground_quad(cam, dir.origin, dir.w, dir.d, w, h) else {
            continue;
        };
        raster::quad_edges(cv, q, ink(0.5), 0.16 * v + 0.05);
        let name = dir.path.to_string_lossy().to_uppercase();
        label(
            cv,
            cam,
            dir.origin + Vec3::new(0.0, 1.0, dir.d + 18.0),
            &name,
            w,
            h,
            ink(0.6),
            v,
        );
    }
}

/// Tous les fichiers : piles de dalles, du plus loin au plus près.
fn files(
    cv: &mut RgbaImage,
    cam: &CamParams,
    w: u32,
    h: u32,
    state: &SceneState,
    layout: &Layout,
    far: f32,
    now: f64,
) {
    let mut items: Vec<(&PathBuf, &FileBox)> = layout.files().collect();
    painter_order(&mut items, |(_, b)| eye_dist(cam, b.base));
    for (path, b) in items {
        if layout.grouped_of(b.dir).is_some() {
            continue; // dossier trop dense : une seule pile, dessinée plus bas
        }
        let st = state.files.get(path);
        draw_file(cv, cam, w, h, b, st, state, now, far, path);
    }
    for (path, b) in grouped_piles(layout) {
        let st = grouped_state(state, layout, b.dir);
        draw_file(cv, cam, w, h, &b, st.as_ref(), state, now, far, &path);
    }
}

/// Piles regroupées des dossiers denses : une pile au centre de la plaque.
fn grouped_piles(layout: &Layout) -> Vec<(PathBuf, FileBox)> {
    layout
        .dirs
        .iter()
        .enumerate()
        .filter_map(|(i, d)| layout.grouped_of(i).map(|b| (d.path.clone(), *b)))
        .collect()
}

/// État cumulé des fichiers d'un dossier regroupé.
fn grouped_state(state: &SceneState, layout: &Layout, dir: usize) -> Option<FileState> {
    let mut acc: Option<FileState> = None;
    for (path, b) in layout.files() {
        if b.dir != dir {
            continue;
        }
        let Some(st) = state.files.get(path) else {
            continue;
        };
        let a = acc.get_or_insert_with(FileState::default);
        a.slabs += st.slabs;
        a.changed |= st.changed;
        a.read_at = latest(a.read_at, st.read_at);
        a.write_at = latest(a.write_at, st.write_at);
    }
    acc
}

/// Le plus récent de deux instants.
fn latest(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, y) => x.or(y),
    }
}

/// Dalles à dessiner : celles de l'état, ou celles de la vague de rewind en
/// train de s'effondrer (avec le décalage de la dalle qui tombe).
fn slabs_to_draw(
    state: &SceneState,
    path: &PathBuf,
    st: Option<&FileState>,
    now: f64,
) -> (u32, Option<f32>) {
    let base = st.map(|s| s.slabs).unwrap_or(1).clamp(1, 64);
    let Some(rw) = &state.rewind else {
        return (base, None);
    };
    if !collapse::active(rw, now) {
        return (base, None);
    }
    let Some(r) = collapse::rank(rw, path) else {
        return (base, None);
    };
    (
        collapse::visible_slabs(rw, r, now, base),
        collapse::falling_offset(rw, r, now),
    )
}

/// Une pile de dalles + son halo au sol + son nom.
fn draw_file(
    cv: &mut RgbaImage,
    cam: &CamParams,
    w: u32,
    h: u32,
    b: &FileBox,
    st: Option<&FileState>,
    state: &SceneState,
    now: f64,
    far: f32,
    path: &PathBuf,
) {
    let v = vis(cam, b.base, far);
    if v <= 0.03 {
        return;
    }
    let (read, active, changed) = intensities(st, now);
    ground_glow(cv, cam, w, h, b.base, read, active, changed, v);
    let (slabs, fall) = slabs_to_draw(state, path, st, now);
    for i in 0..slabs {
        let dy = if i + 1 == slabs {
            fall.unwrap_or(0.0)
        } else {
            0.0
        };
        slab(
            cv,
            cam,
            w,
            h,
            b.base + Vec3::new(0.0, dy, 0.0),
            i,
            read,
            active,
            changed,
            v,
        );
    }
    let top = b.base + Vec3::new(0.0, slabs as f32 * SLAB_PITCH + SLAB_H + 26.0, 0.0);
    let name = path.file_name().map(|n| n.to_string_lossy().to_string());
    if let Some(n) = name {
        if v > 0.45 && !state.hub {
            let c = if active > 0.02 || changed > 0.02 {
                accent(0.9)
            } else {
                ink(0.85)
            };
            label(cv, cam, top, &n.to_uppercase(), w, h, c, v);
        }
    }
}

#[cfg(test)]
mod tests;
