//! AI question choice panel (zerocode style) : question, numbered
//! options (highlighted one on a pumpkin pill), key reminder.
//!
//! While a question is pending, ↑↓ drive the panel (plain ↓ first
//! re-sticks to the bottom when scrolled up), empty Enter answers
//! with the highlight, Esc dismisses ; otherwise the keys keep their
//! usual meaning (agent cursor, expand, clear agent view).

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use super::App;
use crate::chat::{wrap_text, INPUT_HEIGHT};
use crate::question::PendingQuestion;
use crate::theme::{self, ASH, BONE, CANDLE, CRYPT, PUMPKIN, SAND, VOID};

/// AI question panel (zerocode style) : question, numbered options
/// (highlighted on a pumpkin pill), key reminder.
pub(crate) fn draw_question_picker(frame: &mut Frame, body: Rect, q: &PendingQuestion) {
    let area = picker_area(body, q);
    if area.width < 24 || area.height < 6 {
        return;
    }
    frame.render_widget(Clear, area);
    let fill = Style::new().bg(CRYPT);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(CANDLE))
        .style(fill)
        .title(theme::title("pick", CANDLE, PUMPKIN));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut ask = wrap_text(&q.question, inner.width.max(1) as usize);
    ask.truncate(2);
    let chunks = Layout::vertical([
        Constraint::Length(ask.len().max(1) as u16),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let ask_style = Style::new().fg(BONE).bg(CRYPT).add_modifier(Modifier::BOLD);
    let lines: Vec<Line> = ask
        .into_iter()
        .map(|l| Line::styled(l, ask_style))
        .collect();
    frame.render_widget(Paragraph::new(lines).style(fill), chunks[0]);
    let mut rows = Vec::with_capacity(q.options.len());
    for (i, opt) in q.options.iter().enumerate() {
        rows.push(picker_row(i, opt, i == q.cursor));
    }
    let mut state = ListState::default();
    state.select(Some(q.cursor.min(q.options.len().saturating_sub(1))));
    frame.render_stateful_widget(List::new(rows).style(fill), chunks[1], &mut state);
    let hint = Style::new()
        .fg(ASH)
        .bg(CRYPT)
        .add_modifier(Modifier::ITALIC);
    let foot = Paragraph::new(Line::styled("↑↓ pick · Enter answer · Esc dismiss", hint));
    frame.render_widget(foot.style(fill), chunks[2]);
}

/// Question panel rectangle : 5 % margins, anchored above the input
/// (it stays visible for free replies), fitted height (options +
/// question lines + footer + borders).
fn picker_area(body: Rect, q: &PendingQuestion) -> Rect {
    let guess = body.width.saturating_sub(body.width.div_ceil(10) + 2) as usize;
    let wrapped = wrap_text(&q.question, guess);
    let ask_h = wrapped.len().clamp(1, 2) as u16;
    let desired = q.options.len() as u16 + ask_h + 3;
    let top = Rect {
        height: body.height.saturating_sub(INPUT_HEIGHT),
        ..body
    };
    let h = desired.min(top.height).max(3);
    let vert = Layout::vertical([Constraint::Min(0), Constraint::Length(h)]).split(top);
    Layout::horizontal([
        Constraint::Percentage(5),
        Constraint::Min(40),
        Constraint::Percentage(5),
    ])
    .split(vert[1])[1]
}

/// Option row : `❯ 1. Yes` on a pumpkin pill when highlighted.
fn picker_row(i: usize, opt: &str, current: bool) -> ListItem<'static> {
    let label = format!("{}. {opt}", i + 1);
    if current {
        let style = Style::new()
            .bg(PUMPKIN)
            .fg(VOID)
            .add_modifier(Modifier::BOLD);
        ListItem::new(Line::styled(format!("❯ {label}"), style))
    } else {
        ListItem::new(Line::styled(
            format!("  {label}"),
            Style::new().fg(SAND).bg(CRYPT),
        ))
    }
}

impl App {
    /// ↑↓ : pending question → panel ; otherwise agent cursor
    /// (ignored without a chat).
    pub(crate) fn picker_or_agent(&mut self, delta: isize) {
        let idx = self.sessions.index();
        let pending = self.sessions.get(idx).is_some_and(|s| s.pending.is_some());
        if pending {
            self.move_picker(delta);
        } else {
            self.step_agent(delta as i8);
        }
    }

    /// Scrolled-up ↓ : re-stick to the bottom (first, even with a
    /// question) ; otherwise move the panel cursor (nothing pending,
    /// ignored without a chat).
    fn move_picker(&mut self, delta: isize) {
        let idx = self.sessions.index();
        let Some(sess) = self.sessions.get_mut(idx) else {
            return;
        };
        if delta > 0 && sess.chat.scroll.offset() > 0 {
            sess.chat.scroll.to_bottom();
            return;
        }
        if let Some(q) = sess.pending.as_mut() {
            q.step(delta);
        }
    }

    /// Empty Enter : fill the input with the highlight (`false` = no
    /// question or no chat, the caller does nothing).
    pub(crate) fn fill_picker_selection(&mut self) -> bool {
        let idx = self.sessions.index();
        let Some(sess) = self.sessions.get_mut(idx) else {
            return false;
        };
        match sess.pending.take() {
            Some(q) => {
                let label = q.selected().to_string();
                sess.chat.input_insert(&label);
                true
            }
            None => false,
        }
    }

    /// Esc : dismiss the pending question (`false` = none, the caller
    /// falls back to clearing the agent view).
    pub(crate) fn dismiss_picker(&mut self) -> bool {
        let idx = self.sessions.index();
        self.sessions
            .get_mut(idx)
            .is_some_and(|s| s.pending.take().is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderConfig;

    /// Test app (dummy key, no network call expected).
    fn test_app() -> App {
        App::from_config(ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        })
        .unwrap()
    }

    /// Sample pending question (two options).
    fn sample() -> PendingQuestion {
        PendingQuestion {
            question: String::from("Shall we go on?"),
            options: vec![String::from("Yes"), String::from("No")],
            cursor: 0,
        }
    }

    /// Plain key (no modifier).
    fn key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
        crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    #[test]
    fn up_down_drive_panel_when_pending_else_agents() {
        use crossterm::event::KeyCode;
        let mut app = test_app();
        assert!(app.on_key(key(KeyCode::Down)), "no question: agent key");
        assert!(app.sessions.current().pending.is_none());
        app.sessions.current_mut().pending = Some(sample());
        assert!(app.on_key(key(KeyCode::Down)));
        assert_eq!(app.sessions.current().pending.as_ref().unwrap().cursor, 1);
        assert!(app.on_key(key(KeyCode::Up)));
        assert_eq!(app.sessions.current().pending.as_ref().unwrap().cursor, 0);
        // Scrolled up + question : ↓ re-sticks first, cursor unchanged.
        app.sessions.current().chat.scroll.by(-5);
        assert!(app.on_key(key(KeyCode::Down)));
        assert_eq!(app.sessions.current().chat.scroll.offset(), 0);
        assert_eq!(app.sessions.current().pending.as_ref().unwrap().cursor, 0);
        assert!(app.on_key(key(KeyCode::Down)));
        assert_eq!(app.sessions.current().pending.as_ref().unwrap().cursor, 1);
    }

    #[tokio::test]
    async fn empty_enter_answers_with_highlight() {
        use crossterm::event::KeyCode;
        let mut app = test_app();
        // Empty config injected : hermetic to the repo's real .kiro/ (no MCP spawn).
        let vide = std::env::temp_dir().join(format!("kh-picker-{}", std::process::id()));
        std::fs::create_dir_all(&vide).unwrap();
        app.t5.kiro_project = vide.clone();
        app.t5.kiro_global = None;
        app.mcp = crate::kiro::mcp_pool::McpState::for_config(crate::kiro::mcp::Config::default());
        app.sessions.current_mut().pending = Some(sample());
        assert!(app.on_key(key(KeyCode::Down)));
        assert!(app.on_key(key(KeyCode::Enter)), "empty Enter answers");
        let sess = app.sessions.current();
        assert!(sess.pending.is_none(), "answering clears the question");
        assert_eq!(
            sess.history.last(),
            Some(&(String::from("user"), String::from("No"))),
            "the highlight goes out as the message"
        );
        assert!(sess.sending, "the send goes out normally");
    }

    #[test]
    fn esc_dismisses_then_clears_agents() {
        use crossterm::event::KeyCode;
        let mut app = test_app();
        app.sessions.current_mut().pending = Some(sample());
        assert!(app.on_key(key(KeyCode::Esc)), "1st Esc : dismiss, stay");
        assert!(app.sessions.current().pending.is_none());
        assert!(
            app.on_key(key(KeyCode::Esc)),
            "2nd Esc : clear agents, stay"
        );
    }

    #[test]
    fn panel_drawn_at_common_sizes() {
        use ratatui::{backend::TestBackend, Terminal};
        let mut app = test_app();
        app.sessions.current_mut().pending = Some(sample());
        for (w, h) in [(70, 17), (80, 24), (120, 36)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| app.draw(f)).unwrap();
            let buf = term.backend().buffer().clone();
            let screen: String = (0..h)
                .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(screen.contains("pick"), "panel missing at {w}x{h}");
            assert!(
                screen.contains("Shall we go on?"),
                "question missing at {w}x{h}"
            );
            assert!(screen.contains("1. Yes"), "options missing at {w}x{h}");
        }
    }
}
