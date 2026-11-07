//! Mise en page de la crypte : fond à la bougie, en-tête (onglets + jauge
//! de peur), corps selon l'écran, barre d'état powerline, overlay d'effets.
//!
//! Pensé pour Ghostty : truecolor partout, séparateurs powerline et blocs
//! dessinés par ses sprites internes, icônes Nerd Font embarquées, images
//! via le protocole Kitty (voir `gfx`).

use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use super::{short_model, App};
use crate::bestiary::{self, Creature};
use crate::chat::ChatView;
use crate::gfx::ImageBank;
use crate::mascot::MascotState;
use crate::theme::{self, ASH, BLOOD, BONE, CANDLE, ECTO, PUMPKIN, SAND, SPECTRE, UMBER, VOID};
use crate::tokens::fmt_tps;

/// Écran affiché dans la zone principale.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Chat,
    Tokens,
    Bestiary,
    Hub,
    Almanac,
}

impl Screen {
    /// `target` si on n'y est pas déjà, sinon retour au chat (F2, F6, F8).
    pub(crate) fn toggle(self, target: Screen) -> Self {
        if self == target {
            Screen::Chat
        } else {
            target
        }
    }
}

/// Huitièmes de bloc pour une jauge lisse.
const EIGHTHS: [&str; 9] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];
/// Largeur de la jauge de contexte (cellules).
const CTX_W: usize = 12;
/// Onglets de l'en-tête : écran, libellé, touche.
const TABS: [(Screen, &str, &str); 5] = [
    (Screen::Chat, "chat", ""),
    (Screen::Tokens, "grimoire", "F2"),
    (Screen::Bestiary, "bestiary", "F6"),
    (Screen::Hub, "hub", "F7"),
    (Screen::Almanac, "almanac", "F8"),
];
/// Coiffes powerline arrondies (sprites internes de Ghostty).
const CAP_L: &str = "\u{e0b6}";
const CAP_R: &str = "\u{e0b4}";

impl App {
    /// Dessine tout l'écran selon l'écran actif.
    pub(crate) fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        self.plus_btn = None;
        self.bank.begin_frame();
        self.fx.paint_background(area, frame.buffer_mut());
        if area.width < 80 || area.height < 24 {
            let msg = "Terminal too small: 80x24 required (Esc to quit)";
            let style = Style::new().fg(CANDLE);
            frame.render_widget(Paragraph::new(msg).style(style), area);
            return;
        }
        let [head, body, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(10),
            Constraint::Length(1),
        ])
        .areas(area);
        self.draw_header(frame, head);
        match self.screen {
            Screen::Chat => self.draw_chat(frame, body),
            Screen::Tokens => self.draw_tokens(frame, body),
            Screen::Bestiary => self.galerie.draw(frame, body, &mut self.bank),
            Screen::Hub => self.draw_hub(frame, body),
            Screen::Almanac => self.draw_almanac(frame, body),
        }
        self.draw_status(frame, foot);
        self.fx.draw(area, frame.buffer_mut(), self.bank.drawn());
    }

    /// Corps chat : cercueils | discussion | Kiro + apparition.
    fn draw_chat(&mut self, frame: &mut Frame, area: Rect) {
        let side_w = if area.width >= 110 { 22 } else { 18 };
        let [side, chat, right] = Layout::horizontal([
            Constraint::Length(side_w),
            Constraint::Min(36),
            Constraint::Length(right_width(area.width)),
        ])
        .areas(area);
        self.plus_btn = self.sessions.draw_sidebar(frame, side);
        let sess = self.sessions.current();
        let view = ChatView {
            title: &sess.name,
            tick: self.tick,
            sending: sess.sending,
        };
        sess.chat.draw(frame, chat, view);
        let sending = sess.sending;
        let agents = self.sessions.running_agents(self.sessions.index());
        let [kiro, omen] =
            Layout::vertical([Constraint::Percentage(45), Constraint::Min(6)]).areas(right);
        self.draw_kiro(frame, kiro);
        let creature = self.apparition.current(sending, agents);
        let accent = if self.apparition.scaring() {
            BLOOD
        } else {
            PUMPKIN
        };
        draw_card(frame, omen, creature, accent, &mut self.bank);
    }

    /// Panneau de Kiro : spectre au repos, ectoplasme quand il a peur.
    fn draw_kiro(&mut self, frame: &mut Frame, area: Rect) {
        let colour = match self.mascot.state() {
            MascotState::Idle => theme::lerp(UMBER, SPECTRE, 0.45),
            MascotState::Spook => ECTO,
        };
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(colour))
            .title(theme::title("keeper", SPECTRE, BONE));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        self.mascot.draw(frame, inner, &mut self.bank);
    }

    /// Écran tokens : grimoire + la lanterne qui veille.
    fn draw_tokens(&mut self, frame: &mut Frame, area: Rect) {
        let [book, card] = Layout::horizontal([
            Constraint::Min(40),
            Constraint::Length(right_width(area.width)),
        ])
        .areas(area);
        let [ledger, souls] =
            Layout::vertical([Constraint::Length(9), Constraint::Min(4)]).areas(book);
        self.sessions.current().ledger.draw(frame, ledger);
        self.draw_souls(frame, souls);
        draw_card(
            frame,
            card,
            bestiary::get("citrouille-bougies"),
            CANDLE,
            &mut self.bank,
        );
    }

    /// Âmes par chat : tokens de chaque chat + barre relative au plus gourmand.
    fn draw_souls(&self, frame: &mut Frame, area: Rect) {
        let max = self
            .sessions
            .iter()
            .map(|c| c.ledger.total())
            .max()
            .unwrap_or(0);
        let bar_w = area.width.saturating_sub(48).max(4) as u64;
        let mut lines = vec![Line::styled(
            format!("  {:<18}{:>8}{:>8}{:>9}", "chat", "in", "out", "total"),
            Style::new().fg(ASH),
        )];
        for (i, chat) in self.sessions.iter().enumerate() {
            let current = i == self.sessions.index();
            let (bar, ink) = if current {
                (PUMPKIN, BONE)
            } else {
                (UMBER, SAND)
            };
            let l = &chat.ledger;
            let filled = (l.total() * bar_w).checked_div(max).unwrap_or(0) as usize;
            let name: String = chat.name.chars().take(16).collect();
            lines.push(Line::from(vec![
                Span::styled("▍ ", Style::new().fg(bar)),
                Span::styled(format!("{name:<18}"), Style::new().fg(ink)),
                Span::styled(
                    format!("{:>6}{:>8}{:>9}  ", l.prompt, l.completion, l.total()),
                    Style::new().fg(BONE),
                ),
                Span::styled("█".repeat(filled), Style::new().fg(bar)),
            ]));
        }
        lines.push(self.souls_total_line(bar_w));
        lines.push(Line::styled(
            format!("  incl. agents: {}", self.sessions.agents_grand_total()),
            Style::new().fg(ASH),
        ));
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(UMBER))
            .title(theme::title("souls per chat", SAND, BONE));
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }

    /// Ligne TOTAL des âmes : cumul session (in/out/total + barre
    /// pleine, le total valant toujours au moins le max des chats).
    fn souls_total_line(&self, bar_w: u64) -> Line<'static> {
        let total = self.sessions.total_ledger();
        Line::from(vec![
            Span::styled("▍ ", Style::new().fg(PUMPKIN)),
            Span::styled(
                format!("{:<18}", "TOTAL"),
                Style::new().fg(BONE).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "{:>6}{:>8}{:>9}  ",
                    total.prompt,
                    total.completion,
                    total.total()
                ),
                Style::new().fg(BONE),
            ),
            Span::styled("█".repeat(bar_w as usize), Style::new().fg(PUMPKIN)),
        ])
    }

    /// En-tête : titre en dégradé, onglets, jauge de contexte à droite.
    fn draw_header(&self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![Span::raw(" ")];
        spans.extend(theme::gradient(
            "\u{f068c} CRYPT",
            BLOOD,
            CANDLE,
            Modifier::BOLD,
        ));
        spans.push(Span::styled("  ", Style::new()));
        let gauge = ctx_gauge(self.context_used(), self.context_limit());
        let used = spans_width(&spans) + spans_width(&gauge) + 1;
        spans.extend(self.tabs((area.width as usize).saturating_sub(used)));
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        let gauge = Paragraph::new(Line::from(gauge)).alignment(Alignment::Right);
        frame.render_widget(gauge, area);
    }

    /// Onglets d'écran ; sans les touches (F2…) s'ils ne tiennent pas
    /// dans `room` colonnes (la jauge ctx reste entière à 80 colonnes).
    fn tabs(&self, room: usize) -> Vec<Span<'static>> {
        let build = |keys: bool| -> Vec<Span<'static>> {
            TABS.iter()
                .flat_map(|&(screen, label, key)| {
                    tab(label, if keys { key } else { "" }, self.screen == screen)
                })
                .collect()
        };
        let full = build(true);
        if spans_width(&full) <= room {
            full
        } else {
            build(false)
        }
    }

    /// Écran almanach : tokens par nuit + la faucheuse qui tient le compte.
    fn draw_almanac(&mut self, frame: &mut Frame, area: Rect) {
        let [book, card] = Layout::horizontal([
            Constraint::Min(40),
            Constraint::Length(right_width(area.width)),
        ])
        .areas(area);
        self.almanac.draw(frame, book, crate::almanac::today());
        let reaper = bestiary::get("faucheuse");
        draw_card(frame, card, reaper, BLOOD, &mut self.bank);
    }

    /// Barre d'état : pastilles powerline à gauche, raccourcis à droite.
    fn draw_status(&self, frame: &mut Frame, area: Rect) {
        let sess = self.sessions.current();
        let mut left = Vec::new();
        let chat = format!(
            "\u{f02a0} chat {}/{}",
            self.sessions.index() + 1,
            self.sessions.count()
        );
        left.extend(pill(&chat, PUMPKIN, VOID));
        left.extend(pill(&short_model(&self.effective_model()), UMBER, BONE));
        let rate = format!(
            "{} tok · {} total · {}",
            sess.ledger.total(),
            self.sessions.grand_total(),
            fmt_tps(sess.ledger.last_tps())
        );
        left.extend(pill(&rate, theme::lerp(VOID, UMBER, 0.5), SAND));
        let mode = self.bank.mode();
        let mode_fg = if mode == "kitty" { ECTO } else { ASH };
        left.push(Span::styled(format!(" ◆ {mode}"), Style::new().fg(mode_fg)));
        let used: usize = left.iter().map(|s| s.width()).sum();
        frame.render_widget(Paragraph::new(Line::from(left)), area);
        let hints = key_hints();
        let hints_w: usize = hints.iter().map(|s| s.width()).sum();
        if used + hints_w + 2 <= area.width as usize {
            let right = Paragraph::new(Line::from(hints)).alignment(Alignment::Right);
            frame.render_widget(right, area);
        }
    }
}

/// Largeur du panneau droit (Kiro + apparitions) selon la largeur totale.
fn right_width(total: u16) -> u16 {
    (total * 3 / 10).clamp(24, 56)
}

/// Carte illustrée : nom en titre, image, légende en bas.
fn draw_card(
    frame: &mut Frame,
    area: Rect,
    creature: &'static Creature,
    accent: Color,
    bank: &mut ImageBank,
) {
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme::lerp(UMBER, accent, 0.6)))
        .title(theme::title(creature.name, accent, CANDLE));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 3 {
        return;
    }
    let [art, lore] = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).areas(inner);
    bank.draw(frame, art, creature);
    let style = Style::new().fg(SAND).add_modifier(Modifier::ITALIC);
    let text = Paragraph::new(Line::styled(creature.lore, style))
        .alignment(Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(text, lore);
}

/// Largeur affichée d'une suite de spans.
fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(Span::width).sum()
}

/// Onglet : pastille citrouille si actif, texte sable sinon (+ touche).
fn tab(label: &str, key: &str, active: bool) -> Vec<Span<'static>> {
    let mut spans = if active {
        pill(label, PUMPKIN, VOID)
    } else {
        vec![Span::styled(format!(" {label} "), Style::new().fg(SAND))]
    };
    if !key.is_empty() {
        spans.push(Span::styled(key.to_string(), Style::new().fg(UMBER)));
    }
    spans.push(Span::raw(" "));
    spans
}

/// Pastille powerline arrondie : coiffe, texte sur fond `bg`, coiffe.
fn pill(text: &str, bg: Color, fg: Color) -> Vec<Span<'static>> {
    let cap = Style::new().fg(bg);
    vec![
        Span::styled(CAP_L, cap),
        Span::styled(
            format!(" {text} "),
            Style::new().bg(bg).fg(fg).add_modifier(Modifier::BOLD),
        ),
        Span::styled(CAP_R, cap),
        Span::raw(" "),
    ]
}

/// Context gauge: `ctx ██████▍      042`, candle to blood.
/// `used` = estimated tokens of the current chat, `max` = model window.
fn ctx_gauge(used: u64, max: u64) -> Vec<Span<'static>> {
    let max = max.max(1);
    let ratio = used as f32 / max as f32;
    let filled = ratio.min(1.0) * CTX_W as f32;
    let track = Style::new().bg(theme::lerp(VOID, UMBER, 0.35));
    let mut spans = vec![Span::styled("ctx ", Style::new().fg(SAND))];
    for i in 0..CTX_W {
        let part = (filled - i as f32).clamp(0.0, 1.0);
        let glyph = EIGHTHS[(part * 8.0).round() as usize];
        let fg = theme::lerp(CANDLE, BLOOD, i as f32 / (CTX_W - 1) as f32);
        spans.push(Span::styled(glyph, track.fg(fg)));
    }
    let pct = (used.saturating_mul(100) / max).min(999);
    let colour = match pct {
        80.. => BLOOD,
        40.. => CANDLE,
        _ => ASH,
    };
    let num = Style::new().fg(colour).add_modifier(Modifier::BOLD);
    spans.push(Span::styled(format!(" {pct:03} "), num));
    spans
}

/// Raccourcis rappelés dans la barre d'état.
fn key_hints() -> Vec<Span<'static>> {
    let key = Style::new().fg(PUMPKIN).add_modifier(Modifier::BOLD);
    let word = Style::new().fg(ASH);
    let mut spans = Vec::new();
    for (k, w) in [("F3/F4", "chats"), ("F5", "scare"), ("Esc", "flee")] {
        spans.push(Span::styled(k, key));
        spans.push(Span::styled(format!(" {w}  "), word));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Role;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn ctx_gauge_is_fixed_width_and_monotonic() {
        let width = |u| ctx_gauge(u, 1000).iter().map(|s| s.width()).sum::<usize>();
        assert_eq!(width(0), width(1000));
        assert_eq!(width(0), width(9999));
        let full = |u| {
            ctx_gauge(u, 1000)
                .iter()
                .filter(|s| s.content == "█")
                .count()
        };
        assert!(full(0) <= full(500) && full(500) <= full(1000));
        assert_eq!(full(1000), CTX_W);
        assert_eq!(full(5000), CTX_W);
    }

    #[test]
    fn header_fits_five_tabs_and_gauge_at_80_columns() {
        let mut app = App::offline().unwrap();
        for (w, keys) in [(80, false), (120, true)] {
            let mut term = Terminal::new(TestBackend::new(w, 24)).unwrap();
            term.draw(|f| app.draw(f)).unwrap();
            let buf = term.backend().buffer();
            let head: String = (0..w).map(|x| buf[(x, 0)].symbol()).collect();
            for word in ["chat", "grimoire", "bestiary", "hub", "almanac", "ctx"] {
                assert!(head.contains(word), "{w} cols: {word} missing in {head:?}");
            }
            assert_eq!(head.contains("F8"), keys, "{w} cols: {head:?}");
        }
    }

    #[test]
    fn every_screen_renders_at_common_sizes() {
        let mut app = App::offline().unwrap();
        app.almanac = crate::almanac::Almanac::demo(crate::almanac::today());
        app.sessions
            .current_mut()
            .chat
            .push(Role::Assistant, "les os craquent sous la lune");
        for (w, h) in [(80, 24), (120, 40), (200, 60), (60, 20)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            for screen in [
                Screen::Chat,
                Screen::Tokens,
                Screen::Bestiary,
                Screen::Almanac,
            ] {
                app.screen = screen;
                app.spook();
                app.tick();
                term.draw(|f| app.draw(f)).unwrap();
            }
        }
    }
}
