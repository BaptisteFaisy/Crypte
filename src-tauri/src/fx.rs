//! Effets horreur : niveau de peur, éclair d'effroi, compteur de frame.
//!
//! Reprend de `src/fx.rs` (TUI) la machine à états pure (`new`, `calm`,
//! `flash`, `tick`) ; le rendu ratatui (`paint_background`, `draw`,
//! brume, vignette) n'est pas porté — le frontend Svelte le dessine
//! sur canvas à partir de [`FxSnapshot`]. Même constantes, mêmes paliers.

/// Peur gagnée par tick en effroi, perdue par tick au calme.
const FEAR_UP: u8 = 7;
const FEAR_DOWN: u8 = 2;
/// Frames d'éclair au début d'un effroi.
const FLASH_FRAMES: u8 = 4;

/// Machine à effets.
#[derive(Debug, Clone)]
pub struct Fx {
    frame: u64,
    fear: u8,
    flash: u8,
    spooked: bool,
}

/// Vue sérialisable pour le frontend (canvas) et l'intégrateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FxSnapshot {
    pub frame: u64,
    pub fear: u8,
    pub flash: u8,
    pub spooked: bool,
}

impl Fx {
    /// Effets neufs : frame 0, peur 0.
    pub fn new() -> Self {
        Self {
            frame: 0,
            fear: 0,
            flash: 0,
            spooked: false,
        }
    }

    /// Apaise la crypte : la peur chute de `amount` (plancher 0).
    /// Effet de boutique (bougie apaisante, eau bénite).
    pub fn calm(&mut self, amount: u8) {
        self.fear = self.fear.saturating_sub(amount);
    }

    /// Éclair immédiat, comme au premier tick d'un effroi.
    /// Effet de boutique.
    pub fn flash(&mut self) {
        self.flash = FLASH_FRAMES;
    }

    /// Un tick. En effroi la peur monte vers 100 et un éclair part au
    /// premier tick ; au calme la peur redescend.
    pub fn tick(&mut self, spooked: bool) {
        self.frame = self.frame.wrapping_add(1);
        if spooked && !self.spooked {
            self.flash = FLASH_FRAMES;
        } else {
            self.flash = self.flash.saturating_sub(1);
        }
        self.spooked = spooked;
        self.fear = if spooked {
            self.fear.saturating_add(FEAR_UP).min(100)
        } else {
            self.fear.saturating_sub(FEAR_DOWN)
        };
    }

    /// Peur courante (0..=100, rougit la vignette frontend).
    pub fn fear(&self) -> u8 {
        self.fear
    }

    /// Éclair restant (0 = éteint, `FLASH_FRAMES` = plein flash).
    pub fn flash_left(&self) -> u8 {
        self.flash
    }

    /// Compteur de frames (dérive de la brume frontend).
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Vue complète pour le rendu frontend.
    pub fn snapshot(&self) -> FxSnapshot {
        FxSnapshot {
            frame: self.frame,
            fear: self.fear,
            flash: self.flash,
            spooked: self.spooked,
        }
    }
}

impl Default for Fx {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fear_rises_while_spooked_and_clamps_at_100() {
        let mut fx = Fx::new();
        for _ in 0..50 {
            fx.tick(true);
        }
        assert_eq!(fx.fear, 100);
    }

    #[test]
    fn fear_decays_while_calm_and_clamps_at_0() {
        let mut fx = Fx::new();
        for _ in 0..50 {
            fx.tick(true);
        }
        for _ in 0..100 {
            fx.tick(false);
        }
        assert_eq!(fx.fear, 0);
    }

    #[test]
    fn boutique_calm_et_flash() {
        let mut fx = Fx::new();
        for _ in 0..10 {
            fx.tick(true);
        }
        assert!(fx.fear > 30);
        fx.calm(30);
        let after = fx.fear;
        fx.calm(100);
        assert_eq!(fx.fear, 0, "eau bénite : peur à zéro (était {after})");
        fx.flash();
        assert_eq!(fx.flash, FLASH_FRAMES);
    }

    #[test]
    fn snapshot_expose_tout_pour_le_frontend() {
        let mut fx = Fx::new();
        fx.tick(true);
        let snap = fx.snapshot();
        assert_eq!(snap.frame, 1);
        assert_eq!(snap.fear, 7);
        assert_eq!(snap.flash, FLASH_FRAMES);
        assert!(snap.spooked);
    }
}
