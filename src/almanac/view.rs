//! Écran F8 : le décompte des morts (résumé), les âmes fauchées nuit par
//! nuit (barres de sang qui montent du sol, la nuit en cours à la bougie)
//! et la nécrologie (dernières nuits, la plus récente en tête).

use chrono::NaiveDate;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use super::{Almanac, DayTally};
use crate::theme::{self, ASH, BLOOD, BONE, CANDLE, DRIED, PUMPKIN, SAND, UMBER};

/// Huitièmes verticaux, vide → plein (les barres sortent de terre).
const RISE: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
/// Nuits affichées au plus dans le graphe (moins si étroit).
const MAX_NIGHTS: usize = 30;
/// Largeur des colonnes chiffrées de la nécrologie.
const NUM_W: usize = 10;

impl Almanac {
    /// Dessine l'almanach : résumé, graphe des nuits, nécrologie.
    pub fn draw(&self, frame: &mut Frame, area: Rect, today: NaiveDate) {
        let table_h = (area.height / 3).clamp(5, 16);
        let [summary, chart, table] = Layout::vertical([
            Constraint::Length(5),
            Constraint::Min(7),
            Constraint::Length(table_h),
        ])
        .areas(area);
        self.draw_tally(frame, summary, today);
        self.draw_chart(frame, chart, today);
        self.draw_necrology(frame, table, today);
    }

    /// Décompte des morts : ce soir, 7 et 30 nuits, depuis toujours,
    /// moyenne et pire nuit ; détail du soir (ou erreur disque) en bas.
    fn draw_tally(&self, frame: &mut Frame, area: Rect, today: NaiveDate) {
        let tonight = self.day(today);
        let darkest = self.peak().map(|(d, t)| (t.total(), short_date(d)));
        let (worst, worst_day) = darkest.unwrap_or((0, String::new()));
        let rows = [
            [
                cell("tonight", tonight.total(), PUMPKIN),
                cell("all time", self.all_time().total(), BONE),
            ],
            [
                cell("7 nights", self.span(today, 7).total(), BONE),
                cell("per night", self.daily_average(), BONE),
            ],
            [
                cell("30 nights", self.span(today, 30).total(), BONE),
                cell("darkest", worst, BLOOD),
            ],
        ];
        let mut lines: Vec<Line> = rows
            .into_iter()
            .map(|[a, b]| {
                Line::from([vec![Span::raw("  ")], a, vec![Span::raw("   ")], b].concat())
            })
            .collect();
        lines[2].push_span(Span::styled(format!(" {worst_day}"), Style::new().fg(ASH)));
        let footer = match self.error() {
            Some(err) => Line::styled(format!(" {err} "), Style::new().fg(BLOOD)),
            None => Line::styled(tonight_detail(tonight), Style::new().fg(ASH)),
        };
        let block = panel("\u{f068c} tally of the dead", BLOOD, CANDLE).title_bottom(footer);
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }

    /// Âmes fauchées par nuit : une barre par nuit, sang séché → sang
    /// frais, la nuit en cours à la bougie ; échelle (pic) en haut à droite.
    fn draw_chart(&self, frame: &mut Frame, area: Rect, today: NaiveDate) {
        let inner_w = area.width.saturating_sub(2) as usize;
        let nights = (inner_w / 3).clamp(1, MAX_NIGHTS);
        let days = self.last_days(today, nights);
        let peak = days.iter().map(|(_, t)| t.total()).max().unwrap_or(0);
        let scale = Line::styled(format!(" peak {} ", fmt_count(peak)), Style::new().fg(ASH));
        let block = panel("souls reaped per night", DRIED, BLOOD).title(scale.right_aligned());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner_w < 3 || inner.height < 2 {
            return;
        }
        if peak == 0 {
            return blank(frame, inner);
        }
        let slot = inner_w / nights;
        let chart = Chart {
            pad: (inner_w - slot * nights) / 2,
            slot,
            bar: (slot - 1).clamp(1, 4),
            rows: inner.height.saturating_sub(1) as usize,
            peak,
            today,
        };
        let mut lines: Vec<Line> = (0..chart.rows).map(|r| chart.row(&days, r)).collect();
        lines.push(chart.labels(&days));
        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// Nécrologie : nuits inscrites, la plus récente en tête (in, out,
    /// total, appels + barre relative à la pire nuit listée).
    fn draw_necrology(&self, frame: &mut Frame, area: Rect, today: NaiveDate) {
        let block = panel("necrology", SAND, BONE);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let shown: Vec<_> = self
            .recent()
            .take(inner.height.saturating_sub(1) as usize)
            .collect();
        if shown.is_empty() {
            return blank(frame, inner);
        }
        let max = shown.iter().map(|(_, t)| t.total()).max().unwrap_or(1);
        let bar_w = (inner.width as usize).saturating_sub(13 + NUM_W * 3 + 9);
        let head = format!(
            "  {:<11}{:>NUM_W$}{:>NUM_W$}{:>NUM_W$}{:>7}",
            "night", "in", "out", "total", "calls"
        );
        let mut lines = vec![Line::styled(head, Style::new().fg(ASH))];
        for (day, tally) in shown {
            let filled = (tally.total() * bar_w as u64).checked_div(max).unwrap_or(0);
            lines.push(obituary(day, tally, day == today, filled as usize));
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

/// Géométrie du graphe des nuits.
struct Chart {
    /// Marge gauche (centre les barres).
    pad: usize,
    /// Colonnes par nuit (barre + espace).
    slot: usize,
    /// Largeur d'une barre.
    bar: usize,
    /// Rangées de barres (hors étiquettes).
    rows: usize,
    peak: u64,
    today: NaiveDate,
}

impl Chart {
    /// Rangée `r` (0 = haut) : un morceau de barre par nuit.
    fn row(&self, days: &[(NaiveDate, DayTally)], r: usize) -> Line<'static> {
        let level = self.rows - 1 - r;
        let height = level as f32 / self.rows.saturating_sub(1).max(1) as f32;
        let mut spans = vec![Span::raw(" ".repeat(self.pad))];
        for (day, tally) in days {
            let eighths = self.eighths(tally.total());
            let fill = eighths.saturating_sub(level * 8).min(8);
            let (glyph, colour) = if tally.total() == 0 && level == 0 {
                ("▁", UMBER)
            } else if *day == self.today {
                (RISE[fill], theme::lerp(CANDLE, PUMPKIN, height))
            } else {
                (RISE[fill], theme::lerp(DRIED, BLOOD, height))
            };
            spans.push(Span::styled(
                glyph.repeat(self.bar),
                Style::new().fg(colour),
            ));
            spans.push(Span::raw(" ".repeat(self.slot - self.bar)));
        }
        Line::from(spans)
    }

    /// Hauteur d'une valeur en huitièmes de cellule (au moins 1 si > 0).
    fn eighths(&self, value: u64) -> usize {
        if value == 0 {
            return 0;
        }
        let full = (self.rows * 8) as f64;
        ((value as f64 / self.peak as f64 * full).round() as usize).max(1)
    }

    /// Étiquettes : jour du mois sous chaque barre, ce soir à la citrouille.
    fn labels(&self, days: &[(NaiveDate, DayTally)]) -> Line<'static> {
        let mut spans = vec![Span::raw(" ".repeat(self.pad))];
        for (day, _) in days {
            let style = if *day == self.today {
                Style::new().fg(PUMPKIN).add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(ASH)
            };
            let text = format!("{:<w$}", day.format("%d").to_string(), w = self.slot);
            spans.push(Span::styled(text, style));
        }
        Line::from(spans)
    }
}

/// Cadre arrondi couleur terre avec un titre en dégradé.
fn panel(title: &str, from: Color, to: Color) -> Block<'static> {
    Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(UMBER))
        .title(theme::title(title, from, to))
}

/// Case du décompte : libellé sable, nombre gras dans `ink`.
fn cell(label: &str, value: u64, ink: Color) -> Vec<Span<'static>> {
    vec![
        Span::styled(format!("{label:<10}"), Style::new().fg(SAND)),
        Span::styled(
            format!("{:>10}", fmt_count(value)),
            Style::new().fg(ink).add_modifier(Modifier::BOLD),
        ),
    ]
}

/// Ligne de nécrologie : marque (citrouille ce soir), date, chiffres, barre.
fn obituary(day: NaiveDate, tally: DayTally, tonight: bool, filled: usize) -> Line<'static> {
    let (mark, ink) = if tonight {
        (PUMPKIN, BONE)
    } else {
        (DRIED, SAND)
    };
    Line::from(vec![
        Span::styled("▍ ", Style::new().fg(mark)),
        Span::styled(
            format!("{:<11}", day.format("%a %d %b").to_string()),
            Style::new().fg(ink),
        ),
        Span::styled(
            format!(
                "{:>NUM_W$}{:>NUM_W$}{:>NUM_W$}{:>7}  ",
                fmt_count(tally.prompt),
                fmt_count(tally.completion),
                fmt_count(tally.total()),
                fmt_count(tally.calls)
            ),
            Style::new().fg(BONE),
        ),
        Span::styled("█".repeat(filled), Style::new().fg(mark)),
    ])
}

/// Message d'almanach vierge, centré.
fn blank(frame: &mut Frame, area: Rect) {
    let style = Style::new().fg(ASH).add_modifier(Modifier::ITALIC);
    let msg = Paragraph::new(Line::styled(
        "the almanac is blank — no souls reaped yet",
        style,
    ))
    .alignment(Alignment::Center);
    let [mid] = Layout::vertical([Constraint::Length(1)])
        .flex(ratatui::layout::Flex::Center)
        .areas(area);
    frame.render_widget(msg, mid);
}

/// Détail de la nuit en cours (pied du décompte).
fn tonight_detail(t: DayTally) -> String {
    format!(
        " tonight: in {} · out {} · {} calls ",
        fmt_count(t.prompt),
        fmt_count(t.completion),
        fmt_count(t.calls)
    )
}

/// Date courte : `07 Nov`.
fn short_date(day: NaiveDate) -> String {
    day.format("%d %b").to_string()
}

/// Entier avec séparateurs de milliers : `1234567` → `1,234,567`.
pub fn fmt_count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    /// Rend l'almanach seul dans un buffer `w`x`h`, en texte brut.
    fn render(a: &Almanac, today: NaiveDate, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| a.draw(f, f.area(), today)).unwrap();
        let buf = term.backend().buffer();
        buf.content.iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn fmt_count_groups_thousands() {
        assert_eq!(fmt_count(0), "0");
        assert_eq!(fmt_count(999), "999");
        assert_eq!(fmt_count(1_000), "1,000");
        assert_eq!(fmt_count(1_234_567), "1,234,567");
    }

    #[test]
    fn chart_bar_height_tracks_value() {
        let chart = Chart {
            pad: 0,
            slot: 3,
            bar: 2,
            rows: 4,
            peak: 100,
            today: date(2026, 11, 7),
        };
        assert_eq!(chart.eighths(0), 0);
        assert_eq!(chart.eighths(1), 1);
        assert_eq!(chart.eighths(50), 16);
        assert_eq!(chart.eighths(100), 32);
    }

    #[test]
    fn empty_almanac_says_blank() {
        let text = render(&Almanac::memory(), date(2026, 11, 7), 56, 22);
        assert!(text.contains("the almanac is blank"));
        assert!(text.contains("tally of the dead"));
    }

    #[test]
    fn filled_almanac_draws_bars_and_tonight() {
        let today = date(2026, 11, 7);
        let mut a = Almanac::memory();
        a.note(today, 1_000, 234);
        a.note(date(2026, 11, 5), 400, 0);
        let text = render(&a, today, 56, 22);
        assert!(text.contains('█'));
        assert!(text.contains("1,234"));
        assert!(text.contains("Sat 07 Nov"));
        assert!(text.contains("Thu 05 Nov"));
        assert!(!text.contains("the almanac is blank"));
    }
}
