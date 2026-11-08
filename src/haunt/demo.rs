//! Planche de démonstration (`--haunt-shot <png>`) : 4 frames du jeu
//! (titre, cimetière, marais, Faucheuse) en 2×2, sans terminal.
//! À regénérer après une retouche de sprite, de tuile ou du niveau.

use anyhow::{Context, Result};
use crossterm::event::KeyEventKind;
use image::{imageops, RgbaImage};

use super::draw::snapshot;
use super::foes::{Foe, Kind};
use super::input::Act;
use super::shots::Weapon;
use super::{Haunt, Phase, SUB, VIEW_H, VIEW_W};

/// Agrandissement de la planche (les frames font déjà 960×576).
const ZOOM: u32 = 1;

/// Écrit la planche PNG (2×2 frames) dans `path`.
pub fn write_shot(path: &str) -> Result<()> {
    let mut game = Haunt::new();
    let mut frames = Vec::with_capacity(4);
    game.run(20);
    frames.push(snapshot(&mut game));
    game.graveyard();
    frames.push(snapshot(&mut game));
    game.swamp();
    frames.push(snapshot(&mut game));
    game.arena_fight();
    frames.push(snapshot(&mut game));
    let (w, h) = (VIEW_W * ZOOM, VIEW_H * ZOOM);
    let mut sheet = RgbaImage::new(w * 2, h * 2);
    for (i, f) in frames.iter().enumerate() {
        let big = imageops::resize(f, w, h, imageops::FilterType::Nearest);
        let (x, y) = ((i as u32 % 2) * w, (i as u32 / 2) * h);
        imageops::overlay(&mut sheet, &big, i64::from(x), i64::from(y));
    }
    sheet.save(path).with_context(|| format!("writing {path}"))
}

impl Haunt {
    /// `n` pas de référence (1/30 s) ; Kiro, invulnérable, envoie un token
    /// de temps en temps.
    fn run(&mut self, n: u32) {
        for i in 0..n * SUB {
            if self.phase == Phase::Play {
                self.kiro.hurt = self.kiro.hurt.max(2);
                if i % (9 * SUB) == 0 {
                    self.input.feed(Act::Fire, KeyEventKind::Press, self.step);
                }
            }
            self.tick();
        }
    }

    /// Kiro au cœur du cimetière : momies qui sortent de terre,
    /// chauve-souris, tokens en vol, escaliers de dalles.
    fn graveyard(&mut self) {
        self.new_run();
        let feet = self.teleport(300.0);
        for (x, rise) in [(340.0, 0), (372.0, 9), (262.0, 0)] {
            let mut f = Foe::new(Kind::Mummy, x, feet);
            f.rise = rise * SUB as u8;
            self.foes.push(f);
        }
        let mut bat = Foe::new(Kind::Bat, 352.0, feet - 30.0);
        bat.awake = true;
        self.foes.push(bat);
        self.run(10);
    }

    /// Marais d'ectoplasme : cercueils flottants, fantômes noirs, Dame
    /// blanche, tokens tranchants.
    fn swamp(&mut self) {
        self.new_run();
        self.kiro.weapon = Weapon::Sharp;
        self.checkpoint = self.level.mark('k').map_or(0.0, |m| m.x);
        self.respawn();
        let feet = self.teleport(2146.0);
        let mut lady = Foe::new(Kind::Lady, 2196.0, feet);
        lady.age = 30 * SUB;
        self.foes.push(lady);
        self.run(14);
    }

    /// Arène : la Faucheuse lance ses faux, tokens brûlants.
    fn arena_fight(&mut self) {
        self.new_run();
        self.kiro.weapon = Weapon::Hot;
        let gate = self.level.mark('R').map_or(0.0, |m| m.x);
        self.checkpoint = gate - 16.0;
        self.respawn();
        self.teleport(gate + 120.0);
        self.run(150);
    }

    /// Kiro posé au sol (pas sur une dalle) en `x`, vue recentrée ; rend
    /// la hauteur du sol.
    fn teleport(&mut self, x: f32) -> f32 {
        let low = self.level.height() - 60.0;
        let feet = self.level.ground_below(x, low).unwrap_or(low);
        self.kiro.body.x = x - self.kiro.body.w / 2.0;
        self.kiro.body.y = feet - self.kiro.body.h;
        let max = (self.level.width() - VIEW_W as f32).max(0.0);
        self.cam = (x - super::LEAD).clamp(0.0, max);
        feet
    }
}
