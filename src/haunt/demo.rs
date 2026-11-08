//! Planche de démonstration (`--haunt-shot <png>`) : 4 frames du jeu
//! agrandies ×2 (titre, cimetière, marais, Faucheuse), sans terminal.
//! À regénérer après une retouche de sprite, de tuile ou du niveau.

use anyhow::{Context, Result};
use crossterm::event::KeyEventKind;
use image::{imageops, RgbaImage};

use super::draw::compose;
use super::foes::{Foe, Kind};
use super::input::Act;
use super::shots::Weapon;
use super::{Haunt, Phase, SUB, VIEW_H, VIEW_W};

/// Agrandissement de la planche.
const ZOOM: u32 = 2;

/// Écrit la planche PNG (2×2 frames ×2) dans `path`.
pub fn write_shot(path: &str) -> Result<()> {
    let mut game = Haunt::new();
    let mut frames = Vec::with_capacity(4);
    game.run(20);
    frames.push(compose(&game));
    game.graveyard();
    frames.push(compose(&game));
    game.swamp();
    frames.push(compose(&game));
    game.arena_fight();
    frames.push(compose(&game));
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

    /// Kiro au cœur du cimetière : momies qui sortent de terre, main de
    /// tombe, chauve-souris, tokens en vol.
    fn graveyard(&mut self) {
        self.new_run();
        self.teleport(330.0);
        for (x, rise) in [(392.0, 0), (436.0, 9), (268.0, 0)] {
            let mut f = Foe::new(Kind::Mummy, x, 144.0);
            f.rise = rise * SUB as u8;
            self.foes.push(f);
        }
        let mut bat = Foe::new(Kind::Bat, 420.0, 80.0);
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
        self.teleport(2140.0);
        let mut lady = Foe::new(Kind::Lady, 2230.0, 144.0);
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
        self.teleport(gate + 60.0);
        self.run(150);
    }

    /// Kiro posé au sol en `x`, vue recentrée.
    fn teleport(&mut self, x: f32) {
        let feet = self.level.ground_below(x, 0.0).unwrap_or(144.0);
        self.kiro.body.x = x - self.kiro.body.w / 2.0;
        self.kiro.body.y = feet - self.kiro.body.h;
        let max = self.level.width() - VIEW_W as f32;
        self.cam = (x - 120.0).clamp(0.0, max);
    }
}
