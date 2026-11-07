//! Apparitions : la créature du panneau droit, pilotée par la crypte.
//!
//! Priorité : réponse en cours (chaudron) > sous-agents actifs (araignée)
//! > présage récent (erreur, compaction, effroi) > rotation lente du
//! bestiaire. Kiro (le fantôme) a son propre panneau et n'apparaît pas ici.

use std::time::{Duration, Instant};

use crate::bestiary::{self, Creature, BESTIARY};

/// Une créature nouvelle toutes les 20 s au repos.
const ROTATION: Duration = Duration::from_secs(20);
/// Durée d'affichage d'un présage.
const OMEN: Duration = Duration::from_secs(6);
/// Durée d'un jump scare (F5), calée sur l'effroi de Kiro.
const SCARE: Duration = Duration::from_secs(2);
/// Créatures tirées pour un jump scare.
const SCARY: [&str; 8] = [
    "faucheuse",
    "cri",
    "loup-garou",
    "tele",
    "dame-blanche",
    "zombie",
    "masque-bois",
    "tete-poignardee",
];

/// Ce qui vient de se passer dans la crypte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Omen {
    /// Une requête a échoué.
    Error,
    /// L'historique vient d'être compacté.
    Compact,
}

/// État du panneau des apparitions.
#[derive(Debug)]
pub struct Apparition {
    index: usize,
    since: Instant,
    omen: Option<(&'static Creature, Instant, Duration)>,
}

impl Apparition {
    /// Démarre sur une créature tirée au hasard.
    pub fn new() -> Self {
        Self {
            index: random_index(usize::MAX),
            since: Instant::now(),
            omen: None,
        }
    }

    /// Rotation au repos + expiration du présage.
    pub fn tick(&mut self) {
        if self.since.elapsed() >= ROTATION {
            self.advance();
        }
        if let Some((_, at, ttl)) = self.omen {
            if at.elapsed() >= ttl {
                self.omen = None;
            }
        }
    }

    /// Une réponse est arrivée : nouvelle créature.
    pub fn advance(&mut self) {
        self.index = random_index(self.index);
        self.since = Instant::now();
    }

    /// Affiche le présage `omen` quelques secondes.
    pub fn omen(&mut self, omen: Omen) {
        let slug = match omen {
            Omen::Error => "main-tombe",
            Omen::Compact => "cercueil-main",
        };
        self.omen = Some((bestiary::get(slug), Instant::now(), OMEN));
    }

    /// Jump scare (F5) : une créature effrayante pendant 2 s.
    pub fn scare(&mut self) {
        let pick = SCARY[rand::random_range(0..SCARY.len())];
        self.omen = Some((bestiary::get(pick), Instant::now(), SCARE));
    }

    /// Vrai pendant un jump scare.
    pub fn scaring(&self) -> bool {
        matches!(self.omen, Some((_, _, ttl)) if ttl == SCARE)
    }

    /// Créature à montrer selon l'activité du chat courant.
    pub fn current(&self, sending: bool, agents: usize) -> &'static Creature {
        if let Some((creature, _, ttl)) = self.omen {
            if ttl == SCARE {
                return creature;
            }
        }
        if sending {
            return bestiary::get("chaudron");
        }
        if agents > 0 {
            return bestiary::get("araignee");
        }
        match self.omen {
            Some((creature, _, _)) => creature,
            None => &BESTIARY[self.index],
        }
    }
}

impl Default for Apparition {
    fn default() -> Self {
        Self::new()
    }
}

/// Index aléatoire du bestiaire, différent de `avoid` et jamais Kiro.
fn random_index(avoid: usize) -> usize {
    loop {
        let i = rand::random_range(0..BESTIARY.len());
        if i != avoid && BESTIARY[i].slug != "fantome" {
            return i;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_beats_rotation_and_omens() {
        let mut ap = Apparition::new();
        ap.omen(Omen::Error);
        assert_eq!(ap.current(false, 0).slug, "main-tombe");
        assert_eq!(ap.current(false, 2).slug, "araignee");
        assert_eq!(ap.current(true, 2).slug, "chaudron");
        ap.scare();
        assert!(ap.scaring());
        assert!(SCARY.contains(&ap.current(true, 0).slug));
    }

    #[test]
    fn rotation_never_shows_kiro_nor_repeats() {
        let mut ap = Apparition::new();
        for _ in 0..200 {
            let before = ap.index;
            ap.advance();
            assert_ne!(ap.index, before);
            assert_ne!(ap.current(false, 0).slug, "fantome");
        }
    }
}
