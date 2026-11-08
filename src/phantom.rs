//! Kiro en balade : la mascotte de la crypte hante l'écran chat
//! (sidebar + discussion, jamais le panneau droit ni sous la saisie),
//! juge chaque prompt envoyé avec le modèle du chat (bulle désobligeante)
//! et encaisse les clics : coup = recul, étoiles, « BONK! » ; trois coups
//! rapprochés = K.O. tête en bas, puis il se relève en ronchonnant.
//!
//! Grand fantôme blanc des esprits du hub, en 32×32 (`KIRO_*` du
//! bestiaire), Kitty dans Ghostty via
//! [`crate::gfx::ImageBank`] (demi-blocs ailleurs). Logique pure ici,
//! 10 Hz (`tick`) ; mouvement dans `phantom/motion.rs`, rendu dans
//! `phantom/draw.rs`, consigne et répliques dans `phantom/lines.rs`.
//! Déterministe à graine donnée (tests).

mod draw;
mod lines;
mod motion;

pub use lines::{clean_critique, critique_messages};

use ratatui::layout::{Position, Rect};

use crate::bestiary::{
    Creature, KIRO_FLOAT, KIRO_FRIGHT, KIRO_HIT, KIRO_IDLE, KIRO_JUDGE, KIRO_KO, KIRO_TALK,
};

/// Durées en ticks (100 ms) : coup, K.O., effroi, fenêtre de combo.
const HURT_TICKS: u16 = 12;
const KO_TICKS: u16 = 40;
const SCARE_TICKS: u16 = 20;
const COMBO_TICKS: u64 = 25;
/// Coups rapprochés pour un K.O.
const KO_COMBO: u8 = 3;
/// Durée d'affichage du « BONK! ».
const BONK_TICKS: u8 = 5;

/// Humeur courante : choisit le sprite et le mouvement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    /// Erre d'un point à l'autre, pauses en vol stationnaire.
    Roam,
    /// Frappé : recul amorti, tremblement, étoiles (ticks restants).
    Hurt(u16),
    /// K.O. tête en bas, tombe au sol (ticks restants).
    Ko(u16),
    /// Effrayé (F5) : fuit vite (ticks restants).
    Scared(u16),
}

/// Ton d'une bulle : critique (bouche qui parle), cri, marmonnement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Critique,
    Ouch,
    Mumble,
}

/// Bulle au-dessus de Kiro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speech {
    pub text: String,
    pub tone: Tone,
    /// Ticks restants avant disparition.
    left: u16,
    /// Ticks de bouche qui bouge (critique), puis air jugeur.
    flap: u16,
}

/// Où tombe un clic gauche.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Miss,
    Body,
    Bubble,
}

/// Onomatopée d'un coup, à côté de Kiro, du côté du coup.
#[derive(Debug, Clone, Copy)]
struct Bonk {
    row: u16,
    from_left: bool,
    word: &'static str,
    left: u8,
}

/// Kiro en balade (état complet, voir le module).
#[derive(Debug)]
pub struct Phantom {
    on: bool,
    /// Zone de balade (mémorisée au rendu, `None` avant la 1re frame).
    arena: Option<Rect>,
    /// Taille du sprite en cellules (selon le terminal, voir `draw`).
    size: (u16, u16),
    /// Coin haut-gauche relatif à l'arène, en cellules.
    x: f32,
    y: f32,
    /// Vitesse de recul (coups).
    vx: f32,
    vy: f32,
    target: (f32, f32),
    rest: u16,
    spawned: bool,
    mood: Mood,
    speech: Option<Speech>,
    /// Critique en vol (une seule à la fois).
    pending: bool,
    combo: u8,
    last_hit: u64,
    bonk: Option<Bonk>,
    tick: u64,
    rng: u64,
    /// Zones cliquables de la dernière frame (`None` si non dessinées).
    body: Option<Rect>,
    bubble: Option<Rect>,
    /// Critiques reçues et tokens facturés (`/critic`).
    critiques: u32,
    tokens: u64,
}

impl Phantom {
    /// Kiro prêt à apparaître à la première frame (actif par défaut).
    pub fn new(seed: u64) -> Self {
        Self {
            on: true,
            arena: None,
            size: draw::HALFBLOCK_CELLS,
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
            target: (0.0, 0.0),
            rest: 0,
            spawned: false,
            mood: Mood::Roam,
            speech: None,
            pending: false,
            combo: 0,
            last_hit: 0,
            bonk: None,
            tick: 0,
            rng: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
            body: None,
            bubble: None,
            critiques: 0,
            tokens: 0,
        }
    }

    /// En balade (`/critic on`) ?
    pub fn on(&self) -> bool {
        self.on
    }

    /// `/critic on|off` : éteint, Kiro rentre (bulle et coups oubliés).
    pub fn set_on(&mut self, on: bool) {
        self.on = on;
        if !on {
            self.speech = None;
            self.bonk = None;
            self.mood = Mood::Roam;
            self.body = None;
            self.bubble = None;
        }
    }

    /// Humeur courante (tests).
    #[cfg(test)]
    pub fn mood(&self) -> Mood {
        self.mood
    }

    /// Bulle courante (texte + ton, tests).
    #[cfg(test)]
    pub fn speech(&self) -> Option<&Speech> {
        self.speech.as_ref()
    }

    /// (critiques reçues, tokens facturés) depuis le lancement.
    pub fn stats(&self) -> (u32, u64) {
        (self.critiques, self.tokens)
    }

    /// Prêt à juger un prompt : actif et aucune critique en vol.
    pub fn wants_critique(&self) -> bool {
        self.on && !self.pending
    }

    /// Une critique part (Kiro lit : air jugeur + « hm... »).
    pub fn begin_critique(&mut self) {
        self.pending = true;
    }

    /// Critique reçue : comptée même éteint, dite si actif.
    pub fn critique_done(&mut self, text: Option<String>, tokens: u64) {
        self.pending = false;
        self.tokens = self.tokens.saturating_add(tokens);
        let Some(text) = text else {
            return;
        };
        self.critiques = self.critiques.saturating_add(1);
        if self.on {
            self.say(text, Tone::Critique);
        }
    }

    /// Appel raté : Kiro ronchonne (jamais le détail de l'erreur).
    pub fn critique_failed(&mut self) {
        self.pending = false;
        if self.on {
            self.say(lines::FAILED.to_string(), Tone::Mumble);
        }
    }

    /// Affiche `text` (durée selon la longueur ; la critique fait
    /// d'abord bouger la bouche).
    fn say(&mut self, text: String, tone: Tone) {
        let chars = text.chars().count() as u16;
        let left = (30 + chars * 6 / 10).clamp(40, 160);
        let flap = match tone {
            Tone::Critique => (chars / 3).clamp(10, 40),
            _ => 0,
        };
        self.speech = Some(Speech {
            text,
            tone,
            left,
            flap,
        });
    }

    /// Clic sur la bulle : elle disparaît.
    pub fn hush(&mut self) {
        self.speech = None;
    }

    /// F5 : Kiro a peur aussi et fuit à l'autre bout.
    pub fn scare(&mut self) {
        if !self.on {
            return;
        }
        self.mood = Mood::Scared(SCARE_TICKS);
        self.target = self.far_target();
        self.rest = 0;
        self.say(lines::SCARED.to_string(), Tone::Ouch);
    }

    /// Ce que touche un clic en `at` (zones de la dernière frame).
    pub fn hit_test(&self, at: Position) -> Hit {
        if self.body.is_some_and(|r| r.contains(at)) {
            Hit::Body
        } else if self.bubble.is_some_and(|r| r.contains(at)) {
            Hit::Bubble
        } else {
            Hit::Miss
        }
    }

    /// Coup en `at` : recul opposé au coup, réplique, « BONK! » ;
    /// [`KO_COMBO`] coups en moins de [`COMBO_TICKS`] chacun = K.O.
    pub fn smack(&mut self, at: Position) {
        let Some(body) = self.body else {
            return;
        };
        let streak = self.combo > 0 && self.tick.saturating_sub(self.last_hit) <= COMBO_TICKS;
        self.combo = if streak { self.combo + 1 } else { 1 };
        self.last_hit = self.tick;
        let from_left = at.x < body.x + body.width / 2;
        self.knock(from_left);
        let word = self.pick(&lines::BONKS);
        self.bonk = Some(Bonk {
            row: at.y.saturating_sub(body.y),
            from_left,
            word,
            left: BONK_TICKS,
        });
        let line = if self.combo >= KO_COMBO {
            self.combo = 0;
            self.mood = Mood::Ko(KO_TICKS);
            self.pick(&lines::KO)
        } else {
            self.mood = Mood::Hurt(HURT_TICKS);
            self.pick(&lines::OUCH)
        };
        self.say(line.to_string(), Tone::Ouch);
    }

    /// Sprite de l'humeur : K.O., frappé, effroi ; sinon parle (bouche
    /// qui bouge), juge (critique dite ou en cours), ou flotte (2 poses).
    pub fn creature(&self) -> &'static Creature {
        match self.mood {
            Mood::Ko(_) => &KIRO_KO,
            Mood::Hurt(_) => &KIRO_HIT,
            Mood::Scared(_) => &KIRO_FRIGHT,
            Mood::Roam => match &self.speech {
                Some(s) if s.tone == Tone::Critique && s.flap > 0 => {
                    if (self.tick / 2) % 2 == 0 {
                        &KIRO_TALK
                    } else {
                        &KIRO_JUDGE
                    }
                }
                Some(s) if s.tone == Tone::Critique => &KIRO_JUDGE,
                _ if self.pending => &KIRO_JUDGE,
                _ if (self.tick / 4) % 2 == 0 => &KIRO_IDLE,
                _ => &KIRO_FLOAT,
            },
        }
    }

    /// Critique affichée (immobile pour qu'on la lise).
    fn talking(&self) -> bool {
        self.speech
            .as_ref()
            .is_some_and(|s| s.tone == Tone::Critique)
    }

    /// Une réplique au hasard parmi `lines`.
    fn pick(&mut self, lines: &[&'static str]) -> &'static str {
        lines[self.roll(lines.len() as u64) as usize]
    }

    /// Entier pseudo-aléatoire dans `0..n` (xorshift, graine fixe).
    fn roll(&mut self, n: u64) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x % n.max(1)
    }
}

#[cfg(test)]
mod tests;
