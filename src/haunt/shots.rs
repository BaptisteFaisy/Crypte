//! Projectiles et objets : les **tokens** de Kiro (pastilles de texte),
//! les plaintes de la Dame blanche, les faux de la Faucheuse, les flammes
//! des tokens brûlants, le butin des cercueils et les cercueils flottants.

use super::body::Rect;
use super::level::Level;
use super::{DT, SUB};

/// Mots des tokens envoyés par Kiro (police 3×5, 3 lettres au plus).
pub const WORDS: [&str; 14] = [
    "THE", "ING", "OK", "AI", "42", "ED", "TO", "##", "LLM", "EOS", "YES", "GPU", "TOK", "?!",
];

/// Sorte de tokens : simples (départ), tranchants (couteau), brûlants
/// (lanterne).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weapon {
    Token,
    Sharp,
    Hot,
}

impl Weapon {
    /// Tokens en vol au plus (c'est dur : il faut viser).
    pub fn max(self) -> usize {
        match self {
            Weapon::Token => 2,
            Weapon::Sharp => 3,
            Weapon::Hot => 1,
        }
    }

    pub fn damage(self) -> i16 {
        match self {
            Weapon::Hot => 2,
            _ => 1,
        }
    }

    /// Traverse les créatures (touchées une fois chacune).
    pub fn pierce(self) -> bool {
        self == Weapon::Sharp
    }
}

/// Token en vol (centre x, y).
#[derive(Debug, Clone, PartialEq)]
pub struct Shot {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub weapon: Weapon,
    pub word: &'static str,
    pub age: u16,
}

impl Shot {
    /// Token lancé de (x, y) vers `dir` (±1).
    pub fn new(weapon: Weapon, x: f32, y: f32, dir: f32, word: &'static str) -> Shot {
        let (vx, vy) = match weapon {
            Weapon::Token => (6.0, 0.0),
            Weapon::Sharp => (8.5, 0.0),
            Weapon::Hot => (3.6, -4.2),
        };
        Shot {
            x,
            y,
            vx: vx * dir,
            vy,
            weapon,
            word,
            age: 0,
        }
    }

    /// Pastille : largeur du mot + 2 px de chaque côté, 9 px de haut.
    pub fn rect(&self) -> Rect {
        let w = (self.word.chars().count() as f32 * 4.0 - 1.0).max(3.0) + 4.0;
        Rect::around(self.x, self.y, w, 9.0)
    }

    /// Un pas de vol ; faux s'il s'éteint (mur, sol, portée).
    pub fn fly(&mut self, level: &Level) -> bool {
        self.age += 1;
        if self.weapon == Weapon::Hot {
            self.vy = (self.vy + 0.35 * DT).min(6.0);
        }
        self.x += self.vx * DT;
        self.y += self.vy * DT;
        let tile = level.at(
            self.x,
            self.y + if self.weapon == Weapon::Hot { 4.0 } else { 0.0 },
        );
        u32::from(self.age) < 70 * SUB && !tile.solid() && self.y < level.height()
    }
}

/// Projectile ennemi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoltKind {
    Wail,
    Scythe,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bolt {
    pub kind: BoltKind,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub age: u16,
}

impl Bolt {
    /// Projectile de (x, y) visant `to` à la vitesse du genre.
    pub fn aimed(kind: BoltKind, (x, y): (f32, f32), to: (f32, f32)) -> Bolt {
        let speed = match kind {
            BoltKind::Wail => 2.2,
            BoltKind::Scythe => 3.4,
        };
        let (dx, dy) = (to.0 - x, to.1 - y);
        let d = (dx * dx + dy * dy).sqrt().max(1.0);
        Bolt {
            kind,
            x,
            y,
            vx: dx / d * speed,
            vy: dy / d * speed,
            age: 0,
        }
    }

    pub fn rect(&self) -> Rect {
        match self.kind {
            BoltKind::Wail => Rect::around(self.x, self.y, 7.0, 7.0),
            BoltKind::Scythe => Rect::around(self.x, self.y, 11.0, 11.0),
        }
    }

    /// Un pas : la plainte ondule, la faux file droit.
    pub fn fly(&mut self) {
        self.age += 1;
        self.x += self.vx * DT;
        self.y += self.vy * DT;
        if self.kind == BoltKind::Wail {
            self.y += (f32::from(self.age) * DT * 0.4).sin() * 0.6 * DT;
        }
    }
}

/// Flamme d'un token brûlant (pied en x, y), brûle ~0,8 s.
#[derive(Debug, Clone, PartialEq)]
pub struct Flame {
    pub x: f32,
    pub y: f32,
    pub age: u16,
}

impl Flame {
    pub const LIFE: u16 = 24 * SUB as u16;

    /// Flamme posée au sol sous (x, y) si un sol est proche.
    pub fn new(x: f32, y: f32, level: &Level) -> Flame {
        let y = level
            .ground_below(x, y - 4.0)
            .filter(|g| *g - y < 24.0)
            .unwrap_or(y + 6.0);
        Flame { x, y, age: 0 }
    }

    pub fn rect(&self) -> Rect {
        Rect::new(self.x - 10.0, self.y - 16.0, 20.0, 16.0)
    }
}

/// Butin d'un cercueil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loot {
    Sharp,
    Hot,
    Shroud,
    Candy,
}

/// Objet à ramasser (pieds en x, y).
#[derive(Debug, Clone, PartialEq)]
pub struct Pickup {
    pub loot: Loot,
    pub x: f32,
    pub y: f32,
    pub vy: f32,
    pub age: u16,
}

impl Pickup {
    /// Disparaît après 12 s.
    pub const LIFE: u16 = 360 * SUB as u16;

    /// Objet qui saute hors du cercueil puis retombe.
    pub fn new(loot: Loot, x: f32, y: f32) -> Pickup {
        Pickup {
            loot,
            x,
            y,
            vy: -4.0,
            age: 0,
        }
    }

    pub fn rect(&self) -> Rect {
        Rect::new(self.x - 9.0, self.y - 20.0, 18.0, 20.0)
    }

    /// Un pas de chute jusqu'au sol.
    pub fn fall(&mut self, level: &Level) {
        self.age += 1;
        self.vy = (self.vy + 0.35 * DT).min(5.0);
        let next = self.y + self.vy * DT;
        match level.ground_below(self.x, self.y - 1.0) {
            Some(g) if next >= g && self.vy > 0.0 => {
                self.y = g;
                self.vy = 0.0;
            }
            _ => self.y = next,
        }
    }
}

/// Cercueil flottant sur l'ectoplasme : va et vient horizontalement.
#[derive(Debug, Clone, PartialEq)]
pub struct Platform {
    pub x: f32,
    pub y: f32,
    home: f32,
    dir: f32,
    /// Déplacement du dernier pas (Kiro, posé dessus, suit).
    pub dx: f32,
}

impl Platform {
    pub const W: f32 = 48.0;
    pub const H: f32 = 12.0;
    const RANGE: f32 = 44.0;
    const SPEED: f32 = 0.9;

    /// Cercueil centré en `cx`, dessus au haut de la dalle du repère.
    pub fn new(cx: f32, mark_bottom: f32) -> Platform {
        let x = cx - Self::W / 2.0;
        Platform {
            x,
            y: mark_bottom - 16.0 + 4.0,
            home: x,
            dir: 1.0,
            dx: 0.0,
        }
    }

    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, Self::W, Self::H)
    }

    /// Un pas de va-et-vient.
    pub fn slide(&mut self) {
        if (self.x - self.home).abs() >= Self::RANGE {
            self.dir = -(self.x - self.home).signum();
        }
        self.dx = self.dir * Self::SPEED * DT;
        self.x += self.dx;
    }
}
