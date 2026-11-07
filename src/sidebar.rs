//! Sidebar « cercueils » : liste des chats + bouton [+] nouveau chat.
//!
//! `Sessions::draw_sidebar` rend une ligne par chat (nom + tokens + `…`
//! si envoi en cours + ` A{n}` si sous-agents actifs), le chat courant
//! mis en relief, puis le bouton [+] (clic = nouveau chat, voir `mouse`).

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::sessions::Sessions;
use crate::theme;

/// Bouton [+] : tête citrouille + reste sable (ligne sous les chats).
const PLUS_HEAD: &str = "[+] ";
const PLUS_REST: &str = "nouveau chat";

impl Sessions {
    /// Sidebar « cercueils » : une ligne par chat (nom + tokens + `…` si
    /// envoi + ` A{n}`), le chat courant marqué d'une barre citrouille,
    /// puis le bouton [+] (clic = nouveau chat). Rend son rectangle
    /// (`None` s'il est rogné ou si la sidebar est trop étroite).
    pub fn draw_sidebar(&self, frame: &mut Frame, area: Rect) -> Option<Rect> {
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(theme::UMBER))
            .title(theme::title("cercueils", theme::SAND, theme::BONE));
        let inner_w = area.width.saturating_sub(2) as usize;
        if inner_w < 3 {
            frame.render_widget(block, area);
            return None;
        }
        let mut lines: Vec<Line<'static>> = self
            .iter()
            .enumerate()
            .map(|(i, c)| {
                sidebar_line(
                    i == self.index(),
                    &c.name,
                    c.ledger.total(),
                    c.sending,
                    self.running_agents(i),
                    inner_w,
                )
            })
            .collect();
        let plus = plus_rect(area, self.count());
        if plus.is_some() {
            lines.push(plus_line(inner_w));
        }
        frame.render_widget(Paragraph::new(lines).block(block), area);
        plus
    }
}

/// Rectangle du bouton [+] : ligne sous les chats, pleine largeur
/// intérieure (`None` si rogné — trop de chats — ou sidebar étroite).
fn plus_rect(area: Rect, chats: usize) -> Option<Rect> {
    let (w, h) = (area.width.saturating_sub(2), area.height.saturating_sub(2));
    if w < 3 || chats >= h as usize {
        None
    } else {
        Some(Rect::new(area.x + 1, area.y + 1 + chats as u16, w, 1))
    }
}

/// Ligne bouton : `[+]` citrouille gras + `nouveau chat` sable.
fn plus_line(width: usize) -> Line<'static> {
    let rest: String = PLUS_REST
        .chars()
        .take(width.saturating_sub(PLUS_HEAD.len()))
        .collect();
    let head = Style::new().fg(theme::PUMPKIN).add_modifier(Modifier::BOLD);
    Line::from(vec![
        Span::styled(PLUS_HEAD, head),
        Span::styled(rest, Style::new().fg(theme::SAND)),
    ])
}

/// Une ligne de sidebar : `▍` + nom tronqué + `123t` + `…` si envoi +
/// ` A{n}`. Le chat courant : barre citrouille, nom os gras, fond braise.
fn sidebar_line(
    current: bool,
    name: &str,
    total: u64,
    sending: bool,
    agents: usize,
    width: usize,
) -> Line<'static> {
    let tokens = format!(" {total}t");
    let agents = if agents > 0 {
        format!(" A{agents}")
    } else {
        String::new()
    };
    let suffix = if sending { "…" } else { "" };
    let used = 2 + tokens.chars().count() + agents.chars().count() + suffix.chars().count();
    let name_w = width.saturating_sub(used);
    let name: String = name.chars().take(name_w).collect();
    let pad = " ".repeat(name_w.saturating_sub(name.chars().count()));
    let (bar, name_style) = if current {
        let bold = Style::new().fg(theme::BONE).add_modifier(Modifier::BOLD);
        (Style::new().fg(theme::PUMPKIN), bold)
    } else {
        (Style::new().fg(theme::UMBER), Style::new().fg(theme::SAND))
    };
    let line = Line::from(vec![
        Span::styled("▍ ", bar),
        Span::styled(format!("{name}{pad}"), name_style),
        Span::styled(tokens, Style::new().fg(theme::ASH)),
        Span::styled(agents, Style::new().fg(theme::ECTO)),
        Span::styled(suffix, Style::new().fg(theme::CANDLE)),
    ]);
    if current {
        line.style(Style::new().bg(theme::lerp(theme::VOID, theme::DRIED, 0.55)))
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::Sessions;

    #[test]
    fn plus_sous_les_chats_rogne_si_plein_ou_etroit() {
        let area = Rect::new(0, 1, 18, 10);
        assert_eq!(plus_rect(area, 1), Some(Rect::new(1, 3, 16, 1)));
        assert_eq!(plus_rect(area, 8), None); // 8 lignes = plein
        assert_eq!(plus_rect(Rect::new(0, 1, 4, 10), 1), None); // étroit
    }

    #[test]
    fn draw_sidebar_affiche_le_bouton_plus() {
        use ratatui::{backend::TestBackend, Terminal};
        let s = Sessions::new();
        let mut term = Terminal::new(TestBackend::new(18, 10)).unwrap();
        term.draw(|f| {
            s.draw_sidebar(f, f.area());
        })
        .unwrap();
        let buf = term.backend().buffer();
        let row: String = (1u16..17).map(|x| buf[(x, 2)].symbol()).collect();
        assert!(row.starts_with("[+] nouveau chat"), "ligne + : {row:?}");
    }
}
