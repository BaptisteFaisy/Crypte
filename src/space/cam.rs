//! Caméra orbitale (DESIGN §8) : cible, distance, yaw, pitch, fov.
//!
//! La caméra ne connaît que des buts ([`CamGoal`]) et s'en approche en
//! douceur ; le rendu lit [`Camera::effective`] pour projeter.

use super::layout::Layout;
use super::proj::{CamParams, Vec3};
use super::SceneState;

/// Champ vertical par défaut : 42° (DESIGN §8).
pub const FOV: f32 = 42.0_f32.to_radians();

/// Plan rapproché sur l'agent au travail (600–950, 10–24°).
pub const CLOSE: (f32, f32) = (780.0, 17.0_f32.to_radians());
/// Vue d'ensemble du projet (1 500–2 200, 22–32°).
pub const OVERVIEW: (f32, f32) = (1850.0, 27.0_f32.to_radians());
/// Vue hub : tous les projets (9 000+, 38°).
pub const HUB: (f32, f32) = (10_500.0, 38.0_f32.to_radians());
/// Entrée du projet, au démarrage : ras du sol (pitch 3°, distance 300).
pub const ENTRANCE: (f32, f32) = (300.0, 3.0_f32.to_radians());

/// Lacet fixe : trois-quarts, comme le prototype.
pub const YAW: f32 = 0.62;
/// Vitesse de rapprochement du but (par seconde).
const SMOOTH: f32 = 3.2;
/// Amortissement du shake (0.9 s dans DESIGN §7).
const SHAKE_DECAY: f32 = 0.9;
/// Punch de fov au `/rewind` (+8°).
const FOV_PUNCH: f32 = 8.0_f32.to_radians();

/// Ce que la caméra vise, à quelle distance et sous quel angle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamGoal {
    pub target: Vec3,
    pub dist: f32,
    pub pitch: f32,
    pub yaw: f32,
}

impl CamGoal {
    /// But courant, déduit de l'état de la scène.
    pub fn from_state(state: &SceneState, layout: &Layout) -> Self {
        if state.hub {
            return Self::shot(HUB, Vec3::default());
        }
        if let Some(path) = &state.main_file {
            if let Some(b) = layout.file(path) {
                let target = b.base + Vec3::new(0.0, 60.0, 0.0);
                return Self::shot(CLOSE, target);
            }
        }
        Self::shot(OVERVIEW, layout.center())
    }

    /// But d'entrée de projet (caméra au ras du sol).
    pub fn entrance(layout: &Layout) -> Self {
        Self::shot(ENTRANCE, layout.center())
    }

    fn shot((dist, pitch): (f32, f32), target: Vec3) -> Self {
        Self {
            target,
            dist,
            pitch,
            yaw: YAW,
        }
    }
}

/// Caméra : état lissé + secousses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    goal: CamGoal,
    /// Amplitude restante du shake (unités monde).
    shake: f32,
    /// Punch de fov restant (radians).
    punch: f32,
}

impl Camera {
    /// Caméra à l'entrée du projet.
    pub fn new(layout: &Layout) -> Self {
        Self {
            goal: CamGoal::entrance(layout),
            shake: 0.0,
            punch: 0.0,
        }
    }

    /// Avance d'un pas : rapproche le but courant et amortit les effets.
    pub fn step(&mut self, goal: CamGoal, dt: f32) {
        let k = (SMOOTH * dt.max(0.0)).min(1.0);
        self.goal.target = lerp_vec(self.goal.target, goal.target, k);
        self.goal.dist += (goal.dist - self.goal.dist) * k;
        self.goal.pitch += (goal.pitch - self.goal.pitch) * k;
        self.goal.yaw += (goal.yaw - self.goal.yaw) * k;
        self.shake = decay(self.shake, dt);
        self.punch = decay(self.punch, dt);
    }

    /// Impact au sol : secousse de 4 (agent) ou 12 (gros impact).
    pub fn impact(&mut self, big: bool) {
        self.shake = if big { 12.0 } else { 4.0 };
    }

    /// `/rewind` : le fov prend +8°.
    pub fn rewind_punch(&mut self) {
        self.punch = FOV_PUNCH;
    }

    /// Paramètres à utiliser pour projeter cette frame.
    pub fn effective(&self, now: f64) -> CamParams {
        CamParams {
            target: self.goal.target + shake_offset(self.shake, now),
            dist: self.goal.dist,
            yaw: self.goal.yaw,
            pitch: self.goal.pitch,
            fov: FOV + self.punch,
        }
    }

    /// But courant (tests, débogage).
    pub fn goal(&self) -> CamGoal {
        self.goal
    }
}

/// Décroissance exponentielle bornée par le pas de temps.
fn decay(v: f32, dt: f32) -> f32 {
    let t = (1.0 - (dt.max(0.0) / SHAKE_DECAY)).max(0.0);
    if v * t < 0.01 {
        0.0
    } else {
        v * t
    }
}

/// Interpolation linéaire de vecteurs.
fn lerp_vec(a: Vec3, b: Vec3, k: f32) -> Vec3 {
    a + (b - a) * k
}

/// Décalage pseudo-aléatoire déterministe pour le shake.
fn shake_offset(amp: f32, now: f64) -> Vec3 {
    if amp <= 0.0 {
        return Vec3::default();
    }
    let t = now as f32;
    Vec3::new(
        amp * (t * 37.0).sin(),
        amp * 0.6 * (t * 29.0).cos(),
        amp * (t * 43.0).sin(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::scan::{DirNode, FileNode, ProjectTree};
    use std::path::PathBuf;

    fn layout_with(file: &str) -> Layout {
        Layout::build(&ProjectTree {
            root: PathBuf::from("/p"),
            dirs: vec![DirNode {
                path: PathBuf::from("src"),
                parent: None,
            }],
            files: vec![FileNode {
                path: PathBuf::from(file),
                dir: 0,
                lines: 300,
            }],
        })
    }

    fn state_with(file: Option<&str>, hub: bool) -> SceneState {
        let mut s = SceneState::new();
        s.main_file = file.map(PathBuf::from);
        s.hub = hub;
        s
    }

    #[test]
    fn but_hub_très_éloigné() {
        let l = layout_with("src/a.rs");
        let g = CamGoal::from_state(&state_with(None, true), &l);
        assert_eq!(g.dist, HUB.0);
        assert_eq!(g.pitch, HUB.1);
    }

    #[test]
    fn but_rapproché_sur_le_fichier_courant() {
        let l = layout_with("src/a.rs");
        let g = CamGoal::from_state(&state_with(Some("src/a.rs"), false), &l);
        assert_eq!(g.dist, CLOSE.0);
        assert!(g.target.y > 0.0);
    }

    #[test]
    fn but_d_ensemble_sans_fichier_courant() {
        let l = layout_with("src/a.rs");
        let g = CamGoal::from_state(&state_with(Some("inconnu.rs"), false), &l);
        assert_eq!(g.dist, OVERVIEW.0);
        assert_eq!(g.target, l.center());
    }

    #[test]
    fn la_caméra_converge_vers_son_but() {
        let l = layout_with("src/a.rs");
        let mut c = Camera::new(&l);
        let goal = CamGoal::from_state(&state_with(Some("src/a.rs"), false), &l);
        for _ in 0..400 {
            c.step(goal, 1.0 / 30.0);
        }
        let g = c.goal();
        assert!((g.dist - goal.dist).abs() < 1.0);
        assert!((g.pitch - goal.pitch).abs() < 0.01);
    }

    #[test]
    fn shake_et_punch_s_amortissent() {
        let l = layout_with("src/a.rs");
        let mut c = Camera::new(&l);
        c.impact(true);
        c.rewind_punch();
        let before = c.effective(0.0);
        assert!(before.fov > FOV);
        for _ in 0..400 {
            c.step(c.goal(), 1.0 / 30.0);
        }
        let after = c.effective(10.0);
        assert_eq!(after.fov, FOV);
        assert_eq!(after.target, c.goal().target);
        assert!(before.dist == after.dist);
    }

    #[test]
    fn l_entrée_est_ras_du_sol() {
        let l = layout_with("src/a.rs");
        let c = Camera::new(&l);
        assert_eq!(c.goal().dist, ENTRANCE.0);
        assert!(c.goal().pitch < CLOSE.1);
    }
}
