//! Mouvement de Kiro (10 Hz) : errance d'un point à l'autre avec pauses
//! en vol stationnaire, recul amorti des coups (rebond aux murs), chute
//! du K.O. jusqu'au sol, fuite (F5). Immobile pendant une critique.

use super::{lines, Mood, Phantom, Tone, HURT_TICKS};

/// Vitesse d'errance (cellules/tick ; moitié en vertical, cellules 1:2).
const SPEED: f32 = 0.5;
/// Fuite (F5) : multiple de la vitesse d'errance.
const FLEE: f32 = 3.0;
/// Recul d'un coup (cellules/tick), saut vers le haut, amortissement.
const KNOCK: f32 = 2.4;
const HOP: f32 = 0.8;
const FRICTION: f32 = 0.72;
/// Chute du K.O. (cellules/tick²).
const GRAVITY: f32 = 0.15;
/// Rangées du bas jamais visées en errance (saisie).
const KEEP_ROWS: u16 = 4;
/// Pause entre deux points (ticks) : minimum + aléa.
const REST_MIN: u64 = 15;
const REST_RANDOM: u64 = 35;
/// Ticks de tremblement au début d'un coup.
const SHAKE_TICKS: u16 = 6;

impl Phantom {
    /// Un tick : bulle et « BONK! » vieillissent, puis Kiro bouge selon
    /// son humeur (rien avant la première frame : arène inconnue).
    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.age();
        if !self.on {
            return;
        }
        let Some((mx, my, floor)) = self.limits() else {
            return;
        };
        match self.mood {
            Mood::Hurt(_) => self.recoil(mx, 0.0),
            Mood::Ko(_) => self.recoil(mx, GRAVITY),
            Mood::Scared(_) => {
                self.travel(SPEED * FLEE);
            }
            Mood::Roam if self.talking() => {}
            Mood::Roam => self.wander(mx, my),
        }
        self.x = self.x.clamp(0.0, mx);
        self.y = self.y.clamp(0.0, floor);
        self.countdown();
    }

    /// Bornes du coin haut-gauche (arène relative) : x max, y max en
    /// errance (au-dessus de la saisie), y max absolu (sol). `None` si
    /// l'arène est inconnue ou trop petite pour le sprite.
    pub(super) fn limits(&self) -> Option<(f32, f32, f32)> {
        let arena = self.arena?;
        let (w, h) = self.size;
        let mx = arena.width.checked_sub(w)?;
        let floor = arena.height.checked_sub(h)?;
        let my = floor.saturating_sub(KEEP_ROWS);
        Some((f32::from(mx), f32::from(my), f32::from(floor)))
    }

    /// Première apparition : au centre de l'arène, petite pause.
    pub(super) fn spawn(&mut self) {
        let Some((mx, _, floor)) = self.limits() else {
            return;
        };
        self.spawned = true;
        self.x = (mx / 2.0).floor();
        self.y = (floor / 2.0).floor();
        self.target = (self.x, self.y);
        self.rest = (REST_MIN * 2) as u16;
    }

    /// Décalage horizontal de tremblement (début d'un coup), en cellules.
    pub(super) fn shake(&self) -> i32 {
        match self.mood {
            Mood::Hurt(n) if n > HURT_TICKS - SHAKE_TICKS => {
                if n % 2 == 0 {
                    1
                } else {
                    -1
                }
            }
            _ => 0,
        }
    }

    /// Élan d'un coup : chassé à l'opposé du côté frappé, petit saut.
    pub(super) fn knock(&mut self, from_left: bool) {
        self.vx = if from_left { KNOCK } else { -KNOCK };
        self.vy = -HOP;
        self.rest = 0;
    }

    /// Point opposé (fuite) : l'autre bord, hauteur au hasard.
    pub(super) fn far_target(&mut self) -> (f32, f32) {
        let Some((mx, my, _)) = self.limits() else {
            return self.target;
        };
        let x = if self.x < mx / 2.0 { mx } else { 0.0 };
        (x, self.roll_f(my))
    }

    /// Bulle et « BONK! » : un tick de moins, disparus à zéro.
    fn age(&mut self) {
        if let Some(s) = self.speech.as_mut() {
            s.left = s.left.saturating_sub(1);
            s.flap = s.flap.saturating_sub(1);
            if s.left == 0 {
                self.speech = None;
            }
        }
        if let Some(b) = self.bonk.as_mut() {
            b.left = b.left.saturating_sub(1);
            if b.left == 0 {
                self.bonk = None;
            }
        }
    }

    /// Fin d'humeur : coup, effroi → errance ; K.O. → se relève en
    /// ronchonnant.
    fn countdown(&mut self) {
        self.mood = match self.mood {
            Mood::Hurt(n) if n > 1 => Mood::Hurt(n - 1),
            Mood::Scared(n) if n > 1 => Mood::Scared(n - 1),
            Mood::Ko(n) if n > 1 => Mood::Ko(n - 1),
            Mood::Ko(_) => {
                let line = self.pick(&lines::GETUP);
                self.say(line.to_string(), Tone::Mumble);
                Mood::Roam
            }
            _ => Mood::Roam,
        };
    }

    /// Recul : avance de la vitesse, rebondit aux murs, s'amortit ;
    /// `gravity` > 0 le fait tomber (K.O.).
    fn recoil(&mut self, mx: f32, gravity: f32) {
        self.x += self.vx;
        self.y += self.vy;
        if self.x < 0.0 || self.x > mx {
            self.vx = -self.vx * 0.5;
        }
        if self.y < 0.0 {
            self.vy = -self.vy * 0.5;
        }
        self.vx *= FRICTION;
        self.vy = self.vy * FRICTION + gravity;
    }

    /// Avance vers la cible à `speed` (moitié en vertical) ; vrai si
    /// arrivé.
    fn travel(&mut self, speed: f32) -> bool {
        let (dx, dy) = (self.target.0 - self.x, self.target.1 - self.y);
        let vertical = speed / 2.0;
        self.x += dx.clamp(-speed, speed);
        self.y += dy.clamp(-vertical, vertical);
        dx.abs() <= speed && dy.abs() <= vertical
    }

    /// Errance : pause, puis vers la cible ; arrivé = pause et nouvelle
    /// cible au hasard (au-dessus de la saisie).
    fn wander(&mut self, mx: f32, my: f32) {
        if self.rest > 0 {
            self.rest -= 1;
            return;
        }
        if self.travel(SPEED) {
            self.rest = (REST_MIN + self.roll(REST_RANDOM)) as u16;
            self.target = (self.roll_f(mx), self.roll_f(my));
        }
    }

    /// Réel pseudo-aléatoire dans `0..=max`.
    fn roll_f(&mut self, max: f32) -> f32 {
        self.roll(1001) as f32 / 1000.0 * max
    }
}
