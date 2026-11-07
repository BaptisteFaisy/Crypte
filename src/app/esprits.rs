//! Écran « hub des esprits » (F7) : la scène pixel art de `crate::hub`
//! + le panneau des esprits (agents, sous-agents, états) + les touches.

use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use super::{App, Screen};
use crate::hub::{self, Signal};
use crate::sessions::{AgentStatus, ChatSession};
use crate::theme::{self, ASH, BONE, CANDLE, ECTO, PUMPKIN, SAND, SPECTRE, UMBER};

/// Icône fantôme Nerd Font (embarquée par Ghostty).
const GHOST: &str = "\u{f02a0}";

impl App {
    /// F7 : ouvre le hub sur le chat courant, ou revient au chat.
    pub(crate) fn open_hub(&mut self) {
        if self.screen == Screen::Hub {
            self.screen = Screen::Chat;
            return;
        }
        self.hub.selected = self.sessions.index();
        self.hub.sync(&self.sessions);
        self.screen = Screen::Hub;
    }

    /// Touches du hub : flèches (choisir), Entrée (ouvrir le chat), F7 retour.
    pub(crate) fn on_hub_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char('q') => false,
            KeyCode::Left | KeyCode::Up => self.cmd(|s| s.hub_step(-1)),
            KeyCode::Right | KeyCode::Down => self.cmd(|s| s.hub_step(1)),
            KeyCode::Enter => self.cmd(|s| {
                s.sessions.select(s.hub.selected);
                s.screen = Screen::Chat;
            }),
            KeyCode::F(5) => self.cmd(|s| s.spook()),
            KeyCode::F(6) => self.cmd(|s| s.screen = Screen::Bestiary),
            KeyCode::F(7) => self.cmd(|s| s.screen = Screen::Chat),
            _ => true,
        }
    }

    /// Sélection cyclique de l'agent suivant / précédent.
    fn hub_step(&mut self, delta: isize) {
        let n = self.sessions.count() as isize;
        self.hub.selected = (self.hub.selected as isize + delta).rem_euclid(n.max(1)) as usize;
    }

    /// Frame complète + image Kitty animée du hub à écrire après (si le hub
    /// est affiché dans un terminal Kitty).
    pub(crate) fn render_frame(&mut self, frame: &mut Frame) -> Option<String> {
        self.draw(frame);
        self.hub.view.take_pending()
    }

    /// Scène pixel (gauche) + panneau des esprits (droite).
    pub(crate) fn draw_hub(&mut self, frame: &mut Frame, area: Rect) {
        let side_w = (area.width * 3 / 10).clamp(28, 44);
        let [arena, side] =
            Layout::horizontal([Constraint::Min(40), Constraint::Length(side_w)]).areas(area);
        let key = Style::new().fg(PUMPKIN).add_modifier(Modifier::BOLD);
        let dim = Style::new().fg(ASH);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(theme::lerp(UMBER, SPECTRE, 0.35)))
            .title(theme::title("spirit hub", SPECTRE, BONE))
            .title_bottom(Line::from(vec![
                Span::styled(" ←→", key),
                Span::styled(" select  ", dim),
                Span::styled("Enter", key),
                Span::styled(" open chat  ", dim),
                Span::styled("F7", key),
                Span::styled(" back ", dim),
            ]));
        let inner = block.inner(arena);
        frame.render_widget(block, arena);
        let cell = self.bank.kitty_cell();
        hub::render::draw(
            &mut self.hub,
            &self.sessions,
            inner,
            frame.buffer_mut(),
            cell,
        );
        self.bank.protect(inner);
        self.draw_hub_panel(frame, side);
    }

    /// Panneau : chaque agent et ses sous-agents avec leur état, + légende.
    fn draw_hub_panel(&self, frame: &mut Frame, area: Rect) {
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(UMBER))
            .title(theme::title("spirits", SAND, BONE));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let [list, legend] =
            Layout::vertical([Constraint::Min(3), Constraint::Length(6)]).areas(inner);
        let mut lines = census(self.sessions.iter());
        lines.push(Line::raw(""));
        for (i, chat) in self.sessions.iter().enumerate() {
            lines.push(agent_line(chat, i == self.hub.selected));
            for sub in &chat.agents {
                let (label, colour) = sub_state(sub.status);
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(GHOST, Style::new().fg(sub_ink())),
                    Span::styled(format!(" #{} ", sub.id), Style::new().fg(SAND)),
                    Span::styled(label, Style::new().fg(colour)),
                ]));
            }
        }
        let skip = lines.len().saturating_sub(list.height as usize);
        let lines: Vec<Line> = lines.into_iter().skip(skip).collect();
        frame.render_widget(Paragraph::new(lines), list);
        frame.render_widget(Paragraph::new(legend_lines()), legend);
    }

    /// Démo hors ligne (`--smoke`, `--apercu`) : 3 chats, des sous-agents
    /// dans tous les états, un chat en attente, des messages en vol.
    pub(crate) fn populate_hub_demo(&mut self) {
        self.add_chat();
        self.add_chat();
        let names = ["Crypt", "Ossuary", "Belfry"];
        for (i, name) in names.iter().enumerate() {
            if let Some(chat) = self.sessions.get_mut(i) {
                chat.name = (*name).to_string();
            }
        }
        let prompts = [
            "dig through the logs",
            "rewrite the test",
            "read the diff",
            "summarize",
            "translate",
        ];
        let owners = [0, 0, 1, 2, 2];
        let mut ids = Vec::new();
        for (prompt, owner) in prompts.iter().zip(owners) {
            ids.extend(self.sessions.add_agent(owner, prompt, Vec::new()));
        }
        for (id, status) in ids
            .iter()
            .zip([AgentStatus::Done, AgentStatus::Failed])
            .skip(1)
        {
            if let Some((_, agent)) = self.sessions.find_agent_mut(*id) {
                agent.status = status;
            }
        }
        if let Some(chat) = self.sessions.get_mut(1) {
            chat.sending = true;
        }
        self.sessions.select(0);
        self.hub.sync(&self.sessions);
        self.hub.signal(Signal::Tell { from: 0, to: 2 });
        if let Some(&id) = ids.first() {
            self.hub.signal(Signal::Deliver { agent: id, to: 1 });
        }
    }
}

/// Encre des petits fantômes noirs dans le texte (visible sur le fond).
fn sub_ink() -> Color {
    theme::lerp(theme::SPECTRE_DARK, BONE, 0.25)
}

/// « 3 agents · 5 sous-agents » puis « 4 au travail · 1 échoué(s) ».
fn census<'a>(chats: impl Iterator<Item = &'a ChatSession>) -> Vec<Line<'static>> {
    let (mut agents, mut subs, mut busy, mut failed) = (0, 0, 0, 0);
    for chat in chats {
        agents += 1;
        subs += chat.agents.len();
        let count = |st: AgentStatus| chat.agents.iter().filter(|a| a.status == st).count();
        busy += count(AgentStatus::Running);
        failed += count(AgentStatus::Failed);
    }
    let style = Style::new().fg(SAND).add_modifier(Modifier::ITALIC);
    vec![
        Line::styled(format!("{agents} agents · {subs} sub-agents"), style),
        Line::styled(format!("{busy} working · {failed} failed"), style),
    ]
}

/// Ligne d'agent : repère, fantôme blanc, nom, état.
fn agent_line(chat: &ChatSession, selected: bool) -> Line<'static> {
    let marker = if selected { "▶ " } else { "  " };
    let name_style = if selected {
        Style::new().fg(PUMPKIN).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(BONE)
    };
    let (state, colour) = if chat.sending {
        ("awaiting the crypt…", CANDLE)
    } else {
        ("wandering", ASH)
    };
    let name: String = chat.name.chars().take(14).collect();
    Line::from(vec![
        Span::styled(marker, Style::new().fg(PUMPKIN)),
        Span::styled(GHOST, Style::new().fg(BONE)),
        Span::styled(format!(" {name} "), name_style),
        Span::styled(state, Style::new().fg(colour)),
    ])
}

/// Libellé + couleur d'état d'un sous-agent.
fn sub_state(status: AgentStatus) -> (&'static str, Color) {
    match status {
        AgentStatus::Running => ("working", ECTO),
        AgentStatus::Done => ("asleep (done)", SAND),
        AgentStatus::Failed => ("failed", theme::BLOOD),
    }
}

/// Légende du hub.
fn legend_lines() -> Vec<Line<'static>> {
    let row = |icon: &'static str, colour: Color, text: &'static str| {
        Line::from(vec![
            Span::styled(icon, Style::new().fg(colour)),
            Span::styled(format!(" {text}"), Style::new().fg(ASH)),
        ])
    };
    vec![
        Line::styled("legend", Style::new().fg(SAND).add_modifier(Modifier::BOLD)),
        row(GHOST, BONE, "white ghost: agent (a chat)"),
        row(GHOST, sub_ink(), "small black ghost: sub-agent"),
        row("┄", ECTO, "thread: sub-agent at work"),
        row("●", CANDLE, "orb: /tell, /ask, /msg, delivery"),
    ]
}
