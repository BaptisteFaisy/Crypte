//! Particules : éclats d'une créature détruite (aux couleurs de son
//! sprite), âme qui s'envole, lambeaux du linceul, étincelles d'un token,
//! terre soulevée. Plafonnées.

use image::{Rgba, RgbaImage};
use ratatui::style::Color;

use super::dice::Dice;
use super::{DT, SUB};
use crate::canvas::{put, px};
use crate::theme;

/// Particules vivantes au plus (les plus anciennes cèdent la place).
const MAX: usize = 420;

#[derive(Debug, Clone, PartialEq)]
pub struct Spark {
    pub x: f32,
    pub y: f32,
    vx: f32,
    vy: f32,
    ttl: u16,
    colour: Rgba<u8>,
    gravity: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Sparks {
    pub list: Vec<Spark>,
}

impl Sparks {
    /// Gerbe de `n` éclats depuis (x, y), couleurs tirées de `colours`.
    pub fn burst(&mut self, (x, y): (f32, f32), n: usize, colours: &[Color], dice: &mut Dice) {
        for i in 0..n {
            let a = dice.range(0.0, std::f32::consts::TAU);
            let v = dice.range(0.4, 1.6);
            self.push(Spark {
                x,
                y,
                vx: a.cos() * v,
                vy: a.sin() * v - 0.8,
                ttl: (14 + dice.below(16) as u16) * SUB as u16,
                colour: px(colours[i % colours.len().max(1)]),
                gravity: 0.1,
            });
        }
    }

    /// Âme qui s'élève en ondulant (créature détruite, Kiro mort).
    pub fn soul(&mut self, x: f32, y: f32, dice: &mut Dice) {
        for i in 0..8 {
            self.push(Spark {
                x: x + dice.range(-2.0, 2.0),
                y: y - i as f32,
                vx: dice.range(-0.2, 0.2),
                vy: -dice.range(0.3, 0.8),
                ttl: (30 + dice.below(20) as u16) * SUB as u16,
                colour: px(if i % 3 == 0 { theme::BONE } else { theme::ECTO }),
                gravity: -0.01,
            });
        }
    }

    /// Lambeaux du linceul arraché.
    pub fn rags(&mut self, x: f32, y: f32, dice: &mut Dice) {
        self.burst((x, y), 18, &[theme::BONE, theme::SAND, theme::BONE], dice);
    }

    /// Étincelles d'un token qui touche ou s'éteint.
    pub fn spark(&mut self, x: f32, y: f32, dice: &mut Dice) {
        self.burst((x, y), 5, &[theme::CANDLE, theme::PUMPKIN], dice);
    }

    /// Terre soulevée (une créature sort du sol).
    pub fn dust(&mut self, x: f32, feet: f32, dice: &mut Dice) {
        self.burst((x, feet - 2.0), 4, &[theme::UMBER, theme::BURLAP], dice);
    }

    fn push(&mut self, s: Spark) {
        if self.list.len() >= MAX {
            self.list.remove(0);
        }
        self.list.push(s);
    }

    /// Un pas : mouvement, pesanteur, extinction.
    pub fn step(&mut self) {
        for s in &mut self.list {
            s.vy += s.gravity * DT;
            s.x += s.vx * DT;
            s.y += s.vy * DT;
            s.ttl = s.ttl.saturating_sub(1);
        }
        self.list.retain(|s| s.ttl > 0);
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }

    /// Dessine les particules (monde → écran : `x - cam`).
    pub fn draw(&self, cv: &mut RgbaImage, cam: f32, top: f32) {
        for s in &self.list {
            let y = (s.y - top).round() as i64;
            put(cv, (s.x - cam).round() as i64, y, s.colour);
            if u32::from(s.ttl) > 20 * SUB {
                put(cv, (s.x - cam).round() as i64 + 1, y, s.colour);
            }
        }
    }
}
