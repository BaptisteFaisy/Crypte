//! Mise en page de la crypte : fond à la bougie, en-tête (onglets + jauge
//! de peur), corps selon l'écran, barre d'état powerline, overlay d'effets.
//!
//! Pensé pour Ghostty : truecolor partout, séparateurs powerline et blocs
//! dessinés par ses sprites internes, icônes Nerd Font embarquées, images
//! via le protocole Kitty (voir `gfx`).

use ratatui::{
    layout::{Alignment, Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use super::App;
use crate::bestiary::Creature;
use crate::chat::ChatView;
use crate::gfx::ImageBank;
use crate::mascot::MascotState;
use crate::theme::{self, ASH, BLOOD, BONE, CANDLE, ECTO, PUMPKIN, SAND, SPECTRE, UMBER, VOID};
use crate::tokens::{fmt_tps, short_model};

/// Écran affiché dans la zone principale.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Chat,
    Bestiary,
    Hub,
}

impl Screen {
    /// `target` si on n'y est pas déjà, sinon retour au chat (F6).
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
const FEAR_W: usize = 12;
/// Taille minimale du terminal : 70x17 (compact sous 80 colonnes).
pub(crate) const MIN_W: u16 = 70;
pub(crate) const MIN_H: u16 = 17;
/// Seuil compact : sous 80 colonnes, sidebar réduite, panneau droit masqué.
const COMPACT_W: u16 = 80;
/// Onglets de l'en-tête : écran, libellé, touche.
const TABS: [(Screen, &str, &str); 3] = [
    (Screen::Chat, "chat", ""),
    (Screen::Bestiary, "bestiary", "F6"),
    (Screen::Hub, "hub", "F7"),
];
/// Coiffes powerline arrondies (sprites internes de Ghostty).
const CAP_L: &str = "\u{e0b6}";
const CAP_R: &str = "\u{e0b4}";

impl App {
    /// Dessine tout l'écran selon l'écran actif.
    pub(crate) fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        self.plus_btn = None;
        self.side_rows = None;
        self.tab_hits.clear();
        self.bank.begin_frame();
        self.fx.paint_background(area, frame.buffer_mut());
        if area.width < MIN_W || area.height < MIN_H {
            let msg = "Terminal too small: 70x17 required (q to quit)";
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
            Screen::Bestiary => self.galerie.draw(frame, body, &mut self.bank),
            Screen::Hub => self.draw_hub(frame, body),
        }
        self.draw_status(frame, foot);
        self.fx.draw(area, frame.buffer_mut(), self.bank.drawn());
    }

    /// Corps chat : cercueils | discussion | Kiro + apparition.
    /// Sous 80 colonnes : cercueils réduits | discussion (droit masqué).
    fn draw_chat(&mut self, frame: &mut Frame, area: Rect) {
        if area.width < COMPACT_W {
            return self.draw_chat_compact(frame, area);
        }
        let side_w = if area.width >= 110 { 22 } else { 18 };
        let [side, chat, right] = Layout::horizontal([
            Constraint::Length(side_w),
            Constraint::Min(36),
            Constraint::Length(right_width(area.width, side_w + 36, self.right_delta)),
        ])
        .areas(area);
        let hits = self.sessions.draw_sidebar(frame, side);
        self.side_rows = hits.chats;
        self.plus_btn = hits.plus;
        let sess = self.sessions.current_mut();
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

    /// Corps compact (< 80 col.) : sidebar 14 + discussion au reste.
    /// Kiro et apparitions masqués : le chat reste pleinement utilisable.
    fn draw_chat_compact(&mut self, frame: &mut Frame, area: Rect) {
        let [side, chat] =
            Layout::horizontal([Constraint::Length(14), Constraint::Min(30)]).areas(area);
        let hits = self.sessions.draw_sidebar(frame, side);
        self.side_rows = hits.chats;
        self.plus_btn = hits.plus;
        let sess = self.sessions.current_mut();
        let view = ChatView {
            title: &sess.name,
            tick: self.tick,
            sending: sess.sending,
        };
        sess.chat.draw(frame, chat, view);
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

    /// En-tête : titre en dégradé, onglets (cliquables, voir `mouse`),
    /// jauge de contexte à droite.
    fn draw_header(&mut self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![Span::raw(" ")];
        spans.extend(theme::gradient(
            "\u{f068c} CRYPT",
            BLOOD,
            CANDLE,
            Modifier::BOLD,
        ));
        spans.push(Span::styled("  ", Style::new()));
        let gauge = fear_gauge(self.ctx_used, self.ctx_limit);
        let used = spans_width(&spans) + spans_width(&gauge) + 1;
        let room = (area.width as usize).saturating_sub(used);
        let x = area.x + spans_width(&spans) as u16;
        spans.extend(self.tabs(room, Position::new(x, area.y)));
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        let gauge = Paragraph::new(Line::from(gauge)).alignment(Alignment::Right);
        frame.render_widget(gauge, area);
    }

    /// Onglets d'écran ; sans les touches (F6…) s'ils ne tiennent pas
    /// dans `room` colonnes (la jauge fear reste entière à 80 colonnes).
    /// Enregistre leurs rectangles (`tab_hits`, clic = écran, voir `mouse`).
    fn tabs(&mut self, room: usize, at: Position) -> Vec<Span<'static>> {
        let build = |keys: bool| -> Vec<Span<'static>> {
            TABS.iter()
                .flat_map(|&(screen, label, key)| {
                    tab(label, if keys { key } else { "" }, self.screen == screen)
                })
                .collect()
        };
        let keys = spans_width(&build(true)) <= room;
        let mut spans = Vec::new();
        let mut x = at.x;
        self.tab_hits.clear();
        for &(screen, label, key) in TABS.iter() {
            let t = tab(label, if keys { key } else { "" }, self.screen == screen);
            let w = spans_width(&t);
            self.tab_hits
                .push((screen, Rect::new(x, at.y, w as u16, 1)));
            x = x.saturating_add(w as u16);
            spans.extend(t);
        }
        spans
    }

    /// Barre d'état : pastilles powerline à gauche, raccourcis à droite.
    /// Sous 80 colonnes : pastilles courtes (rate compact, modèle à 10,
    /// mode court) pour tenir à 70.
    fn draw_status(&self, frame: &mut Frame, area: Rect) {
        let compact = area.width < COMPACT_W;
        let sess = self.sessions.current();
        let mut left = Vec::new();
        let chat = format!(
            "\u{f02a0} chat {}/{}",
            self.sessions.index() + 1,
            self.sessions.count()
        );
        left.extend(pill(&chat, PUMPKIN, VOID));
        let model = short_model(self.effective_model_ref());
        let model = if compact {
            model.chars().take(10).collect::<String>()
        } else {
            model.into_owned()
        };
        left.extend(pill(&model, UMBER, BONE));
        let rate = if compact {
            format!(
                "{} tok · {}",
                sess.ledger.total(),
                fmt_tps(sess.ledger.last_tps())
            )
        } else {
            format!(
                "{} tok · {} total · {}",
                sess.ledger.total(),
                self.sessions.grand_total(),
                fmt_tps(sess.ledger.last_tps())
            )
        };
        left.extend(pill(&rate, theme::lerp(VOID, UMBER, 0.5), SAND));
        let used: usize = left.iter().map(|s| s.width()).sum();
        frame.render_widget(Paragraph::new(Line::from(left)), area);
        let width = area.width as usize;
        for hints in [key_hints(false), key_hints(true)] {
            let hints_w: usize = hints.iter().map(|s| s.width()).sum();
            if used + hints_w + 2 <= width {
                let right = Paragraph::new(Line::from(hints)).alignment(Alignment::Right);
                frame.render_widget(right, area);
                break;
            }
        }
    }
}

/// Largeur du panneau droit : 30 % borné 24..=56, ajusté de `delta`
/// (Ctrl+←/→), borné 16..=80 sans manger les `keep` cellules du reste.
fn right_width(total: u16, keep: u16, delta: i16) -> u16 {
    let auto = (total * 3 / 10).clamp(24, 56);
    let max = total.saturating_sub(keep).max(16);
    let tuned = (auto as i16 + delta).clamp(16, 80) as u16;
    tuned.min(max)
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
/// `key` est un littéral statique (emprunté, jamais copié).
fn tab(label: &str, key: &'static str, active: bool) -> Vec<Span<'static>> {
    let mut spans = if active {
        pill(label, PUMPKIN, VOID)
    } else {
        vec![Span::styled(format!(" {label} "), Style::new().fg(SAND))]
    };
    if !key.is_empty() {
        spans.push(Span::styled(key, Style::new().fg(UMBER)));
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

/// Fear gauge: `fear ██████▍      42%`, candle to blood.
/// `used` = estimated tokens of the current chat, `max` = model window.
fn fear_gauge(used: u64, max: u64) -> Vec<Span<'static>> {
    let max = max.max(1);
    let ratio = used as f32 / max as f32;
    let filled = ratio.min(1.0) * FEAR_W as f32;
    let track = Style::new().bg(theme::lerp(VOID, UMBER, 0.35));
    let mut spans = Vec::with_capacity(FEAR_W + 2);
    spans.push(Span::styled("fear ", Style::new().fg(SAND)));
    for i in 0..FEAR_W {
        let part = (filled - i as f32).clamp(0.0, 1.0);
        let glyph = EIGHTHS[(part * 8.0).round() as usize];
        let fg = theme::lerp(CANDLE, BLOOD, i as f32 / (FEAR_W - 1) as f32);
        spans.push(Span::styled(glyph, track.fg(fg)));
    }
    let pct = (used.saturating_mul(100) / max).min(999);
    let colour = match pct {
        80.. => BLOOD,
        40.. => CANDLE,
        _ => ASH,
    };
    let num = Style::new().fg(colour).add_modifier(Modifier::BOLD);
    spans.push(Span::styled(format!("{pct:>3}% "), num));
    spans
}

/// Raccourcis rappelés dans la barre d'état (courts si `short`).
fn key_hints(short: bool) -> Vec<Span<'static>> {
    let key = Style::new().fg(PUMPKIN).add_modifier(Modifier::BOLD);
    if short {
        return vec![Span::styled("F5 F6", key)];
    }
    let word = Style::new().fg(ASH);
    let mut spans = Vec::with_capacity(6);
    for (k, w) in [
        ("F5", " scare  "),
        ("Ctrl+←/→", " column  "),
        ("q", " quit  "),
    ] {
        spans.push(Span::styled(k, key));
        spans.push(Span::styled(w, word));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Role;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn fear_gauge_is_fixed_width_and_monotonic() {
        let width = |u| fear_gauge(u, 1000).iter().map(|s| s.width()).sum::<usize>();
        assert_eq!(width(0), width(1000));
        assert_eq!(width(0), width(9999));
        assert!(fear_gauge(420, 1000)
            .iter()
            .any(|s| s.content.contains('%')));
        let full = |u| {
            fear_gauge(u, 1000)
                .iter()
                .filter(|s| s.content == "█")
                .count()
        };
        assert!(full(0) <= full(500) && full(500) <= full(1000));
        assert_eq!(full(1000), FEAR_W);
        assert_eq!(full(5000), FEAR_W);
    }

    #[test]
    fn colonne_droite_delta_borne_et_reste_preserve() {
        // Sans réglage : comportement auto inchangé (30 %, 24..=56).
        assert_eq!(right_width(80, 54, 0), 24);
        assert_eq!(right_width(200, 58, 0), 56);
        assert_eq!(right_width(80, 40, 0), 24);
        // Delta borné 16..=80.
        assert_eq!(right_width(200, 58, 40), 80);
        assert_eq!(right_width(200, 58, -50), 16);
        // Le reste est préservé : chat 80 (18+36), almanach 80 (40).
        assert_eq!(right_width(80, 54, 64), 26);
        assert_eq!(right_width(80, 40, 64), 40);
        // Petit terminal : plancher 16 même si le reste est serré.
        assert_eq!(right_width(60, 54, -64), 16);
    }

    #[test]
    fn header_fits_three_tabs_and_gauge_at_80_columns() {
        let mut app = App::offline().unwrap();
        for w in [70, 80, 120] {
            let mut term = Terminal::new(TestBackend::new(w, 24)).unwrap();
            term.draw(|f| app.draw(f)).unwrap();
            let buf = term.backend().buffer();
            let head: String = (0..w).map(|x| buf[(x, 0)].symbol()).collect();
            for word in ["chat", "bestiary", "hub", "fear"] {
                assert!(head.contains(word), "{w} cols: {word} missing in {head:?}");
            }
            assert!(!head.contains("grimoire"), "{w} cols: {head:?}");
            assert!(!head.contains("almanac"), "{w} cols: {head:?}");
            // 3 onglets : les touches (F6/F7) tiennent dès 80 colonnes.
            if w >= 80 {
                assert!(head.contains("F7"), "{w} cols: {head:?}");
            }
        }
    }

    #[test]
    fn every_screen_renders_at_common_sizes() {
        let mut app = App::offline().unwrap();
        app.sessions
            .current_mut()
            .chat
            .push(Role::Assistant, "les os craquent sous la lune");
        for (w, h) in [(70, 17), (80, 24), (120, 40), (200, 60), (60, 20)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            for screen in [Screen::Chat, Screen::Bestiary, Screen::Hub] {
                app.screen = screen;
                app.spook();
                app.tick();
                term.draw(|f| app.draw(f)).unwrap();
            }
        }
    }

    #[test]
    fn minimum_70x17_shows_ui_not_too_small() {
        let mut app = App::offline().unwrap();
        for screen in [Screen::Chat, Screen::Bestiary, Screen::Hub] {
            app.screen = screen;
            let mut term = Terminal::new(TestBackend::new(70, 17)).unwrap();
            term.draw(|f| app.draw(f)).unwrap();
            let buf = term.backend().buffer().clone();
            let top: String = (0..70).map(|x| buf[(x, 0)].symbol()).collect();
            assert!(top.contains("CRYPT"), "header absent à 70x17 : {top:?}");
            assert!(!top.contains("too small"), "gate à 70x17 : {top:?}");
            let filled = buf.content.iter().filter(|c| c.symbol() != " ").count();
            assert!(filled > 60, "frame presque vide à 70x17 : {filled}");
        }
        for (w, h) in [(69, 17), (70, 16)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| app.draw(f)).unwrap();
            let buf = term.backend().buffer().clone();
            let top: String = (0..w).map(|x| buf[(x, 0)].symbol()).collect();
            assert!(top.contains("too small"), "pas de gate à {w}x{h} : {top:?}");
        }
    }
}
