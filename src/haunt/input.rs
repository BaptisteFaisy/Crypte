//! Commandes du jeu : touches tenues et appuis à consommer.
//!
//! Avec le protocole clavier Kitty (Ghostty), le terminal signale les
//! relâchements : une touche est tenue de l'appui au relâchement. Sans lui
//! (repli), chaque appui la tient quelques pas, prolongés par la
//! répétition automatique du clavier.

use crossterm::event::KeyEventKind;

/// Action de jeu (voir `app/haunt.rs` pour les touches).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Left,
    Right,
    Up,
    Down,
    Fire,
    Start,
    Pause,
}

const ACTS: usize = 7;
/// Repli sans relâchement : tenue après un premier appui (couvre le délai
/// de répétition du clavier), puis après chaque répétition (pas de 1/240 s).
const FIRST: u64 = 12 * super::SUB as u64;
const AGAIN: u64 = 4 * super::SUB as u64;

#[derive(Debug, Clone, Default)]
pub struct Input {
    /// Pas jusqu'auquel l'action est tenue (`u64::MAX` = jusqu'au relâchement).
    until: [u64; ACTS],
    /// Pas de l'appui + 1 (0 = rien à consommer).
    pressed: [u64; ACTS],
    /// Le terminal signale les relâchements.
    releases: bool,
}

impl Input {
    /// Relâchements signalés (protocole Kitty actif) ou non (repli).
    pub fn set_releases(&mut self, on: bool) {
        if self.releases != on {
            self.clear();
        }
        self.releases = on;
    }

    /// Événement clavier pour `act` au pas `now`.
    pub fn feed(&mut self, act: Act, kind: KeyEventKind, now: u64) {
        let i = act as usize;
        match kind {
            KeyEventKind::Release => self.until[i] = 0,
            KeyEventKind::Repeat => self.until[i] = self.hold(i, now),
            KeyEventKind::Press => {
                self.until[i] = self.hold(i, now);
                self.pressed[i] = now + 1;
            }
        }
    }

    fn hold(&self, i: usize, now: u64) -> u64 {
        if self.releases {
            u64::MAX
        } else if self.until[i] > now {
            now + AGAIN
        } else {
            now + FIRST
        }
    }

    /// Action tenue au pas `now`.
    pub fn held(&self, act: Act, now: u64) -> bool {
        self.until[act as usize] > now
    }

    /// Direction horizontale tenue : -1, 0 ou 1.
    pub fn axis(&self, now: u64) -> f32 {
        f32::from(i8::from(self.held(Act::Right, now)) - i8::from(self.held(Act::Left, now)))
    }

    /// Appui récent (moins de `window` pas) : vrai une seule fois.
    pub fn take(&mut self, act: Act, now: u64, window: u64) -> bool {
        let at = self.pressed[act as usize];
        let fresh = at > 0 && now < at + window;
        if fresh {
            self.pressed[act as usize] = 0;
        }
        fresh
    }

    /// Oublie tout (changement d'écran, de mode).
    pub fn clear(&mut self) {
        self.until = [0; ACTS];
        self.pressed = [0; ACTS];
    }
}
