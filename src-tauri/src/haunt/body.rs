//! Boîtes de collision et corps pesants : déplacement axe par axe contre
//! les dalles (murs, sol, plafond, dalles traversables par-dessous) et
//! atterrissage sur les cercueils flottants.

use super::level::{Level, Tile, TILE};
use super::DT;

/// Rectangle (pixels du monde, coin haut-gauche).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Rectangle de taille `w`×`h` centré sur (cx, cy).
    pub fn around(cx: f32, cy: f32, w: f32, h: f32) -> Self {
        Self::new(cx - w / 2.0, cy - h / 2.0, w, h)
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }

    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }

    /// Chevauchement strict (bords qui se touchent = non).
    pub fn hits(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
}

/// Corps soumis à la pesanteur (Kiro, momies, têtes, cercueils…).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub vx: f32,
    pub vy: f32,
    /// Posé au sol (dalle ou cercueil flottant) à la fin du pas.
    pub ground: bool,
    /// A buté contre un mur pendant le pas.
    pub wall: bool,
}

impl Body {
    /// Corps `w`×`h` centré en `cx`, pieds en `feet`.
    pub fn new(cx: f32, feet: f32, w: f32, h: f32) -> Self {
        Self {
            x: cx - w / 2.0,
            y: feet - h,
            w,
            h,
            vx: 0.0,
            vy: 0.0,
            ground: false,
            wall: false,
        }
    }

    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }

    pub fn feet(&self) -> f32 {
        self.y + self.h
    }

    /// Accélère vers le bas pendant un pas, chute plafonnée à `max`
    /// (pixels par pas de référence ; voir [`super::DT`]).
    pub fn fall(&mut self, gravity: f32, max: f32) {
        self.vy = (self.vy + gravity * DT).min(max);
    }

    /// Avance d'un pas à (vx, vy) : x puis y, arrêté par les dalles pleines ;
    /// les dalles fines ne portent que par-dessus (`drop` = les traverser).
    pub fn travel(&mut self, level: &Level, drop: bool) {
        self.wall = false;
        let moved = Rect::new(self.x + self.vx * DT, self.y, self.w, self.h);
        match level.block_x(&moved, self.vx) {
            Some(x) => {
                self.x = x;
                self.vx = 0.0;
                self.wall = true;
            }
            None => self.x = moved.x,
        }
        let old_feet = self.feet();
        let moved = Rect::new(self.x, self.y + self.vy * DT, self.w, self.h);
        self.ground = false;
        if self.vy >= 0.0 {
            if let Some(top) = level.floor(&moved, old_feet, drop) {
                self.y = top - self.h;
                self.vy = 0.0;
                self.ground = true;
                return;
            }
        } else if let Some(under) = level.ceiling(&moved) {
            self.y = under;
            self.vy = 0.0;
            return;
        }
        self.y = moved.y;
    }

    /// Atterrit sur une surface mobile de haut `top` couvrant `x0..x1` si
    /// les pieds la traversent en descendant pendant ce pas.
    pub fn ride(&mut self, top: f32, x0: f32, x1: f32, old_feet: f32) -> bool {
        let feet = self.feet();
        if self.vy < 0.0 || self.x + self.w <= x0 || self.x >= x1 {
            return false;
        }
        if old_feet > top + 0.5 || feet < top {
            return false;
        }
        self.y = top - self.h;
        self.vy = 0.0;
        self.ground = true;
        true
    }

    /// Vide devant les pieds (direction `dir`) : une momie y fait demi-tour.
    pub fn ledge(&self, level: &Level, dir: f32) -> bool {
        let x = if dir > 0.0 {
            self.x + self.w + 2.0
        } else {
            self.x - 2.0
        };
        let below = level.at(x, self.feet() + TILE / 2.0);
        !below.solid() && below != Tile::Slab
    }
}
