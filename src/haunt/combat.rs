//! Combat : tokens contre créatures, cercueils et Faucheuse ; projectiles
//! ennemis, contacts et flammes contre Kiro ; butin ramassé ; créatures
//! et Faucheuse qui agissent.

use super::boss::{Deed, DYING};
use super::foes::{Foe, Kind};
use super::level::TILE;
use super::shots::{Flame, Loot, Pickup, Weapon};
use super::{Haunt, SUB, VIEW_W};
use crate::theme;

impl Haunt {
    /// Créatures : un pas d'IA, projectiles, ménage hors de la vue.
    pub(super) fn run_foes(&mut self) {
        let prey = self.kiro.center();
        for f in &mut self.foes {
            let was = f.rise;
            if let Some(b) = f.think(prey, &self.level, &mut self.dice) {
                self.bolts.push(b);
            }
            if was == super::foes::RISE && f.rise < was {
                self.sparks.dust(f.body.cx(), f.body.feet(), &mut self.dice);
            }
        }
        let (lo, hi) = (self.cam - 64.0, self.cam + VIEW_W as f32 + 240.0);
        let deep = self.level.height() + 40.0;
        self.foes
            .retain(|f| !f.gone && f.body.x + f.body.w > lo && f.body.x < hi && f.body.y < deep);
    }

    /// Faucheuse : un pas, ses faux, ses fantômes, son agonie.
    pub(super) fn run_boss(&mut self) {
        let prey = self.kiro.center();
        let ground = 9.0 * TILE;
        let Some(boss) = &mut self.boss else {
            return;
        };
        match boss.think(prey, ground, &mut self.dice) {
            Some(Deed::Throw(bolt)) => self.bolts.push(bolt),
            Some(Deed::Summon(x, y)) => {
                for dx in [-22.0, 22.0] {
                    self.foes.push(Foe::new(Kind::Wisp, x + dx, y));
                }
            }
            None => {}
        }
        if boss.dying > 0 && u32::from(boss.dying) % (5 * SUB) == 0 {
            let r = boss.rect();
            let at = (
                r.x + self.dice.range(0.0, r.w),
                r.y + self.dice.range(0.0, r.h),
            );
            let fire = [theme::CANDLE, theme::PUMPKIN, theme::BLOOD, theme::BONE];
            self.sparks.burst(at, 10, &fire, &mut self.dice);
            self.shake = 4 * SUB as u8;
        }
        if boss.dying >= DYING {
            self.boss = None;
            self.win();
        }
    }

    /// Tokens en vol : avancent, touchent, s'éteignent (les brûlants
    /// laissent une flamme).
    pub(super) fn fly_shots(&mut self) {
        let mut i = 0;
        while i < self.shots.len() {
            let alive = self.shots[i].fly(&self.level);
            let spent = !alive || self.shot_hits(i);
            if !spent {
                i += 1;
                continue;
            }
            let s = self.shots.swap_remove(i);
            if s.weapon == Weapon::Hot {
                self.flames.push(Flame::new(s.x, s.y, &self.level));
            } else {
                self.sparks.spark(s.x, s.y, &mut self.dice);
            }
        }
    }

    /// Le token `i` touche-t-il une créature, un cercueil, la Faucheuse ?
    /// Vrai s'il s'arrête (les tranchants traversent les créatures).
    fn shot_hits(&mut self, i: usize) -> bool {
        let (r, w) = (self.shots[i].rect(), self.shots[i].weapon);
        for j in 0..self.foes.len() {
            let f = &self.foes[j];
            if !f.vulnerable() || !f.body.rect().hits(&r) || (w.pierce() && f.flash > 0) {
                continue;
            }
            self.strike(j, w.damage());
            if !w.pierce() {
                return true;
            }
        }
        let Some(boss) = &mut self.boss else {
            return false;
        };
        if !boss.vulnerable() || !boss.rect().hits(&r) {
            return false;
        }
        if boss.hurt(w.damage()) {
            self.score += 5000;
            self.shake = 12 * SUB as u8;
        }
        self.sparks.spark(r.cx(), r.cy(), &mut self.dice);
        true
    }

    /// Blesse la créature `j` ; détruite : points, éclats, âme ; un
    /// cercueil s'ouvre sur son butin (ou sur une momie pressée).
    pub(super) fn strike(&mut self, j: usize, dmg: i16) {
        let f = &mut self.foes[j];
        f.hp -= dmg;
        f.flash = 4 * SUB as u8;
        let (at, kind, loot) = (f.center(), f.kind, f.loot);
        if f.hp > 0 {
            self.sparks.spark(at.0, at.1, &mut self.dice);
            return;
        }
        if kind == Kind::Coffin {
            f.open = true;
            let feet = f.body.feet();
            self.sparks.dust(at.0, feet, &mut self.dice);
            match loot {
                Some(loot) => self.pickups.push(Pickup::new(loot, at.0, feet - 8.0)),
                None => {
                    let mut runner = Foe::new(Kind::Runner, at.0, feet);
                    runner.facing = if self.kiro.body.cx() < at.0 {
                        -1.0
                    } else {
                        1.0
                    };
                    self.foes.push(runner);
                    self.say("SOMEBODY GOT OUT");
                }
            }
            return;
        }
        f.gone = true;
        self.score += kind.score();
        self.sparks.burst(at, 16, &palette(kind), &mut self.dice);
        self.sparks.soul(at.0, at.1, &mut self.dice);
        if kind == Kind::Lady {
            // Des bonbons ou un sort : la Dame blanche laisse une sucette.
            let feet = self.foes[j].body.feet();
            self.pickups
                .push(Pickup::new(Loot::Candy, at.0, feet - 8.0));
        }
    }

    /// Projectiles ennemis : avancent, touchent Kiro, quittent la vue.
    pub(super) fn fly_bolts(&mut self) {
        let r = self.kiro.rect();
        let alive = self.kiro.alive();
        let (lo, hi) = (self.cam - 32.0, self.cam + VIEW_W as f32 + 32.0);
        let mut hit = None;
        self.bolts.retain_mut(|b| {
            b.fly();
            if alive && b.rect().hits(&r) {
                hit = Some(b.x);
                return false;
            }
            b.x > lo && b.x < hi && b.y > -48.0 && b.y < 240.0 && u32::from(b.age) < 400 * SUB
        });
        if let Some(x) = hit {
            self.hurt_kiro(x);
        }
    }

    /// Flammes des tokens brûlants : 1 dégât tous les 5 pas à ce qui est
    /// dedans (créatures, Faucheuse).
    pub(super) fn burn(&mut self) {
        for k in 0..self.flames.len() {
            self.flames[k].age += 1;
            if u32::from(self.flames[k].age) % (5 * SUB) != 1 {
                continue;
            }
            let r = self.flames[k].rect();
            for j in 0..self.foes.len() {
                if self.foes[j].vulnerable() && self.foes[j].body.rect().hits(&r) {
                    self.strike(j, 1);
                }
            }
            if let Some(boss) = &mut self.boss {
                if boss.rect().hits(&r) && boss.hurt(1) {
                    self.score += 5000;
                }
            }
        }
        self.flames.retain(|f| f.age < Flame::LIFE);
    }

    /// Contacts : créature ou Faucheuse touchée = coup ; butin ramassé.
    pub(super) fn touch(&mut self) {
        for p in &mut self.pickups {
            p.fall(&self.level);
        }
        self.pickups.retain(|p| p.age < Pickup::LIFE);
        if !self.kiro.alive() {
            return;
        }
        let r = self.kiro.rect();
        let foe = self
            .foes
            .iter()
            .find(|f| f.harmful() && f.body.rect().hits(&r));
        let reaper = self
            .boss
            .as_ref()
            .filter(|b| b.harmful() && b.rect().hits(&r));
        if let Some(x) = foe.map(|f| f.body.cx()).or(reaper.map(|b| b.rect().cx())) {
            self.hurt_kiro(x);
        }
        let mut got = Vec::new();
        self.pickups.retain(|p| {
            let take = p.rect().hits(&r);
            if take {
                got.push(p.loot);
            }
            !take
        });
        for loot in got {
            self.grab(loot);
        }
    }

    /// Butin ramassé.
    fn grab(&mut self, loot: Loot) {
        self.score += 200;
        let (x, y) = self.kiro.center();
        self.sparks.spark(x, y, &mut self.dice);
        match loot {
            Loot::Sharp => {
                self.kiro.weapon = Weapon::Sharp;
                self.say("SHARP TOKENS");
            }
            Loot::Hot => {
                self.kiro.weapon = Weapon::Hot;
                self.say("HOT TOKENS");
            }
            Loot::Shroud => {
                self.kiro.shroud = true;
                self.say("SHROUD MENDED");
            }
            Loot::Candy => {
                self.score += 800;
                self.say("+1000");
            }
        }
    }
}

/// Couleurs des éclats d'une créature (celles de son sprite).
fn palette(kind: Kind) -> [ratatui::style::Color; 3] {
    match kind {
        Kind::Mummy | Kind::Runner => [theme::BONE, theme::SAND, theme::UMBER],
        Kind::Bat => [theme::INK, theme::BONE, theme::CANDLE],
        Kind::Hand | Kind::Head => [theme::ECTO, theme::ECTO_DARK, theme::BLOOD],
        Kind::Wisp => [theme::SPECTRE_DARK, theme::SPECTRE, theme::INK],
        Kind::Lady => [theme::BONE, theme::BLOOD, theme::INK],
        Kind::Coffin => [theme::UMBER, theme::BONE, theme::BLOOD],
    }
}
