//! Créatures du bestiaire en jeu :
//! - momie : sort de terre près de Kiro puis marche vers lui ;
//! - momie pressée : charge tout droit, tombe dans les fosses ;
//! - chauve-souris : attend, puis pique en ondulant ;
//! - main de tombe : jaillit quand Kiro approche, reste plantée ;
//! - petit fantôme noir (sous-agent du hub) : traque à travers les murs ;
//! - Dame blanche : apparaît, gémit trois fois, s'efface, réapparaît ;
//! - tête de zombie : rebondit vers Kiro ;
//! - cercueil : inoffensif, s'ouvre sous les tokens (butin ou piège).

use super::body::Body;
use super::dice::Dice;
use super::level::Level;
use super::player::{GRAVITY, MAX_FALL};
use super::shots::{Bolt, BoltKind, Loot};
use super::{DT, SUB};

/// Pas pour sortir de terre (momie, main de tombe).
pub const RISE: u8 = 24 * SUB as u8;
/// Cycle de la Dame blanche (pas de référence) : apparition (18),
/// 3 plaintes, fondu (18), absence.
const LADY_CYCLE: u32 = 132;
/// La main de tombe jaillit quand Kiro est à moins de ça (pixels).
const HAND_REACH: f32 = 72.0;
/// La chauve-souris pique quand Kiro est à moins de ça (pixels).
const BAT_REACH: f32 = 150.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Mummy,
    Runner,
    Bat,
    Hand,
    Wisp,
    Lady,
    Head,
    Coffin,
}

impl Kind {
    /// Boîte de collision (largeur, hauteur), plus petite que le dessin.
    pub fn size(self) -> (f32, f32) {
        match self {
            Kind::Mummy | Kind::Runner => (14.0, 28.0),
            Kind::Bat => (18.0, 10.0),
            Kind::Hand => (12.0, 22.0),
            Kind::Wisp => (10.0, 12.0),
            Kind::Lady => (16.0, 28.0),
            Kind::Head => (18.0, 18.0),
            Kind::Coffin => (18.0, 28.0),
        }
    }

    pub fn hp(self) -> i16 {
        match self {
            Kind::Bat | Kind::Wisp => 1,
            Kind::Mummy | Kind::Runner | Kind::Head => 2,
            Kind::Hand | Kind::Coffin => 3,
            Kind::Lady => 5,
        }
    }

    pub fn score(self) -> u32 {
        match self {
            Kind::Wisp => 50,
            Kind::Mummy | Kind::Bat => 100,
            Kind::Hand | Kind::Head => 150,
            Kind::Runner => 200,
            Kind::Lady => 500,
            Kind::Coffin => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Foe {
    pub kind: Kind,
    pub body: Body,
    pub hp: i16,
    pub age: u32,
    /// Pas de silhouette blanche après un coup.
    pub flash: u8,
    pub facing: f32,
    /// Pas restant pour sortir de terre (0 = dehors).
    pub rise: u8,
    pub alpha: f32,
    /// Hauteur de vol de référence (chauve-souris), centre.
    pub base: f32,
    /// Chauve-souris lancée / main sortie.
    pub awake: bool,
    /// Butin d'un cercueil (`None` = piège : une momie en sort).
    pub loot: Option<Loot>,
    pub open: bool,
    pub gone: bool,
}

impl Foe {
    /// Créature `kind` centrée en `cx`, pieds en `feet`.
    pub fn new(kind: Kind, cx: f32, feet: f32) -> Foe {
        let (w, h) = kind.size();
        Foe {
            kind,
            body: Body::new(cx, feet, w, h),
            hp: kind.hp(),
            age: 0,
            flash: 0,
            facing: -1.0,
            rise: if matches!(kind, Kind::Mummy | Kind::Hand) {
                RISE
            } else {
                0
            },
            alpha: if kind == Kind::Lady { 0.0 } else { 1.0 },
            base: feet - h / 2.0,
            awake: false,
            loot: None,
            open: false,
            gone: false,
        }
    }

    /// Centre de la boîte.
    pub fn center(&self) -> (f32, f32) {
        let r = self.body.rect();
        (r.cx(), r.cy())
    }

    /// Visible et assez sortie : les tokens la touchent.
    pub fn vulnerable(&self) -> bool {
        !self.gone && !self.open && self.rise < RISE / 2 && self.alpha > 0.5
    }

    /// Blesse Kiro au contact (jamais un cercueil).
    pub fn harmful(&self) -> bool {
        self.kind != Kind::Coffin && self.vulnerable()
    }

    /// Un pas d'IA ; `prey` = centre de Kiro. Rend un projectile éventuel.
    pub fn think(&mut self, prey: (f32, f32), level: &Level, dice: &mut Dice) -> Option<Bolt> {
        self.age += 1;
        self.flash = self.flash.saturating_sub(1);
        match self.kind {
            Kind::Mummy => self.shamble(prey, level),
            Kind::Runner => self.charge(level),
            Kind::Bat => self.swoop(prey),
            Kind::Hand => self.burst(prey),
            Kind::Wisp => self.stalk(prey),
            Kind::Lady => return self.wail(prey, level, dice),
            Kind::Head => self.bounce(prey, level),
            Kind::Coffin => self.settle(level),
        }
        None
    }

    /// Se tourne vers Kiro.
    fn face(&mut self, prey: (f32, f32)) {
        self.facing = if prey.0 >= self.body.cx() { 1.0 } else { -1.0 };
    }

    /// Momie : sort de terre, marche lentement, fait demi-tour au bord.
    fn shamble(&mut self, prey: (f32, f32), level: &Level) {
        if self.rise > 0 {
            self.rise -= 1;
            if self.rise == 0 {
                self.face(prey);
            }
            return;
        }
        if self.age % (120 * SUB) == 0 {
            self.face(prey);
        }
        self.body.vx = self.facing * 0.6;
        self.body.fall(GRAVITY, MAX_FALL);
        self.body.travel(level, false);
        if self.body.wall || (self.body.ground && self.body.ledge(level, self.facing)) {
            self.facing = -self.facing;
        }
    }

    /// Momie pressée : fonce sans regarder où elle met les pieds.
    fn charge(&mut self, level: &Level) {
        self.body.vx = self.facing * 3.0;
        self.body.fall(GRAVITY, MAX_FALL);
        self.body.travel(level, false);
        if self.body.wall {
            self.facing = -self.facing;
        }
    }

    /// Chauve-souris : plane sur place, puis pique en ondulant vers Kiro.
    fn swoop(&mut self, prey: (f32, f32)) {
        if !self.awake {
            if (prey.0 - self.body.cx()).abs() < BAT_REACH {
                self.awake = true;
                self.face(prey);
                self.age = 0;
            }
            let bob = (self.age as f32 * DT * 0.1).sin() * 2.0;
            self.body.y = self.base + bob - self.body.h / 2.0;
            return;
        }
        let t = self.age as f32 * DT;
        self.base += (prey.1 - 4.0 - self.base) * 0.035 * DT;
        self.body.x += self.facing * 1.9 * DT;
        self.body.y = self.base + (t * 0.18).sin() * 16.0 - self.body.h / 2.0;
    }

    /// Main de tombe : jaillit quand Kiro approche.
    fn burst(&mut self, prey: (f32, f32)) {
        if !self.awake && (prey.0 - self.body.cx()).abs() < HAND_REACH {
            self.awake = true;
        }
        if self.awake && self.rise > 0 {
            self.rise -= 1;
        }
    }

    /// Fantôme noir : file vers Kiro en ondulant, à travers tout.
    fn stalk(&mut self, prey: (f32, f32)) {
        let (cx, cy) = self.center();
        let (dx, dy) = (prey.0 - cx, prey.1 - cy);
        let d = (dx * dx + dy * dy).sqrt().max(1.0);
        let wobble = (self.age as f32 * DT * 0.2).sin() * 0.5;
        let (tx, ty) = (dx / d * 1.25, dy / d * 1.25 + wobble);
        self.body.vx += (tx - self.body.vx) * 0.08 * DT;
        self.body.vy += (ty - self.body.vy) * 0.08 * DT;
        self.body.x += self.body.vx * DT;
        self.body.y += self.body.vy * DT;
        if self.body.vx.abs() > 0.05 {
            self.facing = self.body.vx.signum();
        }
    }

    /// Dame blanche : cycle apparition → 3 plaintes → fondu → ailleurs.
    fn wail(&mut self, prey: (f32, f32), level: &Level, dice: &mut Dice) -> Option<Bolt> {
        let beat = self.age % SUB == 0;
        let p = self.age / SUB % LADY_CYCLE;
        if beat && p == 0 {
            let x = prey.0 + dice.sign() * dice.range(70.0, 110.0);
            if let Some(g) = level.ground_below(x, prey.1 - 40.0) {
                self.body = Body::new(x, g, self.body.w, self.body.h);
            }
        }
        self.alpha = match p {
            0..=17 => p as f32 / 18.0,
            18..=101 => 1.0,
            102..=119 => (120 - p) as f32 / 18.0,
            _ => 0.0,
        };
        if beat && matches!(p, 32 | 60 | 88) {
            self.face(prey);
            let mouth = (self.body.cx() + self.facing * 4.0, self.body.y + 8.0);
            return Some(Bolt::aimed(BoltKind::Wail, mouth, prey));
        }
        None
    }

    /// Tête de zombie : rebondit vers Kiro.
    fn bounce(&mut self, prey: (f32, f32), level: &Level) {
        self.body.fall(GRAVITY * 0.8, MAX_FALL);
        let vx = self.body.vx;
        self.body.travel(level, false);
        if self.body.wall {
            self.body.vx = -vx;
        }
        if self.body.ground {
            self.face(prey);
            self.body.vy = -6.2;
            self.body.vx = self.facing * 1.7;
        }
    }

    /// Cercueil : posé au sol, attend les tokens.
    fn settle(&mut self, level: &Level) {
        self.body.vx = 0.0;
        self.body.fall(GRAVITY, MAX_FALL);
        self.body.travel(level, false);
    }
}
