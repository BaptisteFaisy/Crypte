//! Kiro, gardien de la crypte : marche (`a`/`d`), saut (`w`, contrôle
//! réduit en l'air), s'enfoncer dans la terre (`s` : esquive, tokens bas,
//! `s`+`w` sur une dalle = la traverser), envoi de tokens (Espace),
//! linceul-armure, recul quand il est touché, mort.

use super::body::{Body, Rect};
use super::input::Act;
use super::level::Tile;
use super::shots::{Shot, Weapon, WORDS};
use super::{Haunt, DT, SUB};

/// Boîte de Kiro debout / enfoncé (pixels).
pub const W: f32 = 16.0;
pub const H: f32 = 24.0;
pub const SUNK: f32 = 14.0;
/// Pesanteur et chute maximale (pixels par pas de référence de 1/30 s).
pub const GRAVITY: f32 = 0.5;
pub const MAX_FALL: f32 = 7.0;
/// Durée de l'animation de mort (pas).
pub const DEATH: u16 = 70 * SUB as u16;
const SPEED: f32 = 2.0;
/// Accélération horizontale en l'air : on corrige à peine un saut.
const AIR: f32 = 0.16;
const JUMP: f32 = -7.4;
/// Saut accepté quelques pas après avoir quitté le bord / avant d'atterrir.
const COYOTE: u8 = 3 * SUB as u8;
const BUFFER: u64 = 4 * SUB as u64;
/// Invulnérabilité après avoir perdu le linceul (pas).
const INVULN: u16 = 50 * SUB as u16;
const COOLDOWN: u8 = 6 * SUB as u8;
/// Pas sans porter des dalles après `s`+`w`.
const DROP: u8 = 8 * SUB as u8;

#[derive(Debug, Clone, PartialEq)]
pub struct Kiro {
    pub body: Body,
    pub facing: f32,
    /// Linceul porté (armure : le prochain coup ne tue pas).
    pub shroud: bool,
    pub sunk: bool,
    /// Pas d'invulnérabilité restants (clignote).
    pub hurt: u16,
    /// Projeté : aucune commande jusqu'au sol.
    stun: bool,
    /// Pas depuis la mort (0 = vivant).
    pub dead: u16,
    pub weapon: Weapon,
    cooldown: u8,
    coyote: u8,
    drop: u8,
    /// Cercueil flottant porteur.
    pub riding: Option<usize>,
}

impl Kiro {
    /// Kiro debout, linceul sur le dos, centré en `x`, pieds en `feet`.
    pub fn spawn(x: f32, feet: f32, weapon: Weapon) -> Kiro {
        Kiro {
            body: Body::new(x, feet, W, H),
            facing: 1.0,
            shroud: true,
            sunk: false,
            hurt: 0,
            stun: false,
            dead: 0,
            weapon,
            cooldown: 0,
            coyote: 0,
            drop: 0,
            riding: None,
        }
    }

    pub fn alive(&self) -> bool {
        self.dead == 0
    }

    pub fn rect(&self) -> Rect {
        self.body.rect()
    }

    pub fn center(&self) -> (f32, f32) {
        let r = self.body.rect();
        (r.cx(), r.cy())
    }

    /// Départ des tokens : devant la bouche, plus bas enfoncé.
    pub fn muzzle(&self) -> (f32, f32) {
        let y = if self.sunk { 5.0 } else { 9.0 };
        (self.body.cx() + self.facing * 10.0, self.body.y + y)
    }

    /// S'enfonce (boîte basse) ou se relève s'il y a la place.
    fn sink(&mut self, down: bool, room: bool) {
        if down && !self.sunk {
            self.body.y += H - SUNK;
            self.body.h = SUNK;
            self.sunk = true;
        } else if !down && self.sunk && room {
            self.body.y -= H - SUNK;
            self.body.h = H;
            self.sunk = false;
        }
    }
}

impl Haunt {
    /// Un pas de Kiro : commandes, pesanteur, collisions, dangers.
    pub(super) fn move_kiro(&mut self) {
        if !self.kiro.alive() {
            self.kiro.dead += 1;
            self.kiro.body.y -= 0.8 * DT;
            return;
        }
        let k = &mut self.kiro;
        k.hurt = k.hurt.saturating_sub(1);
        k.cooldown = k.cooldown.saturating_sub(1);
        k.drop = k.drop.saturating_sub(1);
        if let Some(p) = k.riding.and_then(|i| self.platforms.get(i)) {
            k.body.x += p.dx;
        }
        self.steer();
        let k = &mut self.kiro;
        let old_feet = k.body.feet();
        k.body.fall(GRAVITY, MAX_FALL);
        k.body.travel(&self.level, k.drop > 0);
        self.ride_platforms(old_feet);
        let k = &mut self.kiro;
        if k.body.x < self.cam {
            k.body.x = self.cam;
        }
        if k.body.ground {
            k.stun = false;
            k.coyote = COYOTE;
        } else {
            k.coyote = k.coyote.saturating_sub(1);
        }
        self.hazards();
        if self.kiro.body.y > self.level.height() + 24.0 {
            self.kill_kiro("SWALLOWED BY THE DARK");
        }
    }

    /// Commandes : marche, s'enfoncer, traverser une dalle, sauter.
    fn steer(&mut self) {
        let now = self.step;
        let (k, input) = (&mut self.kiro, &mut self.input);
        if k.stun {
            return;
        }
        let dir = input.axis(now);
        if dir != 0.0 {
            k.facing = dir;
        }
        let down = input.held(Act::Down, now) && k.body.ground;
        let standing = Rect::new(k.body.x, k.body.feet() - H, W, H);
        let room = self.level.ceiling(&standing).is_none();
        k.sink(down, room);
        if k.sunk {
            k.body.vx = 0.0;
            let slab = self.level.at(k.body.cx(), k.body.feet() + 2.0) == Tile::Slab;
            if slab && input.take(Act::Up, now, BUFFER) {
                k.drop = DROP;
                k.sink(false, true);
            }
            return;
        }
        if k.body.ground {
            k.body.vx = dir * SPEED;
        } else {
            k.body.vx += (dir * SPEED - k.body.vx).clamp(-AIR * DT, AIR * DT);
        }
        if (k.body.ground || k.coyote > 0) && input.take(Act::Up, now, BUFFER) {
            k.body.vy = JUMP;
            k.body.ground = false;
            k.coyote = 0;
        }
    }

    /// Posé sur un cercueil flottant ? (Kiro suivra son va-et-vient.)
    fn ride_platforms(&mut self, old_feet: f32) {
        let k = &mut self.kiro;
        k.riding = None;
        if k.drop > 0 {
            return;
        }
        for (i, p) in self.platforms.iter().enumerate() {
            let r = p.rect();
            if k.body.ride(r.y, r.x, r.right(), old_feet) {
                k.riding = Some(i);
                k.coyote = COYOTE;
                break;
            }
        }
    }

    /// Ectoplasme : mort ; pieux d'os : coup (et recul).
    fn hazards(&mut self) {
        match self.level.hazard(&self.kiro.rect()) {
            Some(Tile::Ecto) => self.kill_kiro("DROWNED IN ECTOPLASM"),
            Some(Tile::Spikes) => {
                let from = self.kiro.body.cx() + self.kiro.facing;
                self.hurt_kiro(from);
            }
            _ => {}
        }
    }

    /// Espace : envoie un token si la limite en vol le permet.
    pub(super) fn fire(&mut self) {
        let k = &self.kiro;
        let w = k.weapon;
        if !k.alive() || k.cooldown > 0 || self.shots.len() >= w.max() {
            return;
        }
        if !self.input.take(Act::Fire, self.step, super::PRESS) {
            return;
        }
        let (x, y) = k.muzzle();
        let word = WORDS[self.dice.below(WORDS.len() as u64) as usize];
        self.shots.push(Shot::new(w, x, y, k.facing, word));
        self.kiro.cooldown = COOLDOWN;
        self.sent += 1;
    }

    /// Coup reçu venant de `from_x` : le linceul se déchire (recul,
    /// invulnérable un moment), sinon Kiro meurt.
    pub(super) fn hurt_kiro(&mut self, from_x: f32) {
        if !self.kiro.alive() || self.kiro.hurt > 0 {
            return;
        }
        if !self.kiro.shroud {
            return self.kill_kiro("TORN TO SHREDS");
        }
        let k = &mut self.kiro;
        k.shroud = false;
        k.hurt = INVULN;
        k.stun = true;
        k.sink(false, true);
        let away = if k.body.cx() >= from_x { 1.0 } else { -1.0 };
        k.body.vx = away * 2.6;
        k.body.vy = -4.5;
        k.body.ground = false;
        let (x, y) = k.center();
        self.sparks.rags(x, y, &mut self.dice);
        self.shake = 6 * SUB as u8;
        self.say("THE SHROUD IS TORN");
    }

    /// Mort de Kiro (`why` s'affiche sous « YOU ARE DUST »).
    pub(super) fn kill_kiro(&mut self, why: &'static str) {
        if !self.kiro.alive() {
            return;
        }
        let k = &mut self.kiro;
        k.dead = 1;
        k.shroud = false;
        k.sink(false, true);
        k.body.vx = 0.0;
        k.body.vy = 0.0;
        let (x, y) = k.center();
        self.sparks.soul(x, y, &mut self.dice);
        self.sparks.rags(x, y, &mut self.dice);
        self.cause = why;
        self.shake = 10 * SUB as u8;
        self.flash = 3 * SUB as u8;
    }
}
