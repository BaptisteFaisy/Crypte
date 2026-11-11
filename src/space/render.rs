//! Composition de la scène 3D : sol, plaques de dossiers, piles de dalles,
//! fantôme principal. Voir `paint`, `slabs`, `actors` pour les détails.

use std::path::PathBuf;

use image::{Rgba, RgbaImage};

use super::actors::main_ghost;
use super::cam::{CamGoal, Camera};
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
}

impl Space {
    /// Construit le monde d'un projet.
    pub fn new(tree: &ProjectTree) -> Self {
        let layout = Layout::build(tree);
        let cam = Camera::new(&layout);
        Self { layout, cam }
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
        let goal = CamGoal::from_state(state, &self.layout);
        self.cam.step(goal, dt);
        let cam = self.cam.effective(now);
        let fog = fog_far(state, cam.dist);
        raster_clear(cv, px(theme::SPACE_BG));
        floor(cv, &cam, w, h, goal.target, fog);
        plates(cv, &cam, w, h, &self.layout, fog);
        files(cv, &cam, w, h, state, &self.layout, fog, now);
        main_ghost(cv, &cam, w, h, state, &self.layout, now);
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
        draw_file(cv, cam, w, h, b, st, now, far, path);
    }
    for (path, b) in grouped_piles(layout) {
        let st = grouped_state(state, layout, b.dir);
        draw_file(cv, cam, w, h, &b, st.as_ref(), now, far, &path);
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

/// Une pile de dalles + son halo au sol + son nom.
fn draw_file(
    cv: &mut RgbaImage,
    cam: &CamParams,
    w: u32,
    h: u32,
    b: &FileBox,
    st: Option<&FileState>,
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
    let slabs = st.map(|s| s.slabs).unwrap_or(1).clamp(1, 64);
    for i in 0..slabs {
        slab(cv, cam, w, h, b.base, i, read, active, changed, v);
    }
    let top = b.base + Vec3::new(0.0, slabs as f32 * SLAB_PITCH + SLAB_H + 26.0, 0.0);
    let name = path.file_name().map(|n| n.to_string_lossy().to_string());
    if let Some(n) = name {
        let c = if active > 0.02 || changed > 0.02 {
            accent(0.9)
        } else {
            ink(0.85)
        };
        label(cv, cam, top, &n.to_uppercase(), w, h, c, v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::paint::envelope;
    use crate::space::scan::{DirNode, FileNode};

    fn tree() -> ProjectTree {
        ProjectTree {
            root: PathBuf::from("/p"),
            dirs: vec![
                DirNode {
                    path: PathBuf::from("src"),
                    parent: None,
                },
                DirNode {
                    path: PathBuf::from("src/space"),
                    parent: Some(0),
                },
            ],
            files: vec![
                FileNode {
                    path: PathBuf::from("src/main.rs"),
                    dir: 0,
                    lines: 300,
                },
                FileNode {
                    path: PathBuf::from("src/space/mod.rs"),
                    dir: 1,
                    lines: 1200,
                },
            ],
        }
    }

    fn state(read: bool, write: bool) -> SceneState {
        let mut s = SceneState::new();
        s.open_project(std::path::Path::new("/p"), tree());
        let main = PathBuf::from("src/main.rs");
        if read {
            s.files.get_mut(&main).unwrap().read_at = Some(0.1);
            s.main_file = Some(main.clone());
        }
        if write {
            s.files.get_mut(&main).unwrap().write_at = Some(0.1);
            s.files.get_mut(&main).unwrap().changed = true;
            s.main_wrote = true;
            s.main_file = Some(main);
        }
        s
    }

    fn render(state: &SceneState, now: f64) -> RgbaImage {
        let mut cv = RgbaImage::from_pixel(320, 180, Rgba([0, 0, 0, 255]));
        let mut space = Space::new(&tree());
        for i in 0..40 {
            space.frame(
                &mut cv,
                state,
                now - (40 - i) as f64 / 30.0,
                1.0 / 30.0,
                None,
            );
        }
        cv
    }

    fn count(cv: &RgbaImage, f: impl Fn(&Rgba<u8>) -> bool) -> usize {
        cv.pixels().filter(|p| f(p)).count()
    }

    #[test]
    fn le_projet_actif_est_dessiné() {
        let cv = render(&state(false, false), 1.0);
        assert!(count(&cv, |p| p[0] > 20) > 150, "sol et dalles visibles");
    }

    #[test]
    fn une_écriture_allume_l_orange() {
        let cv = render(&state(false, true), 0.3);
        assert!(
            count(&cv, |p| p[0] > 120 && p[1] < 110 && p[2] < 80) > 40,
            "orange présent"
        );
    }

    #[test]
    fn une_lecture_allume_le_blanc_sans_orange() {
        let cv = render(&state(true, false), 0.3);
        assert!(count(&cv, |p| p[0] > 150 && p[1] > 150 && p[2] > 150) > 40);
    }

    #[test]
    fn le_rendu_est_déterministe() {
        let a = render(&state(true, true), 0.4);
        let b = render(&state(true, true), 0.4);
        assert_eq!(a.as_raw(), b.as_raw());
    }

    #[test]
    fn le_voile_du_panneau_assombrit() {
        let mut cv = RgbaImage::from_pixel(200, 120, Rgba([0, 0, 0, 255]));
        let mut space = Space::new(&tree());
        let s = state(true, false);
        space.frame(&mut cv, &s, 1.0, 1.0 / 30.0, None);
        let sans = cv.get_pixel(30, 60)[0];
        space.frame(&mut cv, &s, 1.0, 1.0 / 30.0, Some([0, 0, 80, 120]));
        assert!(cv.get_pixel(30, 60)[0] <= sans);
    }

    #[test]
    fn intensités_et_enveloppes() {
        assert_eq!(intensities(None, 1.0), (0.0, 0.0, 0.0));
        let mut f = FileState::default();
        f.read_at = Some(0.0);
        f.changed = true;
        let (r, _a, c) = intensities(Some(&f), 0.15);
        assert!(r > 0.4 && c == 1.0);
        assert_eq!(envelope(Some(0.0), 100.0), 0.0);
        assert_eq!(envelope(None, 1.0), 0.0);
    }

    /// Planche de contrôle visuelle (ignorée en CI) :
    /// `cargo test --release -- --ignored space_preview --nocapture`
    /// puis ouvrir `/tmp/space-preview.png`.
    #[test]
    #[ignore]
    fn space_preview() {
        let tree = ProjectTree {
            root: PathBuf::from("/p"),
            dirs: (0..4)
                .map(|i| DirNode {
                    path: PathBuf::from(format!("src/mod{i}")),
                    parent: (i > 0).then_some(0),
                })
                .collect(),
            files: (0..24)
                .map(|i| FileNode {
                    path: PathBuf::from(format!("src/mod{}/f{i:02}.rs", i % 4)),
                    dir: i % 4,
                    lines: (40 + (i * 137) % 900) as u32,
                })
                .collect(),
        };
        let mut s = SceneState::new();
        s.open_project(std::path::Path::new("/p"), tree.clone());
        let hot = PathBuf::from("src/mod1/f05.rs");
        s.files.get_mut(&hot).unwrap().write_at = Some(3.9);
        s.files.get_mut(&hot).unwrap().changed = true;
        s.files
            .get_mut(&PathBuf::from("src/mod0/f04.rs"))
            .unwrap()
            .read_at = Some(3.8);
        s.main_file = Some(hot);
        s.main_wrote = true;
        s.step = Some(super::super::Step::Act);
        let mut cv = RgbaImage::from_pixel(960, 540, Rgba([0, 0, 0, 255]));
        let mut space = Space::new(&tree);
        for i in 0..150 {
            space.frame(
                &mut cv,
                &s,
                i as f64 / 30.0,
                1.0 / 30.0,
                Some([32, 55, 347, 457]),
            );
        }
        cv.save("/tmp/space-preview.png").expect("png");
        println!("/tmp/space-preview.png écrit (960×540)");
    }

    #[test]
    fn brouillard_selon_l_activité() {
        assert_eq!(fog_far(&SceneState::new(), 100.0), FOG_REST);
        let mut s = SceneState::new();
        s.hub = true;
        assert_eq!(fog_far(&s, 100.0), FOG_HUB);
        let s2 = state(true, false);
        assert_eq!(fog_far(&s2, 100.0), FOG_WORK);
        assert!(fog_far(&SceneState::new(), 4000.0) > FOG_REST);
    }
}
