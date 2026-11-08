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

    /// Jour le plus gourmand (le plus récent en cas d'égalité).
    pub fn peak(&self) -> Option<(NaiveDate, DayTally)> {
        self.days
            .iter()
            .filter(|(_, t)| t.total() > 0)
            .max_by_key(|(_, t)| t.total())
            .map(|(d, t)| (*d, *t))
    }

    /// Moyenne par jour actif (0 si almanach vide).
    pub fn daily_average(&self) -> u64 {
        let active = self.recent().count() as u64;
        self.all_time().total().checked_div(active).unwrap_or(0)
    }

    /// Bloc `/usage` : ce soir, 7/30 nuits, total, moyenne, pire nuit +
    /// graphe des 30 dernières nuits. Vide → une seule ligne.
    pub fn usage_block(&self, today: NaiveDate) -> String {
        let nights = self.recent().count();
        if nights == 0 {
            return String::from("almanac: blank — no souls reaped yet");
        }
        let tonight = self.day(today);
        let darkest = self
            .peak()
            .map(|(day, tally)| format!("{} ({})", tally.total(), day.format("%d %b")))
            .unwrap_or_else(|| String::from("—"));
        let mut out = format!(
            "ALMANAC (all sessions) —\n\
             tonight: {} (in={} out={} calls={})\n\
             7 nights: {} · 30 nights: {} · all time: {} ({nights} nights, ~{}/night)\n\
             darkest: {darkest}\n\
             last 30 nights: {}",
            tonight.total(),
            tonight.prompt,
            tonight.completion,
            tonight.calls,
            self.span(today, 7).total(),
            self.span(today, 30).total(),
            self.all_time().total(),
            self.daily_average(),
            sparkline(&self.last_days(today, SPARK_NIGHTS)),
        );
        if let Some(err) = self.error() {
            out.push_str(&format!("\nalmanac error: {err}"));
        }
        out
    }

    /// Almanach de démonstration (mémoire) pour `--smoke` : 40 nuits
    /// pseudo-aléatoires mais déterministes, quelques nuits calmes.
    pub fn demo(today: NaiveDate) -> Self {
        let mut almanac = Self::memory();
        for back in 0..40u64 {
            let wave = (back * 7919 + 13) % 23;
            let Some(day) = today.checked_sub_days(Days::new(back)) else {
                continue;
            };
            if wave % 5 == 0 {
                continue;
            }
            for call in 0..1 + wave % 6 {
                almanac.note(day, 900 + wave * 310 + call * 50, 200 + wave * 95);
            }
        }
        almanac
    }
}

/// Nuits couvertes par le graphe de [`Almanac::usage_block`].
const SPARK_NIGHTS: usize = 30;
/// Niveaux du graphe, du plus bas au plus haut.
const SPARK_LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Graphe une ligne des totaux (0 → `▁`, max → `█`), ancien → récent.
fn sparkline(days: &[(NaiveDate, DayTally)]) -> String {
    let max = days.iter().map(|(_, t)| t.total()).max().unwrap_or(0);
    if max == 0 {
        return "▁".repeat(days.len());
    }
    days.iter()
        .map(|(_, t)| {
            let idx = (t.total().saturating_mul(8) / max).min(7) as usize;
            SPARK_LEVELS[idx]
        })
        .collect()
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
    fn peak_average_and_recent_skip_empty_days() {
        let mut a = Almanac::memory();
        assert_eq!(a.peak(), None);
        assert_eq!(a.daily_average(), 0);
        a.note(date(2026, 11, 1), 50, 50);
        a.note(date(2026, 11, 3), 10, 10);
        assert_eq!(a.peak().map(|(d, _)| d), Some(date(2026, 11, 1)));
        assert_eq!(a.daily_average(), 60);
        let recent: Vec<_> = a.recent().map(|(d, _)| d).collect();
        assert_eq!(recent, vec![date(2026, 11, 3), date(2026, 11, 1)]);
    }

    #[test]
    fn usage_block_blank_when_no_calls() {
        let a = Almanac::memory();
        assert_eq!(
            a.usage_block(date(2026, 11, 7)),
            "almanac: blank — no souls reaped yet"
        );
    }

    #[test]
    fn usage_block_shows_tally_and_sparkline() {
        let mut a = Almanac::memory();
        a.note(date(2026, 11, 7), 100, 50);
        a.note(date(2026, 11, 6), 10, 0);
        a.note(date(2026, 10, 1), 5, 5);
        let text = a.usage_block(date(2026, 11, 7));
        assert!(
            text.contains("tonight: 150 (in=100 out=50 calls=1)"),
            "{text}"
        );
        assert!(text.contains("7 nights: 160"), "{text}");
        assert!(text.contains("30 nights: 160"), "{text}");
        assert!(
            text.contains("all time: 170 (3 nights, ~56/night)"),
            "{text}"
        );
        assert!(text.contains("darkest: 150 (07 Nov)"), "{text}");
        let spark = text
            .lines()
            .find(|l| l.starts_with("last 30 nights: "))
            .unwrap()
            .trim_start_matches("last 30 nights: ");
        assert_eq!(spark.chars().count(), 30);
        assert!(spark.ends_with('█'), "{spark}");
    }

    #[test]
    fn demo_is_deterministic_and_has_quiet_nights() {
        let today = date(2026, 11, 7);
        let a = Almanac::demo(today);
        let b = Almanac::demo(today);
        assert_eq!(a.all_time(), b.all_time());
        let nights = a.last_days(today, 30);
        assert!(nights.iter().any(|(_, t)| t.calls == 0));
        assert!(nights.iter().any(|(_, t)| t.total() > 0));
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
