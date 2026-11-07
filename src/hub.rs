//! Hub des esprits (F7) : tous les agents (chats, fantômes blancs) et leurs
//! sous-agents (`/agent`, petits fantômes noirs) vivent dans une crypte.
//!
//! - Agent au repos : erre, croise les autres et bavarde (bulles) ; en
//!   attente du LLM : flotte sur place, bulle « … ».
//! - Sous-agent en cours : tourne autour de son agent, relié par un fil
//!   d'ectoplasme ; terminé : s'endort à ses pieds ; échoué : coule, yeux
//!   rouges.
//! - Messages réels (`/tell`, `/ask`, livraison d'un sous-agent) : un orbe
//!   vole de l'émetteur au destinataire, qui sursaute.
//! - F5 : tout le monde fuit.
//!
//! Simulation en **dalles** d'une crypte carrée vue en isométrique
//! (`hub/iso.rs` projette, `hub/render.rs` dessine), déterministe (graine
//! fixe) pour les tests.

mod font;
pub mod iso;
pub mod live;
pub mod render;
mod social;

use crate::sessions::{AgentStatus, Sessions};

/// Côté de la crypte (dalles) avant le premier rendu.
const ROOM: f32 = 10.0;
/// Marge aux murs (dalles).
const WALL_GAP: f32 = 0.6;
/// Vitesses (dalles par tick de 100 ms).
const WANDER: f32 = 0.035;
const ORBIT: f32 = 0.11;
const FLEE: f32 = 0.16;
/// Rayon d'orbite d'un sous-agent autour de son agent (dalles).
const ORBIT_R: f32 = 1.15;
/// Distance de rencontre de deux agents (dalles).
const MEET_DIST: f32 = 2.2;
/// Durées (ticks).
const MEET: u16 = 40;
const MEET_COOLDOWN: u16 = 220;
const PANIC: u16 = 22;
const BUBBLE: u16 = 18;
/// Avancée d'un orbe par tick (arrivée en ~30 ticks).
const ORB_SPEED: f32 = 0.034;

/// Identité d'un esprit : agent = index de chat, sous-agent = id d'agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Agent(usize),
    Sub(usize),
}

/// Humeur, tirée de l'état réel du chat / sous-agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Idle,
    Waiting,
    Running,
    Done,
    Failed,
}

/// Contenu d'une bulle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Talk,
    Alert,
    Ask,
    Sleep,
    Love,
}

/// Un fantôme du hub (position au sol, en dalles).
#[derive(Debug, Clone)]
pub struct Spectre {
    pub key: Key,
    pub owner: usize,
    pub x: f32,
    pub y: f32,
    vx: f32,
    vy: f32,
    goal: (f32, f32),
    pub left: bool,
    pub mood: Mood,
    pub fade: u8,
    pub jump: u8,
    meet: u16,
    partner: Option<Key>,
    cooldown: u16,
}

/// Bulle au-dessus d'un esprit.
#[derive(Debug, Clone, Copy)]
pub struct Bubble {
    pub key: Key,
    pub glyph: Glyph,
    pub ttl: u16,
}

/// Orbe-message en vol vers `to` (`t` de 0 à 1) ; la traînée garde
/// (x, y, hauteur 0..1) en dalles.
#[derive(Debug, Clone)]
pub struct Orb {
    pub from: (f32, f32),
    pub to: Key,
    pub t: f32,
    pub on_arrival: Glyph,
    pub trail: Vec<(f32, f32, f32)>,
}

/// Événement réel venu des commandes et des réponses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// `/tell` : le chat `from` écrit au chat `to`.
    Tell { from: usize, to: usize },
    /// `/ask` : le chat `from` relance le sous-agent `agent`.
    Ask { from: usize, agent: usize },
    /// Un sous-agent livre sa réponse au chat `to`.
    Deliver { agent: usize, to: usize },
    /// Le LLM a répondu au chat `chat`.
    Reply { chat: usize },
}

/// Le hub : esprits, orbes, bulles, sélection.
pub struct Hub {
    pub spectres: Vec<Spectre>,
    pub orbs: Vec<Orb>,
    pub bubbles: Vec<Bubble>,
    pub selected: usize,
    pub tick: u64,
    pub sprites: render::Sprites,
    pub view: live::View,
    room: f32,
    panic: u16,
    seed: u64,
}

impl Hub {
    /// Hub vide (crypte de 10×10 dalles jusqu'au premier rendu).
    pub fn new() -> Self {
        Self {
            spectres: Vec::new(),
            orbs: Vec::new(),
            bubbles: Vec::new(),
            selected: 0,
            tick: 0,
            sprites: render::Sprites::load(),
            view: live::View::new(),
            room: ROOM,
            panic: 0,
            seed: 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// Côté de la crypte (dalles) ; les esprits sont ramenés dedans.
    pub fn set_room(&mut self, tiles: f32) {
        self.room = tiles.max(3.0);
        for i in 0..self.spectres.len() {
            self.clamp(i);
        }
    }

    /// Aligne les esprits sur l'état réel : un agent par chat, un
    /// sous-agent par `/agent` ; les nouveaux apparaissent en fondu.
    pub fn sync(&mut self, sessions: &Sessions) {
        for (i, chat) in sessions.iter().enumerate() {
            let mood = if chat.sending {
                Mood::Waiting
            } else {
                Mood::Idle
            };
            let at = self.random_spot();
            self.upsert(Key::Agent(i), i, mood, at);
            let home = self.pos(Key::Agent(i)).unwrap_or(at);
            for agent in &chat.agents {
                let mood = match agent.status {
                    AgentStatus::Running => Mood::Running,
                    AgentStatus::Done => Mood::Done,
                    AgentStatus::Failed => Mood::Failed,
                };
                self.upsert(Key::Sub(agent.id), i, mood, home);
            }
        }
        self.selected = self.selected.min(sessions.count().saturating_sub(1));
    }

    /// Crée l'esprit `key` à `at` s'il manque, sinon met son humeur à jour
    /// (un sous-agent qui échoue lève une alerte).
    fn upsert(&mut self, key: Key, owner: usize, mood: Mood, at: (f32, f32)) {
        if let Some(s) = self.spectres.iter_mut().find(|s| s.key == key) {
            if s.mood != mood && mood == Mood::Failed {
                self.bubbles.push(Bubble {
                    key,
                    glyph: Glyph::Alert,
                    ttl: BUBBLE * 2,
                });
            }
            s.mood = mood;
            return;
        }
        self.spectres.push(Spectre {
            key,
            owner,
            x: at.0,
            y: at.1,
            vx: 0.0,
            vy: 0.0,
            goal: at,
            left: false,
            mood,
            fade: 0,
            jump: 0,
            meet: 0,
            partner: None,
            cooldown: MEET_COOLDOWN / 2,
        });
    }

    /// F5 : panique générale, tout le monde fuit le centre.
    pub fn scare(&mut self) {
        self.panic = PANIC;
        for i in 0..self.spectres.len() {
            self.spectres[i].meet = 0;
            self.spectres[i].partner = None;
        }
    }

    /// Un tick de simulation.
    pub fn step(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.panic = self.panic.saturating_sub(1);
        for i in 0..self.spectres.len() {
            self.think(i);
            self.advance(i);
        }
        self.separate();
        for i in 0..self.spectres.len() {
            self.clamp(i);
        }
        self.maybe_meet();
        self.fly_orbs();
        for b in &mut self.bubbles {
            b.ttl = b.ttl.saturating_sub(1);
        }
        self.bubbles.retain(|b| b.ttl > 0);
    }

    /// Choisit la cible et la vitesse de l'esprit `i` selon son humeur.
    fn think(&mut self, i: usize) {
        let s = &self.spectres[i];
        let c = self.room / 2.0;
        let (target, speed) = if self.panic > 0 {
            let (dx, dy) = (s.x - c, s.y - c);
            let d = (dx * dx + dy * dy).sqrt().max(0.01);
            ((s.x + dx / d * 2.0, s.y + dy / d * 2.0), FLEE)
        } else {
            match s.key {
                Key::Agent(_) => self.agent_goal(i),
                Key::Sub(id) => self.sub_goal(i, id),
            }
        };
        steer(&mut self.spectres[i], target, speed);
    }

    /// Agent : bavarde, attend sur place, ou erre vers un but aléatoire.
    fn agent_goal(&mut self, i: usize) -> ((f32, f32), f32) {
        let s = &self.spectres[i];
        let (key, here, goal, waiting) = (s.key, (s.x, s.y), s.goal, s.mood == Mood::Waiting);
        if s.meet > 0 || waiting {
            if waiting && self.tick % 24 == 0 {
                self.say(key, Glyph::Talk);
            }
            return (here, 0.01);
        }
        let (dx, dy) = (goal.0 - here.0, goal.1 - here.1);
        if dx * dx + dy * dy < 0.04 || self.roll(160) {
            let spot = self.random_spot();
            self.spectres[i].goal = spot;
        }
        (self.spectres[i].goal, WANDER)
    }

    /// Sous-agent : orbite (en cours), dort aux pieds (terminé), coule (échoué).
    fn sub_goal(&mut self, i: usize, id: usize) -> ((f32, f32), f32) {
        let owner = self.spectres[i].owner;
        let c = self.room / 2.0;
        let (ox, oy) = self.pos(Key::Agent(owner)).unwrap_or((c, c));
        let n = id as f32;
        match self.spectres[i].mood {
            Mood::Done => {
                if self.roll(70) {
                    self.say(Key::Sub(id), Glyph::Sleep);
                }
                ((ox - 0.8 + (id % 3) as f32 * 0.8, oy + 0.9), 0.03)
            }
            Mood::Failed => {
                let jitter = if self.tick % 4 < 2 { 0.6 } else { -0.6 };
                (
                    (
                        ox + if id % 2 == 0 { 15.0 } else { -15.0 } + jitter,
                        oy + 12.0,
                    ),
                    0.3,
                )
            }
            _ => {
                let a = self.tick as f32 * 0.07 + n * 2.399;
                ((ox + a.cos() * ORBIT_R, oy + a.sin() * ORBIT_R), ORBIT)
            }
        }
    }

    /// Applique la vitesse, l'orientation, les compteurs et les bornes.
    fn advance(&mut self, i: usize) {
        let s = &mut self.spectres[i];
        s.x += s.vx;
        s.y += s.vy;
        // Vers la gauche de l'écran : x diminue ou y augmente (isométrie).
        let screen_dx = s.vx - s.vy;
        if screen_dx.abs() > 0.006 && s.meet == 0 {
            s.left = screen_dx < 0.0;
        }
        s.fade = s.fade.saturating_add(1).min(8);
        s.jump = s.jump.saturating_sub(1);
        s.cooldown = s.cooldown.saturating_sub(1);
        if s.meet > 0 {
            s.meet -= 1;
            if s.meet == 0 {
                s.partner = None;
                s.cooldown = MEET_COOLDOWN;
            }
        }
        self.clamp(i);
    }

    /// Écarte les esprits qui se chevauchent (sauf deux qui bavardent).
    fn separate(&mut self) {
        let n = self.spectres.len();
        for a in 0..n {
            for b in a + 1..n {
                let (sa, sb) = (&self.spectres[a], &self.spectres[b]);
                let min = spacing(sa.key, sb.key);
                let (dx, dy) = (sb.x - sa.x, sb.y - sa.y);
                let d = (dx * dx + dy * dy).sqrt();
                if d >= min || sa.partner == Some(sb.key) {
                    continue;
                }
                let (ux, uy) = if d < 0.01 {
                    (1.0, 0.0)
                } else {
                    (dx / d, dy / d)
                };
                let push = (min - d) * 0.15;
                self.spectres[a].x -= ux * push;
                self.spectres[a].y -= uy * push;
                self.spectres[b].x += ux * push;
                self.spectres[b].y += uy * push;
            }
        }
    }

    /// Position (centre) de l'esprit `key`.
    pub fn pos(&self, key: Key) -> Option<(f32, f32)> {
        self.spectres
            .iter()
            .find(|s| s.key == key)
            .map(|s| (s.x, s.y))
    }

    /// Ramène l'esprit `i` dans la crypte (à distance des murs).
    fn clamp(&mut self, i: usize) {
        let hi = (self.room - WALL_GAP).max(WALL_GAP);
        let s = &mut self.spectres[i];
        s.x = s.x.clamp(WALL_GAP, hi);
        s.y = s.y.clamp(WALL_GAP, hi);
    }

    /// Point aléatoire au sol, loin des murs (pour un agent).
    fn random_spot(&mut self) -> (f32, f32) {
        let span = (self.room - 2.0 * WALL_GAP - 0.4).max(0.1);
        (
            WALL_GAP + 0.2 + self.unit() * span,
            WALL_GAP + 0.2 + self.unit() * span,
        )
    }

    /// Vrai une fois sur `n` (xorshift, déterministe).
    fn roll(&mut self, n: u64) -> bool {
        self.next() % n.max(1) == 0
    }

    /// Flottant pseudo-aléatoire dans [0, 1).
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn next(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

/// Distance mini entre deux esprits avant de se repousser.
fn spacing(a: Key, b: Key) -> f32 {
    match (a, b) {
        (Key::Agent(_), Key::Agent(_)) => 1.3,
        (Key::Sub(_), Key::Sub(_)) => 0.55,
        _ => 0.75,
    }
}

/// Oriente doucement la vitesse vers `target`, plafonnée à `speed`.
fn steer(s: &mut Spectre, target: (f32, f32), speed: f32) {
    let (dx, dy) = (target.0 - s.x, target.1 - s.y);
    let d = (dx * dx + dy * dy).sqrt();
    let (tvx, tvy) = if d < 0.02 {
        (0.0, 0.0)
    } else {
        let k = speed.min(d) / d;
        (dx * k, dy * k)
    };
    s.vx = s.vx * 0.75 + tvx * 0.25;
    s.vy = s.vy * 0.75 + tvy * 0.25;
}

#[cfg(test)]
mod tests;
