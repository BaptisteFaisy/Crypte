//! Projection perspective 3D → 2D, culling et tri du peintre.
//!
//! Repère monde (comme le prototype three.js) : `x`/`z` au sol, `y` vers le
//! haut. La caméra orbite une cible (`distance`, `yaw`, `pitch`) ; le rendu
//! est purement logiciel, sans profondeur par pixel — l'ordre de peinture
//! (peintre) trie les objets du plus loin au plus près.

/// Point du monde (unités monde du prototype : dalle 110, pas 150…).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    /// Nouveau point.
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Produit scalaire.
    pub fn dot(self, b: Self) -> f32 {
        self.x * b.x + self.y * b.y + self.z * b.z
    }

    /// Produit vectoriel.
    pub fn cross(self, b: Self) -> Self {
        Self::new(
            self.y * b.z - self.z * b.y,
            self.z * b.x - self.x * b.z,
            self.x * b.y - self.y * b.x,
        )
    }

    /// Norme euclidienne.
    pub fn len(self) -> f32 {
        self.dot(self).sqrt()
    }

    /// Vecteur unitaire (nul si norme nulle).
    pub fn normalize(self) -> Self {
        let l = self.len();
        if l < 1e-9 {
            Self::default()
        } else {
            Self::new(self.x / l, self.y / l, self.z / l)
        }
    }
}

impl std::ops::Add for Vec3 {
    type Output = Self;

    fn add(self, b: Self) -> Self {
        Self::new(self.x + b.x, self.y + b.y, self.z + b.z)
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Self;

    fn sub(self, b: Self) -> Self {
        Self::new(self.x - b.x, self.y - b.y, self.z - b.z)
    }
}

impl std::ops::Mul<f32> for Vec3 {
    type Output = Self;

    fn mul(self, k: f32) -> Self {
        Self::new(self.x * k, self.y * k, self.z * k)
    }
}

/// Position d'une caméra orbitale : cible + (distance, yaw, pitch, fov).
/// Calculée par [`super::cam::Camera`], consommée par [`project`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamParams {
    pub target: Vec3,
    pub dist: f32,
    /// Lacet en radians (0 = derrière sur +z).
    pub yaw: f32,
    /// Tangage en radians (positif = au-dessus).
    pub pitch: f32,
    /// Champ vertical en radians.
    pub fov: f32,
}

impl CamParams {
    /// Position de l'œil (même formule que le prototype :
    /// `pos = cible + d·(cos(p)·sin(y), sin(p), cos(p)·cos(y))`).
    pub fn eye(&self) -> Vec3 {
        let (sp, cp) = (self.pitch.sin(), self.pitch.cos());
        let (sy, cy) = (self.yaw.sin(), self.yaw.cos());
        self.target + Vec3::new(self.dist * cp * sy, self.dist * sp, self.dist * cp * cy)
    }
}

/// Distance minimale de rendu (proche du plan de la caméra, unités monde).
pub const NEAR: f32 = 10.0;

/// Repère vue : œil + base orthonormée (droite, haut, avant).
struct Basis {
    eye: Vec3,
    right: Vec3,
    up: Vec3,
    fwd: Vec3,
    /// `h/2 / tan(fov/2)` — facteur de projection perspective.
    f: f32,
    cx: f32,
    cy: f32,
}

/// Construit la base de vue pour un canevas `w × h`.
fn basis(cam: &CamParams, w: u32, h: u32) -> Basis {
    let eye = cam.eye();
    let fwd = (cam.target - eye).normalize();
    let right = fwd.cross(Vec3::new(0.0, 1.0, 0.0)).normalize();
    let up = right.cross(fwd);
    let tan = (cam.fov / 2.0).tan().max(1e-6);
    Basis {
        eye,
        right,
        up,
        fwd,
        f: (h as f32 / 2.0) / tan,
        cx: w as f32 / 2.0,
        cy: h as f32 / 2.0,
    }
}

/// Coordonnées vue d'un point monde (x droite, y haut, z profondeur avant).
fn view_coords(b: &Basis, p: Vec3) -> (f32, f32, f32) {
    let d = p - b.eye;
    (d.dot(b.right), d.dot(b.up), d.dot(b.fwd))
}

/// Projette un point monde : `Some((x écran, y écran, profondeur))`, ou
/// `None` s'il est derrière la caméra (culling, plan proche [`NEAR`]).
pub fn project(cam: &CamParams, p: Vec3, w: u32, h: u32) -> Option<(f32, f32, f32)> {
    let b = basis(cam, w, h);
    let (vx, vy, vz) = view_coords(&b, p);
    if vz <= NEAR {
        return None;
    }
    Some((b.cx + vx * b.f / vz, b.cy - vy * b.f / vz, vz))
}

/// Distance monde entre l'œil et un point (pour le brouillard, les fades).
pub fn eye_dist(cam: &CamParams, p: Vec3) -> f32 {
    (p - cam.eye()).len()
}

/// Rayon écran (px) d'une taille monde à la profondeur `depth`.
pub fn world_to_px(cam: &CamParams, size: f32, depth: f32, h: u32) -> f32 {
    let tan = (cam.fov / 2.0).tan().max(1e-6);
    size * (h as f32 / 2.0) / tan / depth.max(1.0)
}

/// Tri du peintre : ordonne des indices par profondeur **décroissante**
/// (le plus loin d'abord peint). `depth_of(i)` rend la distance monde.
pub fn painter_order<T>(items: &mut [T], mut depth_of: impl FnMut(&T) -> f32) {
    items.sort_by(|a, b| {
        depth_of(b)
            .partial_cmp(&depth_of(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    fn cam() -> CamParams {
        CamParams {
            target: Vec3::default(),
            dist: 800.0,
            yaw: 0.0,
            pitch: 0.4,
            fov: 42.0_f32.to_radians(),
        }
    }

    #[test]
    fn vec3_base() {
        let v = Vec3::new(3.0, 0.0, 4.0);
        assert_eq!(v.len(), 5.0);
        assert_eq!(v.normalize().len(), 1.0);
        assert_eq!(Vec3::default().normalize(), Vec3::default());
        assert_eq!(
            Vec3::new(1.0, 0.0, 0.0).cross(Vec3::new(0.0, 1.0, 0.0)),
            Vec3::new(0.0, 0.0, 1.0)
        );
    }

    #[test]
    fn cible_au_centre() {
        let (x, y, d) = project(&cam(), Vec3::default(), 960, 540).expect("devant");
        assert!((x - 480.0).abs() < 1.0 && (y - 270.0).abs() < 1.0);
        assert!(d > 700.0);
    }

    #[test]
    fn derriere_la_caméra_cullé() {
        // Un point à l'opposé de la cible, derrière l'œil.
        let c = cam();
        let behind = c.eye() + (c.eye() - c.target).normalize() * 100.0;
        assert!(project(&c, behind, 960, 540).is_none());
    }

    #[test]
    fn hauteur_écran_décroît_avec_la_distance() {
        let c = CamParams {
            pitch: FRAC_PI_2 / 2.0,
            ..cam()
        };
        let proche = project(&c, Vec3::new(0.0, 50.0, 0.0), 960, 540);
        let lointain = project(
            &CamParams { dist: 4000.0, ..c },
            Vec3::new(0.0, 50.0, 0.0),
            960,
            540,
        );
        assert!(proche.is_some() && lointain.is_some());
    }

    #[test]
    fn peintre_du_loin_au_près() {
        let mut v = vec![1.0_f32, 5.0, 3.0];
        painter_order(&mut v, |d| *d);
        assert_eq!(v, vec![5.0, 3.0, 1.0]);
    }
}
