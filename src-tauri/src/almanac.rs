//! Almanach des âmes : tokens consommés par jour, conservés d'une session
//! à l'autre (lu par `usage_get`, fichier dans `almanac_disk.rs`).
//!
//! Port tel quel d'`almanac.rs` du TUI.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{Days, Local, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::almanac_disk::{default_path, read_days, write_days};

/// Tokens d'une journée (in, out, nombre d'appels).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayTally {
    pub prompt: u64,
    pub completion: u64,
    pub calls: u64,
}

impl DayTally {
    /// Total in + out.
    pub fn total(&self) -> u64 {
        self.prompt.saturating_add(self.completion)
    }

    /// Ajoute `other` (sommes saturantes).
    fn absorb(&mut self, other: DayTally) {
        self.prompt = self.prompt.saturating_add(other.prompt);
        self.completion = self.completion.saturating_add(other.completion);
        self.calls = self.calls.saturating_add(other.calls);
    }
}

/// Registre des jours ; `path = None` = mémoire seule (tests, ou après
/// une erreur disque). `session` cumule les appels du process en cours
/// (jamais persisté : pièces boutique de la session).
#[derive(Debug, Clone, Default)]
pub struct Almanac {
    days: BTreeMap<NaiveDate, DayTally>,
    session: DayTally,
    path: Option<PathBuf>,
    error: Option<String>,
}

impl Almanac {
    /// Almanach en mémoire seule : ne touche jamais le disque.
    pub fn memory() -> Self {
        Self::default()
    }

    /// Almanach du fichier par défaut ([`default_path`]) ; sans `HOME`,
    /// mémoire seule avec l'erreur affichée.
    pub fn open_default() -> Self {
        match default_path() {
            Ok(path) => Self::open(path),
            Err(err) => Self::detached(err),
        }
    }

    /// Almanach adossé à `path` (fichier absent = almanach vide).
    pub fn open(path: PathBuf) -> Self {
        let mut almanac = Self {
            path: Some(path),
            ..Self::default()
        };
        almanac.refresh();
        almanac
    }

    /// Mémoire seule avec une erreur à afficher.
    fn detached(err: anyhow::Error) -> Self {
        Self {
            error: Some(format!("{err:#}")),
            ..Self::default()
        }
    }

    /// Relit le fichier (d'autres cryptes ont pu écrire). En cas d'échec,
    /// se détache du fichier pour ne jamais l'écraser.
    pub fn refresh(&mut self) {
        let Some(path) = &self.path else {
            return;
        };
        match read_days(path) {
            Ok(days) => self.days = days,
            Err(err) => self.detach(err),
        }
    }

    /// Enregistre un appel à la date locale du jour : relit, ajoute, écrit.
    pub fn record(&mut self, prompt: u64, completion: u64) {
        self.refresh();
        self.note(today(), prompt, completion);
        self.session.absorb(DayTally {
            prompt,
            completion,
            calls: 1,
        });
        let Some(path) = &self.path else {
            return;
        };
        if let Err(err) = write_days(path, &self.days) {
            self.detach(err);
        }
    }

    /// Ajoute un appel à `day`, en mémoire seulement.
    pub fn note(&mut self, day: NaiveDate, prompt: u64, completion: u64) {
        let call = DayTally {
            prompt,
            completion,
            calls: 1,
        };
        self.days.entry(day).or_default().absorb(call);
    }

    /// Passe en mémoire seule (le delta courant reste visible) + erreur.
    fn detach(&mut self, err: anyhow::Error) {
        self.error = Some(format!("{err:#} (kept in memory)"));
        self.path = None;
    }

    /// Dernière erreur disque, s'il y en a une.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Tokens de `day` (zéro si rien ce jour-là).
    pub fn day(&self, day: NaiveDate) -> DayTally {
        self.days.get(&day).copied().unwrap_or_default()
    }

    /// Les `n` derniers jours jusqu'à `today` inclus, du plus ancien au
    /// plus récent, jours vides compris.
    pub fn last_days(&self, today: NaiveDate, n: usize) -> Vec<(NaiveDate, DayTally)> {
        (0..n as u64)
            .rev()
            .filter_map(|back| today.checked_sub_days(Days::new(back)))
            .map(|day| (day, self.day(day)))
            .collect()
    }

    /// Somme des `n` derniers jours jusqu'à `today` inclus.
    pub fn span(&self, today: NaiveDate, n: usize) -> DayTally {
        let mut sum = DayTally::default();
        for (_, tally) in self.last_days(today, n) {
            sum.absorb(tally);
        }
        sum
    }

    /// Cumul de tous les jours inscrits.
    pub fn all_time(&self) -> DayTally {
        let mut sum = DayTally::default();
        for tally in self.days.values() {
            sum.absorb(*tally);
        }
        sum
    }

    /// Jours inscrits (avec au moins un appel), du plus récent au plus ancien.
    pub fn recent(&self) -> impl Iterator<Item = (NaiveDate, DayTally)> + '_ {
        self.days
            .iter()
            .rev()
            .filter(|(_, t)| t.calls > 0)
            .map(|(d, t)| (*d, *t))
    }

    /// Nombre de jours inscrits (avec au moins un appel).
    pub fn nights(&self) -> usize {
        self.recent().count()
    }

    /// Tokens consommés par le process en cours (pièces boutique).
    pub fn session(&self) -> DayTally {
        self.session
    }
}

/// Date locale du jour (les nuits changent à minuit, heure locale).
pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn note_accumulates_per_day() {
        let mut a = Almanac::memory();
        a.note(date(2026, 11, 7), 10, 5);
        a.note(date(2026, 11, 7), 1, 2);
        a.note(date(2026, 11, 6), 100, 0);
        let tally = a.day(date(2026, 11, 7));
        assert_eq!((tally.prompt, tally.completion, tally.calls), (11, 7, 2));
        assert_eq!(tally.total(), 18);
        assert_eq!(a.day(date(2026, 11, 5)), DayTally::default());
        assert_eq!(a.all_time().total(), 118);
        assert_eq!(a.nights(), 2);
    }

    #[test]
    fn last_days_fills_gaps_oldest_first_across_months() {
        let mut a = Almanac::memory();
        a.note(date(2026, 10, 31), 3, 0);
        a.note(date(2026, 11, 2), 4, 0);
        let days = a.last_days(date(2026, 11, 2), 4);
        let got: Vec<_> = days.iter().map(|(d, t)| (*d, t.total())).collect();
        assert_eq!(
            got,
            vec![
                (date(2026, 10, 30), 0),
                (date(2026, 10, 31), 3),
                (date(2026, 11, 1), 0),
                (date(2026, 11, 2), 4),
            ]
        );
        assert_eq!(a.span(date(2026, 11, 2), 2).total(), 4);
        assert_eq!(a.span(date(2026, 11, 2), 3).total(), 7);
    }

    #[test]
    fn session_cumule_sans_persister() {
        let mut a = Almanac::memory();
        a.record(10, 5);
        a.record(1, 2);
        assert_eq!(a.session().total(), 18);
        assert_eq!(a.session().calls, 2);
    }
}
