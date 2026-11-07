//! Settings screen: the provider table, the masked key input and the footer
//! (vault path, counts, shortcuts). Keys are rendered as bullets only.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use super::Settings;
use crate::theme::{ASH, BLOOD, CANDLE, CRYPT, ECTO, PUMPKIN, SAND, UMBER};

/// Columns kept for the provider name.
const NAME_W: usize = 13;
/// Columns kept for the key state (`key ✓` / `no key`).
const KEY_W: usize = 7;
/// Footer height (notice, shortcuts, vault line).
const FOOT_H: u16 = 4;

impl Settings {
    /// Draws the whole screen: provider table + footer.
    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        if area.height < FOOT_H + 3 || area.width < 40 {
            return;
        }
        let [table, foot] =
            Layout::vertical([Constraint::Min(3), Constraint::Length(FOOT_H)]).areas(area);
        self.draw_table(frame, table);
        self.draw_footer(frame, foot);
    }

    /// Table of providers, scrolled so the cursor stays visible.
    fn draw_table(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(UMBER))
            .title(Span::styled(
                format!(" SETTINGS · PROVIDERS ({}) ", self.rows().len()),
                Style::new().fg(PUMPKIN).add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if self.rows().is_empty() || inner.height == 0 {
            return;
        }
        let (name_w, base_w, model_w) = columns(usize::from(inner.width));
        let height = usize::from(inner.height);
        let first = (self.selected + 1).saturating_sub(height);
        for (offset, row) in self.rows().iter().skip(first).take(height).enumerate() {
            let cursor = first + offset == self.selected;
            let key = if row.has_key {
                Span::styled(fit("key ✓", KEY_W), Style::new().fg(ECTO))
            } else {
                Span::styled(fit("no key", KEY_W), Style::new().fg(ASH))
            };
            let mut style = Style::new().fg(if cursor { CANDLE } else { SAND });
            if cursor {
                style = style.bg(CRYPT).add_modifier(Modifier::BOLD);
            }
            let line = Line::from(vec![
                Span::styled(
                    format!(
                        "{} {}",
                        if cursor { "▸" } else { " " },
                        fit(&row.name, name_w)
                    ),
                    style,
                ),
                Span::styled(fit(&row.base_url, base_w), style),
                Span::styled(fit(&row.model, model_w), style),
                key,
            ]);
            let y = inner.y + offset as u16;
            frame.render_widget(
                Paragraph::new(line),
                Rect {
                    height: 1,
                    y,
                    ..inner
                },
            );
        }
    }

    /// Footer: last notice, input line (masked) or shortcuts, vault path.
    fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        let notice = match self.notice.as_deref() {
            Some(text) if text.contains("not saved") || text.contains("unreadable") => {
                Span::styled(text.to_string(), Style::new().fg(BLOOD))
            }
            Some(text) => Span::styled(text.to_string(), Style::new().fg(SAND)),
            None => Span::styled(
                String::from("keys are stored encrypted; the TUI never shows them"),
                Style::new().fg(ASH),
            ),
        };
        let input = match self.edit.as_ref() {
            Some(edit) => {
                let name = self.selected_row().map(|r| r.name.as_str()).unwrap_or("");
                Span::styled(
                    format!("> {} {name}: {}", edit.field.label(), self.edit_preview()),
                    Style::new().fg(CANDLE).add_modifier(Modifier::BOLD),
                )
            }
            None => Span::styled(
                String::from("Enter key · m model · b base · c connect · d erase · q close"),
                Style::new().fg(ASH),
            ),
        };
        let vault = Span::styled(
            format!(
                "{} providers · {} keys · {}",
                self.rows().len(),
                self.keyed_count(),
                self.store.data_path().display()
            ),
            Style::new().fg(UMBER),
        );
        let lines = vec![Line::from(notice), Line::from(input), Line::from(vault)];
        frame.render_widget(Paragraph::new(lines), area);
    }
}

/// Widths of the name / base / model columns for `width` columns of table.
/// Narrow tables drop the model column (base + key state still readable).
fn columns(width: usize) -> (usize, usize, usize) {
    let fixed = NAME_W + KEY_W + 4;
    let room = width.saturating_sub(fixed);
    if room < 48 {
        return (NAME_W, room, 0);
    }
    let base = (room * 3 / 5).min(52);
    let model = room - base;
    (NAME_W, base, model)
}

/// Truncates `text` to `width` chars (with `…`) and pads with spaces.
fn fit(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let count = text.chars().count();
    let mut out: String = if count > width {
        let mut cut: String = text.chars().take(width - 1).collect();
        cut.push('…');
        cut
    } else {
        text.to_string()
    };
    let len = out.chars().count();
    out.push_str(&" ".repeat(width.saturating_sub(len)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Field, Store};
    use ratatui::{backend::TestBackend, Terminal};
    use std::path::PathBuf;

    /// Settings in a throwaway directory (returned for cleanup).
    fn settings(name: &str) -> (Settings, PathBuf) {
        let dir = std::env::temp_dir().join(format!("kh-ui-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        (Settings::with_store(Store::new(dir.clone())), dir)
    }

    fn screen_text(settings: &Settings, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|frame| {
            let area = frame.area();
            settings.draw(frame, area);
        })
        .unwrap();
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_the_provider_table_at_common_sizes() {
        let (settings, dir) = settings("table");
        for (w, h) in [(80, 24), (120, 40), (200, 60)] {
            let text = screen_text(&settings, w, h);
            assert!(text.contains("SETTINGS · PROVIDERS"), "{w}x{h}");
            assert!(text.contains("openai"), "{w}x{h}");
            assert!(text.contains("no key"), "{w}x{h}");
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn masked_input_never_shows_the_key() {
        let (mut settings, dir) = settings("mask");
        settings.begin_edit(Field::Key);
        for c in "sk-do-not-print".chars() {
            settings.push_char(c);
        }
        let text = screen_text(&settings, 120, 40);
        assert!(!text.contains("sk-do-not-print"));
        assert!(text.contains("•"));
        assert!(text.contains("API key"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn tiny_area_does_not_panic() {
        let (settings, dir) = settings("tiny");
        for (w, h) in [(40, 7), (10, 3), (80, 6)] {
            screen_text(&settings, w, h);
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn fit_truncates_with_ellipsis_and_pads() {
        assert_eq!(fit("abc", 5), "abc  ");
        assert_eq!(fit("abcdef", 3), "ab…");
        assert_eq!(fit("", 2), "  ");
        assert_eq!(fit("abc", 0), "");
    }

    #[test]
    fn narrow_tables_drop_the_model_column() {
        let (name, base, model) = columns(70 - 2);
        assert_eq!(name, NAME_W);
        assert_eq!(model, 0);
        assert!(base > 30);
        let (_, base, model) = columns(120 - 2);
        assert!(model > 0);
        assert!(base + model + NAME_W + KEY_W + 4 <= 120);
    }
}
