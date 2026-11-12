//! Composition de la scène 3D : sol, plaques de dossiers, piles de dalles,
//! fantôme principal. Voir `paint`, `slabs`, `actors` pour les détails.

use std::path::PathBuf;

use image::{Rgba, RgbaImage};

use super::actors::{kiro, main_ghost};
use super::cam::{CamGoal, Camera};
use super::collapse;
use super::falls;
use super::layout::{FileBox, Layout, FOLDER_PAD, SLAB_H, SLAB_PITCH};
use super::paint::{accent, ink, intensities};
use super::proj::{eye_dist, painter_order, project, world_to_px, CamParams, Vec3};
use super::raster;
use super::scan::ProjectTree;
use super::slabs::{ground_glow, ground_quad, slab};
use super::{hud, post};
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
        Self::new_many(&[(String::new(), tree.clone(), true)])
    }

    /// Construit le monde de plusieurs projets (quartiers).
    pub fn new_many(entries: &[(String, ProjectTree, bool)]) -> Self {
        let layout = Layout::build_many(entries);
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
        let mut labels = Labels::default();
        districts(cv, &cam, w, h, &self.layout, fog, &mut labels);
        plates(cv, &cam, w, h, &self.layout, fog, &mut labels);
        files(cv, &cam, w, h, state, &self.layout, fog, now, &mut labels);
        labels.draw(cv);
        main_ghost(cv, &cam, w, h, state, &self.layout, now, fog);
        if let Some(k) = &state.kiro {
            kiro(cv, &cam, w, h, k, &self.layout, now, fog);
        }
        self.falls
            .draw(cv, &cam, w, h, state, &self.layout, now, fog);
        post::pillars(cv, &cam, w, h, 7);
        post::ambient_dust(cv, &cam, w, h, 11);
        post::bloom(cv, 0.7, 0.62);
        raster::vignette(cv, 0.55);
        if let Some(p) = panel {
            panel_veil(cv, p);
        }
        hud::draw(cv, state, now);
    }
}

/// Le panneau de chat posé sur la 3D (DESIGN §4) : flou du fond, dégradé
/// blanc 3 %→0,75 %, bordure 1 px, coins orange 9 px. Tout est cuit dans
/// l'image, car les cellules du terminal sont opaques.
fn panel_veil(cv: &mut RgbaImage, p: [i64; 4]) {
    raster::blur_rect(cv, p[0], p[1], p[2], p[3], 3);
    raster::dim_rect(cv, p[0], p[1], p[2], p[3], PANEL_DIM);
    let (w, h) = (p[2], p[3]);
    if w < 8 || h < 8 {
        return;
    }
    // Dégradé vertical très léger (3 % en haut, 0,75 % en bas).
    for y in 0..h {
        let t = y as f32 / h as f32;
        let a = 0.03 * (1.0 - t) + 0.0075 * t;
        for x in 0..w {
            raster::over(cv, p[0] + x, p[1] + y, ink(1.0), a);
        }
    }
    // Bordure 1 px (13 % de blanc).
    let line = ink(0.13);
    for x in 0..w {
        raster::over(cv, p[0] + x, p[1], line, 1.0);
        raster::over(cv, p[0] + x, p[1] + h - 1, line, 1.0);
    }
    for y in 0..h {
        raster::over(cv, p[0], p[1] + y, line, 1.0);
        raster::over(cv, p[0] + w - 1, p[1] + y, line, 1.0);
    }
    // Coins orange (L 9 px) : haut-gauche et bas-droite.
    let orange = accent(1.0);
    for i in 0..9 {
        raster::over(cv, p[0] + i, p[1], orange, 0.9);
        raster::over(cv, p[0], p[1] + i, orange, 0.9);
        raster::over(cv, p[0] + w - 1 - i, p[1] + h - 1, orange, 0.9);
        raster::over(cv, p[0] + w - 1, p[1] + h - 1 - i, orange, 0.9);
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

/// Plafond d'étiquettes par frame (au-delà, les moins prioritaires sautent).
const MAX_LABELS: usize = 90;

/// Une étiquette candidate : position écran, texte, couleur, priorité.
#[derive(Clone)]
struct LabelCand {
    x: i64,
    y: i64,
    w: i64,
    text: String,
    color: Rgba<u8>,
    prio: f32,
}

/// Collecte les étiquettes d'une frame puis les pose sans chevauchement, les
/// plus prioritaires d'abord (actif > modifié > lu > proche > lointain).
#[derive(Default)]
struct Labels {
    items: Vec<LabelCand>,
}

impl Labels {
    /// Ajoute une étiquette centrée sur le point monde `at`.
    fn push(
        &mut self,
        cam: &CamParams,
        w: u32,
        h: u32,
        at: Vec3,
        text: &str,
        color: Rgba<u8>,
        prio: f32,
    ) {
        let Some((sx, sy, _)) = project(cam, at, w, h) else {
            return;
        };
        let width = crate::hub::font::width(text);
        self.items.push(LabelCand {
            x: sx as i64 - width / 2,
            y: sy as i64,
            w: width,
            text: text.to_string(),
            color,
            prio,
        });
    }

    /// Pose les étiquettes ; celles qui chevauchent une déjà posée sautent.
    fn draw(&self, cv: &mut RgbaImage) {
        let mut items = self.items.clone();
        items.sort_by(|a, b| {
            b.prio
                .partial_cmp(&a.prio)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut placed: Vec<(i64, i64, i64, i64)> = Vec::new();
        for it in items.iter().take(MAX_LABELS) {
            let r = (it.x, it.y, it.x + it.w, it.y + 5);
            if placed.iter().any(|p| hits(p, &r)) {
                continue;
            }
            placed.push(r);
            let c = it.color;
            crate::hub::font::paint(cv, it.x, it.y, &it.text, 1, |_| c);
        }
    }
}

/// Deux rectangles écran se chevauchent-ils ?
fn hits(a: &(i64, i64, i64, i64), b: &(i64, i64, i64, i64)) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
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

/// Quartiers : la plaque extérieure de chaque projet ; l'actif en orange
/// pointillé avec « YOU ARE HERE » (DESIGN §6.1).
fn districts(
    cv: &mut RgbaImage,
    cam: &CamParams,
    w: u32,
    h: u32,
    layout: &Layout,
    far: f32,
    labels: &mut Labels,
) {
    for p in &layout.projects {
        let v = vis(cam, p.origin, far);
        if v <= 0.02 {
            continue;
        }
        let pad = FOLDER_PAD * 2.0;
        let Some(q) = ground_quad(
            cam,
            p.origin - Vec3::new(FOLDER_PAD, 0.0, FOLDER_PAD),
            p.w + pad,
            p.d + pad,
            w,
            h,
        ) else {
            continue;
        };
        if p.active {
            dashed_quad(cv, q, accent(0.9), 0.8 * v);
        } else {
            raster::quad_edges(cv, q, ink(0.5), 0.3 * v);
        }
        let mid = p.origin + Vec3::new(p.w / 2.0, 2.0, 60.0);
        let c = if p.active {
            accent(0.9 * v)
        } else {
            ink(0.7 * v)
        };
        labels.push(cam, w, h, mid, &p.name.to_uppercase(), c, 40.0);
        if p.active {
            labels.push(
                cam,
                w,
                h,
                mid + Vec3::new(0.0, 0.0, 60.0),
                "YOU ARE HERE",
                accent(0.8 * v),
                39.0,
            );
        }
    }
}

/// Contour pointillé d'un quadrilatère (tirets ~6 px, trous ~6 px).
fn dashed_quad(cv: &mut RgbaImage, q: [(f32, f32); 4], c: Rgba<u8>, a: f32) {
    for i in 0..4 {
        let (x0, y0) = q[i];
        let (x1, y1) = q[(i + 1) % 4];
        let len = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
        let steps = (len / 12.0).max(2.0) as usize;
        for k in (0..steps).step_by(2) {
            let t0 = k as f32 / steps as f32;
            let t1 = ((k + 1) as f32 / steps as f32).min(1.0);
            raster::line(
                cv,
                x0 + (x1 - x0) * t0,
                y0 + (y1 - y0) * t0,
                x0 + (x1 - x0) * t1,
                y0 + (y1 - y0) * t1,
                c,
                a,
            );
        }
    }
}

/// Plaques des dossiers : contour fin et chemin écrit au sol.
fn plates(
    cv: &mut RgbaImage,
    cam: &CamParams,
    w: u32,
    h: u32,
    layout: &Layout,
    far: f32,
    labels: &mut Labels,
) {
    for dir in &layout.dirs {
        let v = vis(cam, dir.origin, far);
        if v <= 0.03 {
            continue;
        }
        let Some(q) = ground_quad(cam, dir.origin, dir.w, dir.d, w, h) else {
            continue;
        };
        raster::quad_edges(cv, q, ink(0.5), 0.16 * v + 0.05);
        if v > 0.2 {
            let name = dir.path.to_string_lossy().to_uppercase();
            labels.push(
                cam,
                w,
                h,
                dir.origin + Vec3::new(0.0, 1.0, dir.d + 18.0),
                &name,
                ink(0.6 * v),
                20.0 + v,
            );
        }
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
    labels: &mut Labels,
) {
    let mut items: Vec<(&PathBuf, &FileBox)> = layout.files().collect();
    painter_order(&mut items, |(_, b)| eye_dist(cam, b.base));
    for (path, b) in items {
        if layout.grouped_of(b.dir).is_some() {
            continue; // dossier trop dense : une seule pile, dessinée plus bas
        }
        let st = state.files.get(path);
        draw_file(cv, cam, w, h, b, st, state, now, far, path, labels);
    }
    for (path, b) in grouped_piles(layout) {
        let st = grouped_state(state, layout, b.dir);
        draw_file(
            cv,
            cam,
            w,
            h,
            &b,
            st.as_ref(),
            state,
            now,
            far,
            &path,
            labels,
        );
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
    labels: &mut Labels,
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
    if v > 0.35 && !state.hub {
        if let Some(n) = path.file_name().map(|n| n.to_string_lossy().to_uppercase()) {
            let warm = active > 0.02 || changed > 0.02;
            let c = if warm { accent(0.9 * v) } else { ink(0.85 * v) };
            let prio = 10.0 * active + 8.0 * changed + 5.0 * read + 2.0 * v;
            labels.push(cam, w, h, top, &n, c, prio);
        }
    }
}

#[cfg(test)]
mod tests;
