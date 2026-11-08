//! La Faucheuse (sprite du bestiaire ×2, 64×64), gardienne de l'arène :
//! descend du ciel, dérive en lançant des faux, invoque des fantômes
//! noirs ; à mi-vie, elle s'arme (tremble, yeux rouges) puis plonge sur
//! Kiro jusqu'au sol avant de remonter.

use super::body::Rect;
use super::dice::Dice;
use super::shots::{Bolt, BoltKind};
use super::{DT, SUB};

/// Côté du sprite (pixels).
pub const SIZE: f32 = 64.0;
pub const HP: i16 = 36;
/// Durée de l'agonie (pas) avant la victoire.
pub const DYING: u16 = 100 * SUB as u16;
/// Hauteur de vol au repos (haut du sprite).
const SKY: f32 = 22.0;
const WIND: u16 = 26 * SUB as u16;
const DIVE: u16 = 40 * SUB as u16;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Enter,
    Drift,
    /// Armement : tremble, puis plonge (pas restants).
    Wind(u16),
    /// Plongeon de `from` vers l'abscisse `to` (pas écoulés).
    Dive {
        from: (f32, f32),
        to: f32,
        k: u16,
    },
}

/// Ce que la Faucheuse fait ce pas-ci.
#[derive(Debug, Clone, PartialEq)]
pub enum Deed {
    Throw(Bolt),
    Summon(f32, f32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reaper {
    pub x: f32,
    pub y: f32,
    pub hp: i16,
    pub age: u32,
    pub flash: u8,
    pub facing: f32,
    pub dying: u16,
    mode: Mode,
    /// Bord gauche de l'arène (pixels).
    home: f32,
}

impl Reaper {
    /// Faucheuse au-dessus de l'arène commençant en `home`.
    pub fn new(home: f32) -> Reaper {
        Reaper {
            x: home + 200.0,
            y: -SIZE,
            hp: HP,
            age: 0,
            flash: 0,
            facing: -1.0,
            dying: 0,
            mode: Mode::Enter,
            home,
        }
    }

    /// Boîte : la robe et le capuchon, pas la faux.
    pub fn rect(&self) -> Rect {
        Rect::new(self.x + 15.0, self.y + 8.0, 34.0, 52.0)
    }

    pub fn vulnerable(&self) -> bool {
        self.dying == 0 && self.mode != Mode::Enter
    }

    pub fn harmful(&self) -> bool {
        self.dying == 0
    }

    /// Tremble avant de plonger (dessin + sons… visuels).
    pub fn winding(&self) -> bool {
        matches!(self.mode, Mode::Wind(_))
    }

    /// Encaisse `dmg` ; vrai si c'est le coup fatal.
    pub fn hurt(&mut self, dmg: i16) -> bool {
        if !self.vulnerable() || self.flash > 0 {
            return false;
        }
        self.hp -= dmg;
        self.flash = 3 * SUB as u8;
        if self.hp <= 0 {
            self.dying = 1;
            return true;
        }
        false
    }

    /// Un pas ; `prey` = centre de Kiro, `ground` = sol de l'arène.
    pub fn think(&mut self, prey: (f32, f32), ground: f32, dice: &mut Dice) -> Option<Deed> {
        self.age += 1;
        self.flash = self.flash.saturating_sub(1);
        if self.dying > 0 {
            self.dying += 1;
            self.y += 0.4 * DT;
            return None;
        }
        self.facing = if prey.0 >= self.x + SIZE / 2.0 {
            1.0
        } else {
            -1.0
        };
        match self.mode {
            Mode::Enter => {
                self.y += 2.0 * DT;
                if self.y >= SKY {
                    self.mode = Mode::Drift;
                }
                None
            }
            Mode::Drift => self.drift(prey, dice),
            Mode::Wind(k) => {
                self.mode = match k {
                    0 => Mode::Dive {
                        from: (self.x, self.y),
                        to: prey.0 - SIZE / 2.0,
                        k: 0,
                    },
                    _ => Mode::Wind(k - 1),
                };
                None
            }
            Mode::Dive { from, to, k } => self.dive(from, to, k, ground),
        }
    }

    /// Dérive en huit, faux régulières, fantômes de temps en temps.
    fn drift(&mut self, prey: (f32, f32), dice: &mut Dice) -> Option<Deed> {
        let t = self.age as f32 * DT;
        let cx = self.home + 160.0 + (t * 0.021).sin() * 110.0;
        self.x += (cx - SIZE / 2.0 - self.x) * 0.08 * DT;
        self.y += (SKY + (t * 0.07).sin() * 8.0 - self.y) * 0.1 * DT;
        let enraged = self.hp <= HP / 2;
        if enraged && self.age % (150 * SUB) == 0 {
            self.mode = Mode::Wind(WIND);
            return None;
        }
        let every = if enraged { 52 } else { 66 } * SUB;
        if self.age % every == 0 {
            let hand = (self.x + SIZE / 2.0 + self.facing * 18.0, self.y + 24.0);
            let aim = (prey.0, prey.1 + dice.range(-6.0, 6.0));
            return Some(Deed::Throw(Bolt::aimed(BoltKind::Scythe, hand, aim)));
        }
        if self.age % (240 * SUB) == 120 * SUB {
            return Some(Deed::Summon(self.x + SIZE / 2.0, self.y + SIZE / 2.0));
        }
        None
    }

    /// Plongeon : glisse jusqu'à `to` en descendant au sol, puis remonte.
    fn dive(&mut self, from: (f32, f32), to: f32, k: u16, ground: f32) -> Option<Deed> {
        let u = f32::from(k) / f32::from(DIVE);
        let depth = ground - SIZE + 4.0 - from.1;
        self.x = from.0 + (to - from.0) * (u * 2.0).min(1.0);
        self.y = from.1 + depth * (u * std::f32::consts::PI).sin();
        self.mode = if k >= DIVE {
            Mode::Drift
        } else {
            Mode::Dive { from, to, k: k + 1 }
        };
        None
    }
}
