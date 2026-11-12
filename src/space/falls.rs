//! Sous-agents : vol vers leur cible, poussée dans le vide, chute, impact.
//!
//! Cycle (DESIGN §7) : un petit fantôme quitte le principal, vole vers son
//! fichier (1,2 s, 2,2 s pour le web) avec un faisceau orange ; à `done_at`
//! il est **poussé** (vx 260, vy 200, gravité 900, tumbling) ; à l'impact :
//! anneaux, flash, 26 poussières, secousse caméra ; puis il s'enfonce.

use std::collections::BTreeMap;

use image::RgbaImage;

use super::actors::ghost;
use super::layout::Layout;
use super::paint::{accent, ink};
use super::proj::{eye_dist, project, world_to_px, CamParams, Vec3};
use super::raster;
use super::{SceneState, Target};

/// Largeur d'un sous-agent (DESIGN §6.1).
const SUB_W: f32 = 40.0;
/// Durée du vol vers la cible (s) : 1,2 fichier, 2,2 web.
const FLY_FILE: f64 = 1.2;
const FLY_WEB: f64 = 2.2;
/// Arc du vol (hauteur maximale, unités).
const ARC: f32 = 140.0;
/// Poussée : vitesses et gravité (DESIGN §7).
const PUSH_X: f32 = 260.0;
const PUSH_UP: f32 = 200.0;
const GRAV: f32 = 900.0;
/// Enfoncement dans le sol après l'impact (s).
const SINK: f64 = 0.6;
/// Vitesse de tumbling (rad/s).
const TUMBLE: f32 = 5.0;
/// Poussières d'un impact (DESIGN §7).
const DUST: usize = 26;

/// Un sous-agent poussé dans le vide.
#[derive(Debug, Clone)]
pub struct Fall {
    pub from: Vec3,
    pub started: f64,
    pub dir: f32,
    impacted: bool,
}

impl Fall {
    /// Nouvelle chute depuis `from`, direction alternée par le seed.
    pub fn new(from: Vec3, started: f64, seed: u64) -> Self {
        Self {
            from,
            started,
            dir: if seed % 2 == 0 { 1.0 } else { -1.0 },
            impacted: false,
        }
    }

    /// Instant de l'impact (y repasse à 0).
    pub fn land(&self) -> f64 {
        let t = (PUSH_UP + (PUSH_UP * PUSH_UP + 2.0 * GRAV * self.from.y.max(0.0)).sqrt()) / GRAV;
        self.started + t as f64
    }

    /// Position et angle de tumbling ; `None` une fois enfoncé.
    pub fn pose(&self, now: f64) -> Option<(Vec3, f32)> {
        let t = (now - self.started) as f32;
        if t < 0.0 {
            return None;
        }
        let land = (self.land() - self.started) as f32;
        if t <= land {
            let y = self.from.y + PUSH_UP * t - 0.5 * GRAV * t * t;
            return Some((
                Vec3::new(self.from.x + self.dir * PUSH_X * t, y.max(0.0), self.from.z),
                t * TUMBLE * self.dir,
            ));
        }
        let s = (t - land) as f64 / SINK;
        if s >= 1.0 {
            return None;
        }
        let x = self.from.x + self.dir * PUSH_X * land;
        Some((
            Vec3::new(x, -60.0 * s as f32, self.from.z),
            land * TUMBLE * self.dir,
        ))
    }
}

/// Un impact au sol.
#[derive(Debug, Clone)]
pub struct Impact {
    pub at: f64,
    pub pos: Vec3,
    pub big: bool,
}

/// Le parc des chutes et impacts en cours.
#[derive(Debug, Clone, Default)]
pub struct Falls {
    falls: BTreeMap<u32, Fall>,
    impacts: Vec<Impact>,
}

impl Falls {
    /// Met à jour le parc depuis l'état ; rend `Some(big)` si un impact
    /// vient d'avoir lieu (pour la secousse caméra).
    pub fn sync(&mut self, state: &SceneState, layout: &Layout, now: f64) -> Option<bool> {
        let mut fresh: Option<bool> = None;
        for (id, s) in &state.subs {
            if s.done_at.is_none() || self.falls.contains_key(id) {
                continue;
            }
            let at = s.done_at.unwrap_or(now);
            let from = flight_pos(state, layout, *id, at).unwrap_or_else(|| layout.center());
            self.falls.insert(*id, Fall::new(from, at, u64::from(*id)));
        }
        for f in self.falls.values_mut() {
            if !f.impacted && now >= f.land() {
                let t = (f.land() - f.started) as f32;
                self.impacts.push(Impact {
                    at: f.land(),
                    pos: Vec3::new(f.from.x + f.dir * PUSH_X * t, 0.0, f.from.z),
                    big: false,
                });
                f.impacted = true;
                fresh = Some(false);
            }
        }
        self.falls
            .retain(|_, f| f.pose(now).is_some() || !f.impacted);
        self.impacts.retain(|i| now - i.at < 2.0);
        fresh
    }

    /// Dessine les sous-agents (vol ou chute), leurs faisceaux et impacts.
    pub fn draw(
        &self,
        cv: &mut RgbaImage,
        cam: &CamParams,
        w: u32,
        h: u32,
        state: &SceneState,
        layout: &Layout,
        now: f64,
        far: f32,
    ) {
        for (id, s) in &state.subs {
            if s.done_at.is_some() {
                continue;
            }
            if let Some(p) = flight_pos(state, layout, *id, now) {
                let v = raster::fog(eye_dist(cam, p), far * 0.12, far);
                ghost(cv, cam, w, h, p, SUB_W, now, v);
                if let Some(t) = target_pos(&s.target, layout) {
                    beam_to(cv, cam, w, h, p, t, v);
                }
            }
        }
        for f in self.falls.values() {
            if let Some((p, _ang)) = f.pose(now) {
                let v = raster::fog(eye_dist(cam, p), far * 0.12, far);
                ghost(cv, cam, w, h, p, SUB_W, now, v);
            }
        }
        for im in &self.impacts {
            draw_impact(cv, cam, w, h, im, now);
        }
    }
}

/// Position d'un sous-agent en vol à `now` (arc vers sa cible).
fn flight_pos(state: &SceneState, layout: &Layout, id: u32, now: f64) -> Option<Vec3> {
    let s = state.subs.get(&id)?;
    let dur = if s.kind == super::AgentKind::Web {
        FLY_WEB
    } else {
        FLY_FILE
    };
    let start = layout.center() + Vec3::new(0.0, 80.0, 0.0);
    let goal = target_pos(&s.target, layout).unwrap_or(start);
    let u = ((now - s.spawned_at) / dur).clamp(0.0, 1.0);
    let e = u * u * (3.0 - 2.0 * u);
    let arc = (std::f64::consts::PI * u).sin() as f32 * ARC;
    Some(lerp(start, goal, e as f32) + Vec3::new(0.0, arc, 0.0))
}

/// Position de la cible d'un sous-agent (fichier ou plateforme web).
fn target_pos(t: &Target, layout: &Layout) -> Option<Vec3> {
    match t {
        Target::Path(p) => layout.file(p).map(|b| b.base + Vec3::new(0.0, 40.0, 0.0)),
        Target::Web => Some(layout.center() + Vec3::new(-1500.0, 140.0, -800.0)),
    }
}

/// Interpolation linéaire.
fn lerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a + (b - a) * t
}

/// Faisceau orange du sous-agent vers le fichier qu'il construit.
fn beam_to(cv: &mut RgbaImage, cam: &CamParams, w: u32, h: u32, from: Vec3, to: Vec3, v: f32) {
    let (Some(a), Some(b)) = (project(cam, from, w, h), project(cam, to, w, h)) else {
        return;
    };
    raster::line(cv, a.0, a.1, b.0, b.1, accent(1.0), 0.5 * v);
}

/// Impact au sol : anneaux, flash, poussières (DESIGN §7).
fn draw_impact(cv: &mut RgbaImage, cam: &CamParams, w: u32, h: u32, im: &Impact, now: f64) {
    let dt = now - im.at;
    if dt < 0.0 || dt > 1.8 {
        return;
    }
    let Some((sx, sy, d)) = project(cam, im.pos, w, h) else {
        return;
    };
    let scale = |units: f32| world_to_px(cam, units, d, h).max(2.0);
    let size = if im.big { 420.0 } else { 260.0 };
    let u = (dt / 1.4).min(1.0);
    let e = 1.0 - (1.0 - u as f32).powi(2);
    raster::ring(
        cv,
        sx,
        sy,
        scale(size) * e,
        2.0,
        accent(1.0),
        (1.0 - u as f32) * 0.9,
    );
    let u2 = (dt - 0.12) / 1.6;
    if u2 > 0.0 {
        let e2 = 1.0 - (1.0 - u2.min(1.0) as f32).powi(2);
        raster::ring(
            cv,
            sx,
            sy,
            scale(size) * 1.5 * e2,
            1.0,
            ink(1.0),
            (1.0 - u2 as f32) * 0.45,
        );
    }
    if dt < 0.45 {
        raster::glow(
            cv,
            sx,
            sy,
            scale(size) * 0.6,
            accent(1.0),
            (1.0 - dt as f32 / 0.45) * 0.8,
        );
    }
    dust(cv, cam, w, h, im, dt as f32, scale(1.0));
}

/// 26 poussières projetées vers l'extérieur, estompées en 1,2 s.
fn dust(
    cv: &mut RgbaImage,
    cam: &CamParams,
    w: u32,
    h: u32,
    im: &Impact,
    dt: f32,
    px_per_unit: f32,
) {
    if dt > 1.2 {
        return;
    }
    let Some((sx, sy, _)) = project(cam, im.pos, w, h) else {
        return;
    };
    let a = (1.0 - dt / 1.2) * 0.9;
    for j in 0..DUST {
        let h1 = hash(j as i64, 11) as f32 / u64::MAX as f32;
        let h2 = hash(j as i64, 29) as f32 / u64::MAX as f32;
        let ang = h1 * std::f32::consts::TAU;
        let spd = (40.0 + h2 * 160.0) * px_per_unit;
        let x = sx + ang.cos() * spd * dt;
        let y = sy + ang.sin() * spd * dt * 0.45 - (60.0 * dt - 90.0 * dt * dt) * px_per_unit;
        raster::dot(cv, x, y, 1.4, ink(1.0), a);
    }
}

/// Hash entier déterministe (poussières, directions).
fn hash(x: i64, y: i64) -> u64 {
    let mut h = (x as u64) ^ (y as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^ (h >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::scan::{DirNode, FileNode, ProjectTree};
    use std::path::PathBuf;

    fn layout() -> Layout {
        Layout::build(&ProjectTree {
            root: PathBuf::from("/p"),
            dirs: vec![DirNode {
                path: PathBuf::from("src"),
                parent: None,
            }],
            files: vec![FileNode {
                path: PathBuf::from("src/a.rs"),
                dir: 0,
                lines: 300,
            }],
        })
    }

    fn state_with_sub(done: Option<f64>) -> SceneState {
        let mut s = SceneState::new();
        s.subs.insert(
            3,
            super::super::SubState {
                kind: super::super::AgentKind::Sub,
                target: Target::Path(PathBuf::from("src/a.rs")),
                spawned_at: 0.0,
                done_at: done,
                pushed: done.is_some(),
            },
        );
        s
    }

    #[test]
    fn la_chute_monte_puis_tombe_puis_s_enfonce() {
        let f = Fall::new(Vec3::new(0.0, 100.0, 0.0), 0.0, 1);
        let y0 = f.pose(0.0).unwrap().0.y;
        let y1 = f.pose(0.2).unwrap().0.y;
        assert!(y1 > y0, "poussé vers le haut d'abord");
        let land = f.land();
        assert!(f.pose(land - 0.01).unwrap().0.y >= 0.0);
        assert!(f.pose(land + 0.3).unwrap().0.y < 0.0, "s'enfonce");
        assert!(f.pose(land + SINK + 0.1).is_none(), "disparu");
    }

    #[test]
    fn l_impact_arrive_quand_la_chute_touche() {
        let mut falls = Falls::default();
        let l = layout();
        let s = state_with_sub(Some(1.0));
        assert_eq!(falls.sync(&s, &l, 1.0), None);
        assert!(falls.falls.contains_key(&3));
        let land = falls.falls[&3].land();
        let fresh = falls.sync(&s, &l, land + 0.01);
        assert_eq!(fresh, Some(false), "impact signalé");
        assert_eq!(falls.impacts.len(), 1);
        falls.sync(&s, &l, land + 3.0);
        assert!(falls.impacts.is_empty(), "impacts périmés");
    }

    #[test]
    fn le_vol_va_vers_la_cible() {
        let l = layout();
        let s = state_with_sub(None);
        let p0 = flight_pos(&s, &l, 3, 0.0).unwrap();
        let p1 = flight_pos(&s, &l, 3, FLY_FILE).unwrap();
        let goal = target_pos(&Target::Path(PathBuf::from("src/a.rs")), &l).unwrap();
        assert!((p1 - goal).len() < 1.0, "arrivé");
        assert!((p1 - p0).len() > 1.0, "a bougé");
    }

    #[test]
    fn l_impact_se_dessine_sans_paniquer() {
        let mut cv = RgbaImage::from_pixel(200, 120, image::Rgba([5, 5, 5, 255]));
        let cam = CamParams {
            target: Vec3::default(),
            dist: 800.0,
            yaw: 0.6,
            pitch: 0.4,
            fov: 42.0_f32.to_radians(),
        };
        let im = Impact {
            at: 0.0,
            pos: Vec3::default(),
            big: true,
        };
        draw_impact(&mut cv, &cam, 200, 120, &im, 0.5);
        assert!(cv.pixels().any(|p| p[0] > 40));
        draw_impact(&mut cv, &cam, 200, 120, &im, 5.0);
    }
}
