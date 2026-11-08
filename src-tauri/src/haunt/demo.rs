//! Scènes de démonstration (ex-`--haunt-shot <png>` TUI : la planche
//! PNG part avec le rendu ; les scénarios — titre, cimetière, marais,
//! Faucheuse — servent la préview Tauri via [`Haunt::demo_scene`]).

use super::foes::{Foe, Kind};
use super::input::{Act, KeyKind};
use super::shots::Weapon;
use super::{FrameData, Haunt, Phase, SUB, VIEW_W};

impl Haunt {
    /// Joue la scène `scene` (0 = titre, 1 = cimetière, 2 = marais,
    /// 3 = arène Faucheuse) et rend sa frame (préview Tauri).
    pub fn demo_scene(&mut self, scene: u8) -> FrameData {
        match scene {
            1 => self.graveyard(),
            2 => self.swamp(),
            3 => self.arena_fight(),
            _ => self.run(20),
        }
        self.frame()
    }
    /// `n` pas de référence (1/30 s) ; Kiro, invulnérable, envoie un token
    /// de temps en temps.
    fn run(&mut self, n: u32) {
        for i in 0..n * SUB {
            if self.phase == Phase::Play {
                self.kiro.hurt = self.kiro.hurt.max(2);
                if i % (9 * SUB) == 0 {
                    self.input.feed(Act::Fire, KeyKind::Press, self.step);
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
