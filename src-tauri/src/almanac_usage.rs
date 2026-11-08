//! Restitution de l'almanach : pire nuit, moyenne, bloc `/usage`, démo.
//!
//! Extrait d'`almanac.rs` (budget 30 fonctions) : que de l'API publique
//! (`recent`, `day`, `span`, `all_time`, `last_days`, `note`), aucun
//! accès aux champs. `peak` tranche les égalités vers le plus récent.

use chrono::{Days, NaiveDate};

use crate::almanac::{Almanac, DayTally};

impl Almanac {
    /// Jour le plus gourmand (le plus récent en cas d'égalité).
    pub fn peak(&self) -> Option<(NaiveDate, DayTally)> {
        self.recent().fold(None, |best, (day, tally)| {
            let better = best.is_none_or(|(_, top): (NaiveDate, DayTally)| {
                tally.total() > top.total()
            });
            if better {
                Some((day, tally))
            } else {
                best
            }
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Date calendaire (panique si invalide, tests seuls).
    fn day(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn peak_recent_en_cas_d_egalite() {
        let mut a = Almanac::memory();
        assert_eq!(a.peak(), None);
        assert_eq!(a.daily_average(), 0);
        a.note(day(2026, 11, 1), 50, 50);
        a.note(day(2026, 11, 3), 10, 10);
        assert_eq!(a.peak().map(|(d, _)| d), Some(day(2026, 11, 1)));
        assert_eq!(a.daily_average(), 60);
        a.note(day(2026, 11, 3), 80, 0);
        assert_eq!(a.peak().map(|(d, _)| d), Some(day(2026, 11, 3)));
    }

    #[test]
    fn usage_block_blank_when_no_calls() {
        let a = Almanac::memory();
        assert_eq!(
            a.usage_block(day(2026, 11, 7)),
            "almanac: blank — no souls reaped yet"
        );
    }

    #[test]
    fn usage_block_shows_tally_and_sparkline() {
        let mut a = Almanac::memory();
        a.note(day(2026, 11, 7), 100, 50);
        a.note(day(2026, 11, 6), 10, 0);
        a.note(day(2026, 10, 1), 5, 5);
        let text = a.usage_block(day(2026, 11, 7));
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
        let today = day(2026, 11, 7);
        let a = Almanac::demo(today);
        let b = Almanac::demo(today);
        assert_eq!(a.all_time(), b.all_time());
        let nights = a.last_days(today, 30);
        assert!(nights.iter().any(|(_, t)| t.calls == 0));
        assert!(nights.iter().any(|(_, t)| t.total() > 0));
    }
}
