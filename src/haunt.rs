//! Haunt (F9) : jeu de plateforme et de tir dans le style du bestiaire.
//! Kiro, gardien de la crypte, traverse le cimetière (niveau 1, « The
//! Boneyard », très difficile) en **envoyant des tokens** (pastilles de
//! texte) sur les créatures du bestiaire, jusqu'à la Faucheuse.
//!
//! - Commandes : `a`/`d` gauche/droite, `w` saut, `s` s'enfoncer (esquive,
//!   tir bas ; `s`+`w` traverse une dalle), Espace = token, Entrée,
//!   `p` pause (flèches aussi, voir `app/haunt.rs`).
//! - Linceul = armure : le premier coup l'arrache, le second tue ; 3 vies,
//!   une reprise à mi-niveau, 4:00 avant que la bougie ne s'éteigne.
//! - « 32 bits » : canevas 960×576, parallaxe, ciel en bandes, éclairage
//!   (`backdrop.rs`, `draw.rs`) ; mini-sprites (6 à 18 px) dans le style
//!   du bestiaire, dalles de 6 px : un grand niveau de plateformes.
//! - Simulation à **240 Hz** (`pump`, pas fixe de 1/240 s : physique,
//!   entrées, IA), rendu cadencé jusqu'à 240 images/s ; vitesses et
//!   durées écrites en pas de référence de 1/30 s (`SUB`, `DT`).
//! - Dé à graine fixe : parties rejouables.

mod actors;
mod backdrop;
mod body;
mod boss;
mod combat;
mod demo;
mod dice;
pub mod draw;
mod foes;
mod hud;
pub mod input;
mod level;
mod player;
mod shots;
mod sparks;
mod spawn;
mod sprites;
#[cfg(test)]
mod tests;

use std::time::Duration;

use crossterm::event::KeyEventKind;

pub use self::demo::write_shot;
pub use self::input::Act;

use self::boss::Reaper;
use self::dice::Dice;
use self::draw::Painter;
use self::foes::Foe;
use self::input::Input;
use self::level::Level;
use self::player::{Kiro, DEATH};
use self::shots::{Bolt, Flame, Pickup, Platform, Shot, Weapon};
use self::sparks::Sparks;
use self::sprites::Sprites;
use crate::hub::live::View;

/// Canevas (pixels d'art).
pub const VIEW_W: u32 = 960;
pub const VIEW_H: u32 = 576;
/// Pas de simulation par seconde.
pub const HZ: u32 = 240;
/// Vitesses et durées sont écrites en pas de référence de 1/30 s :
/// `SUB` pas réels par pas de référence, `DT` = sa fraction par pas réel.
pub const SUB: u32 = HZ / 30;
pub const DT: f32 = 1.0 / SUB as f32;
/// Durée d'un pas de simulation (1/240 s).
pub const STEP: Duration = Duration::from_nanos(1_000_000_000 / HZ as u64);
/// Pas rattrapés au plus d'un coup (au-delà, le temps est perdu).
const CATCH_UP: u32 = 32;
/// Vies au départ.
const LIVES: u8 = 3;
/// Bougie : 4 min par vie (en pas), le niveau fait six écrans.
const CANDLE: u32 = 240 * HZ;
/// Pas avant d'accepter Entrée (fin de partie) : pas de saut involontaire.
const SETTLE: u16 = 45 * SUB as u16;
/// Durée d'un message (pas).
const TOAST: u16 = 60 * SUB as u16;
/// Fenêtre d'un appui (pas) : un appui reste valable ~0,1 s.
const PRESS: u64 = 3 * SUB as u64;
/// Kiro se tient à un tiers de la vue (on voit loin devant).
const LEAD: f32 = VIEW_W as f32 / 3.0;
/// Graine du dé (parties identiques à commandes identiques).
const SEED: u64 = 0x5EED_C0FF_1A57_B0E5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Title,
    Play,
    Paused,
    Over,
    Clear,
}

pub struct Haunt {
    pub phase: Phase,
    level: Level,
    kiro: Kiro,
    foes: Vec<Foe>,
    boss: Option<Reaper>,
    shots: Vec<Shot>,
    bolts: Vec<Bolt>,
    flames: Vec<Flame>,
    pickups: Vec<Pickup>,
    platforms: Vec<Platform>,
    sparks: Sparks,
    /// Repères pas encore déclenchés (même ordre que `level.marks`).
    armed: Vec<bool>,
    /// Bord gauche de la vue (pixels du monde) ; ne recule jamais.
    cam: f32,
    /// Arène verrouillée (bord gauche) pendant le combat final.
    arena: Option<f32>,
    step: u64,
    pub score: u32,
    best: u32,
    pub lives: u8,
    candle: u32,
    /// Tokens envoyés pendant la partie.
    pub sent: u32,
    /// Abscisse de reprise après une mort.
    checkpoint: f32,
    toast: Option<(&'static str, u16)>,
    cause: &'static str,
    shake: u8,
    flash: u8,
    settle: u16,
    director: u16,
    dice: Dice,
    acc: Duration,
    input: Input,
    pub view: View,
    sprites: Sprites,
    painter: Painter,
}

impl Haunt {
    /// Écran titre devant le début du niveau 1.
    pub fn new() -> Self {
        let level = Level::first();
        let start = level.mark('P').map_or((24.0, 144.0), |m| (m.x, m.y));
        let mut game = Self {
            phase: Phase::Title,
            kiro: Kiro::spawn(start.0, start.1, Weapon::Token),
            armed: Vec::new(),
            level,
            foes: Vec::new(),
            boss: None,
            shots: Vec::new(),
            bolts: Vec::new(),
            flames: Vec::new(),
            pickups: Vec::new(),
            platforms: Vec::new(),
            sparks: Sparks::default(),
            cam: 0.0,
            arena: None,
            step: 0,
            score: 0,
            best: 0,
            lives: LIVES,
            candle: CANDLE,
            sent: 0,
            checkpoint: start.0,
            toast: None,
            cause: "",
            shake: 0,
            flash: 0,
            settle: 0,
            director: 0,
            dice: Dice::new(SEED),
            acc: Duration::ZERO,
            input: Input::default(),
            view: View::new(),
            sprites: Sprites::load(),
            painter: Painter::default(),
        };
        game.new_run();
        game.phase = Phase::Title;
        game
    }

    /// Nouvelle partie : score, vies, armes et repères remis à zéro.
    fn new_run(&mut self) {
        self.score = 0;
        self.sent = 0;
        self.lives = LIVES;
        self.checkpoint = self.level.mark('P').map_or(24.0, |m| m.x);
        self.armed = self
            .level
            .marks
            .iter()
            .map(|m| spawn::armed(m.ch))
            .collect();
        self.kiro.weapon = Weapon::Token;
        self.dice = Dice::new(SEED);
        self.respawn();
        self.phase = Phase::Play;
    }

    /// Avance le temps réel `dt` par pas fixes de 1/240 s (`CATCH_UP` au
    /// plus d'un coup) ; vrai si au moins un pas a tourné (à redessiner).
    pub fn pump(&mut self, dt: Duration) -> bool {
        self.acc += dt.min(Duration::from_millis(250));
        let mut steps = 0;
        while self.acc >= STEP && steps < CATCH_UP {
            self.acc -= STEP;
            self.tick();
            steps += 1;
        }
        if steps == CATCH_UP {
            self.acc = Duration::ZERO;
        }
        steps > 0
    }

    /// Touche de jeu reçue (appui, répétition, relâchement).
    pub fn feed(&mut self, act: Act, kind: KeyEventKind) {
        self.input.feed(act, kind, self.step);
    }

    /// Le terminal signale-t-il les relâchements (protocole Kitty) ?
    pub fn set_releases(&mut self, on: bool) {
        self.input.set_releases(on);
    }

    /// Quitter l'écran : la partie se met en pause, les touches s'oublient.
    pub fn pause(&mut self) {
        if self.phase == Phase::Play {
            self.phase = Phase::Paused;
        }
        self.input.clear();
    }

    /// F5 dans le jeu : éclair et secousse.
    pub fn thunder(&mut self) {
        self.flash = 4 * SUB as u8;
        self.shake = 6 * SUB as u8;
    }

    /// Un pas de simulation.
    pub fn tick(&mut self) {
        self.step += 1;
        self.shake = self.shake.saturating_sub(1);
        self.flash = self.flash.saturating_sub(1);
        self.settle = self.settle.saturating_sub(1);
        if let Some((_, ttl)) = &mut self.toast {
            *ttl = ttl.saturating_sub(1);
        }
        self.toast = self.toast.filter(|(_, ttl)| *ttl > 0);
        match self.phase {
            Phase::Title => self.wait_start(),
            Phase::Play => self.play(),
            Phase::Paused => {
                let now = self.step;
                if self.input.take(Act::Pause, now, PRESS)
                    || self.input.take(Act::Start, now, PRESS)
                {
                    self.phase = Phase::Play;
                }
            }
            Phase::Over | Phase::Clear => {
                if self.settle == 0 {
                    self.wait_start();
                }
            }
        }
        self.sparks.step();
        self.weather();
    }

    /// Entrée (ou un token) lance une nouvelle partie.
    fn wait_start(&mut self) {
        let now = self.step;
        let go = self.input.take(Act::Start, now, PRESS) | self.input.take(Act::Fire, now, PRESS);
        if go {
            self.new_run();
        }
    }

    /// Un pas de jeu.
    fn play(&mut self) {
        if self.input.take(Act::Pause, self.step, PRESS) {
            self.phase = Phase::Paused;
            return;
        }
        for p in &mut self.platforms {
            p.slide();
        }
        self.move_kiro();
        self.fire();
        self.follow();
        self.wake();
        self.direct();
        self.reach_checkpoint();
        self.open_gate();
        self.run_foes();
        self.run_boss();
        self.fly_shots();
        self.fly_bolts();
        self.burn();
        self.touch();
        self.clock();
    }

    /// Vue : suit Kiro (un tiers d'écran devant lui), ne recule jamais ;
    /// glisse sur l'arène quand elle se ferme.
    fn follow(&mut self) {
        let max = (self.level.width() - VIEW_W as f32).max(0.0);
        let want = (self.kiro.body.cx() - LEAD).clamp(0.0, max);
        self.cam = match self.arena {
            Some(x) => self.cam + (x.min(max) - self.cam) * 0.15 * DT,
            None => self.cam.max(want),
        };
    }

    /// Bougie qui se consume ; fin de l'agonie de Kiro (vie perdue).
    fn clock(&mut self) {
        if self.kiro.alive() {
            self.candle = self.candle.saturating_sub(1);
            if self.candle == 0 {
                self.kill_kiro("THE CANDLE BURNT OUT");
            }
        } else if self.kiro.dead >= DEATH {
            self.lose_life();
        }
    }

    /// Une vie de moins : reprise au dernier point, ou fin de partie.
    fn lose_life(&mut self) {
        self.lives = self.lives.saturating_sub(1);
        if self.lives == 0 {
            self.best = self.best.max(self.score);
            self.phase = Phase::Over;
            self.settle = SETTLE;
            return;
        }
        self.respawn();
    }

    /// La Faucheuse est tombée : bonus de bougie, écran de victoire.
    fn win(&mut self) {
        self.score += self.candle / HZ * 50;
        self.best = self.best.max(self.score);
        self.phase = Phase::Clear;
        self.settle = SETTLE;
    }

    /// Message bref au centre de l'écran.
    fn say(&mut self, text: &'static str) {
        self.toast = Some((text, TOAST));
    }

    /// Éclairs de temps en temps (le ciel blanchit un instant).
    fn weather(&mut self) {
        if self.dice.below(260 * u64::from(SUB)) == 0 {
            self.flash = self.flash.max(2 * SUB as u8);
        }
    }
}

impl Default for Haunt {
    fn default() -> Self {
        Self::new()
    }
}
