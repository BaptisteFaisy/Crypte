//! Apparitions : repères du niveau qui entrent dans la vue (créatures
//! placées, cercueils et leur butin), « directeur » qui fait surgir des
//! créatures sans fin selon la zone, point de reprise, porte de l'arène,
//! reprise après une mort.

use super::boss::Reaper;
use super::foes::{Foe, Kind};
use super::level::{Zone, TILE};
use super::player::Kiro;
use super::shots::{Loot, Platform};
use super::{Haunt, CANDLE, SUB, VIEW_W};

/// Butin des cercueils, dans l'ordre du niveau (`None` = piège).
const COFFINS: [Option<Loot>; 4] = [Some(Loot::Sharp), Some(Loot::Shroud), Some(Loot::Hot), None];
/// Créatures du directeur en même temps au plus, par zone.
const MAX_MUMMIES: usize = 3;
const MAX_WISPS: usize = 4;
const MAX_BATS: usize = 2;
const MAX_HEADS: usize = 2;

/// Le repère `ch` fait-il apparaître quelque chose en entrant dans la vue ?
pub fn armed(ch: char) -> bool {
    "Chbmlg".contains(ch)
}

impl Haunt {
    /// Repères qui entrent dans la vue : la créature ou l'objet apparaît.
    pub(super) fn wake(&mut self) {
        let edge = self.cam + VIEW_W as f32 + 24.0;
        let mut coffins = 0;
        for i in 0..self.level.marks.len() {
            let m = self.level.marks[i];
            if m.ch == 'C' {
                coffins += 1;
            }
            if !self.armed[i] || m.x > edge {
                continue;
            }
            self.armed[i] = false;
            match m.ch {
                'h' => self.foes.push(Foe::new(Kind::Hand, m.x, m.y)),
                'b' => self.foes.push(Foe::new(Kind::Bat, m.x, m.y)),
                'm' => self.foes.push(Foe::new(Kind::Runner, m.x, m.y)),
                'l' => {
                    let mut lady = Foe::new(Kind::Lady, m.x, m.y);
                    lady.age = 1;
                    self.foes.push(lady);
                }
                'g' => {
                    for k in 0..3 {
                        let (dx, dy) = (k as f32 * 8.0, k as f32 * 6.0 - 6.0);
                        self.foes.push(Foe::new(Kind::Wisp, m.x + dx, m.y + dy));
                    }
                }
                'C' => {
                    let mut coffin = Foe::new(Kind::Coffin, m.x, m.y);
                    coffin.loot = COFFINS.get(coffins - 1).copied().flatten();
                    self.foes.push(coffin);
                }
                _ => {}
            }
        }
    }

    /// Directeur : selon la zone sous Kiro, des créatures surgissent sans
    /// fin (c'est le niveau 1, et il est très difficile).
    pub(super) fn direct(&mut self) {
        if self.arena.is_some() || !self.kiro.alive() {
            return;
        }
        self.director = self.director.saturating_sub(1);
        if self.director > 0 {
            return;
        }
        let (zone, pause) = (
            self.level.zone(self.kiro.body.cx()),
            self.dice.below(40) as u16,
        );
        let wait = match zone {
            Zone::Calm => 30,
            Zone::Mummies => {
                if self.count(Kind::Mummy) < MAX_MUMMIES {
                    self.raise_mummy();
                }
                38 + pause
            }
            Zone::Ghosts => {
                if self.count(Kind::Wisp) < MAX_WISPS {
                    let y = self.kiro.body.y + self.dice.range(-90.0, 40.0);
                    let x = self.cam + VIEW_W as f32 + 8.0;
                    self.foes.push(Foe::new(Kind::Wisp, x, y));
                }
                56 + pause
            }
            Zone::Bats => {
                if self.count(Kind::Bat) < MAX_BATS {
                    let mut bat = Foe::new(Kind::Bat, self.cam + VIEW_W as f32 + 12.0, 0.0);
                    bat.base = self.kiro.body.y + self.dice.range(-50.0, 10.0);
                    bat.awake = true;
                    self.foes.push(bat);
                }
                70 + pause
            }
            Zone::Heads => {
                if self.count(Kind::Head) < MAX_HEADS {
                    let x = self.cam + VIEW_W as f32 + 10.0;
                    let y = self.kiro.body.y - 40.0;
                    self.foes.push(Foe::new(Kind::Head, x, y));
                }
                64 + pause
            }
        };
        self.director = wait * SUB as u16;
    }

    /// Créatures de genre `kind` présentes.
    fn count(&self, kind: Kind) -> usize {
        self.foes.iter().filter(|f| f.kind == kind).count()
    }

    /// Une momie sort de terre à quelques pas de Kiro, dans la vue, sur
    /// un sol plein (jamais au-dessus du vide ni de l'ectoplasme).
    fn raise_mummy(&mut self) {
        let side = self.dice.sign();
        let x = self.kiro.body.cx() + side * self.dice.range(30.0, 110.0);
        let x = x.clamp(self.cam + 8.0, self.cam + VIEW_W as f32 - 8.0);
        if (x - self.kiro.body.cx()).abs() < 20.0 {
            return;
        }
        let feet = self.kiro.body.feet();
        let ground = self.level.ground_below(x, feet - 30.0);
        if let Some(g) = ground.filter(|g| *g < feet + 60.0) {
            self.foes.push(Foe::new(Kind::Mummy, x, g));
        }
    }

    /// Kiro dépasse une lanterne : nouveau point de reprise.
    pub(super) fn reach_checkpoint(&mut self) {
        let x = self.kiro.body.cx();
        let passed = self
            .level
            .marks
            .iter()
            .filter(|m| m.ch == 'k' && m.x < x && m.x > self.checkpoint)
            .map(|m| m.x)
            .last();
        if let Some(at) = passed {
            self.checkpoint = at;
            self.say("CHECKPOINT");
        }
    }

    /// Kiro entre dans l'arène : la vue se ferme, la Faucheuse descend.
    pub(super) fn open_gate(&mut self) {
        if self.arena.is_some() {
            return;
        }
        let Some(gate) = self.level.mark('R') else {
            return;
        };
        if self.kiro.body.cx() > gate.x + 24.0 {
            let left = gate.x - TILE / 2.0;
            self.arena = Some(left);
            self.boss = Some(Reaper::new(left));
            self.foes.retain(|f| f.kind == Kind::Coffin);
            self.say("THE REAPER COMES");
        }
    }

    /// Kiro renaît au dernier point de reprise : linceul neuf, bougie
    /// neuve, créatures balayées, repères suivants réarmés.
    pub(super) fn respawn(&mut self) {
        let x = self.checkpoint;
        let mark = self.level.marks.iter().find(|m| (m.x - x).abs() < 0.5);
        let feet = mark
            .map(|m| m.y)
            .or_else(|| self.level.ground_below(x, 0.0));
        let feet = feet.unwrap_or(self.level.height());
        let weapon = self.kiro.weapon;
        self.kiro = Kiro::spawn(x, feet, weapon);
        for (i, m) in self.level.marks.iter().enumerate() {
            if m.x > x {
                self.armed[i] = armed(m.ch);
            }
        }
        self.foes.clear();
        self.boss = None;
        self.arena = None;
        self.shots.clear();
        self.bolts.clear();
        self.flames.clear();
        self.pickups.clear();
        self.sparks.clear();
        self.platforms = (self.level.marks.iter())
            .filter(|m| m.ch == 'H')
            .map(|m| Platform::new(m.x, m.y))
            .collect();
        self.candle = CANDLE;
        self.director = 45 * SUB as u16;
        let max = (self.level.width() - VIEW_W as f32).max(0.0);
        self.cam = (x - super::LEAD).clamp(0.0, max);
    }
}
