//! Sidebar « cercueils » : liste des chats + bouton [+] nouveau chat.
//!
//! `Sessions::draw_sidebar` rend une ligne par chat (point `●` vert au
//! repos / bleu en réflexion + nom + tokens + `…` si envoi en cours +
//! ` A{n}` si sous-agents actifs), le chat courant mis en relief, puis
//! le bouton [+] (clic = nouveau chat, voir `mouse`).

use std::borrow::Cow;

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
const PLUS_REST: &str = "new chat";

/// Zones cliquables de la sidebar, mémorisées au rendu (voir `mouse`).
pub struct SidebarHits {
    /// Bloc des lignes de chats (une ligne par chat, clic = choisir).
    pub chats: Option<Rect>,
    /// Bouton [+] (clic = nouveau chat).
    pub plus: Option<Rect>,
}

impl Sessions {
    /// Sidebar « cercueils » : une ligne par chat (point `●` vert au
    /// repos / bleu en réflexion + nom + tokens + `…` si envoi + ` A{n}`),
    /// le chat courant marqué d'une barre citrouille, puis le bouton [+]
    /// (clic = nouveau chat). Rend ses zones cliquables (`None` si
    /// rognées ou si la sidebar est trop étroite).
    pub fn draw_sidebar(&self, frame: &mut Frame, area: Rect) -> SidebarHits {
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(theme::UMBER))
            .title(theme::title("coffins", theme::SAND, theme::BONE));
        let inner_w = area.width.saturating_sub(2) as usize;
        if inner_w < 3 {
            frame.render_widget(block, area);
            return SidebarHits {
                chats: None,
                plus: None,
            };
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
        SidebarHits {
            chats: chat_rows_rect(area, self.count()),
            plus,
        }
    }
}

/// Bloc des lignes de chats : une ligne par chat visible depuis le haut
/// intérieur (`None` si aucune ligne visible — sidebar naine ou étroite).
fn chat_rows_rect(area: Rect, chats: usize) -> Option<Rect> {
    let (w, h) = (area.width.saturating_sub(2), area.height.saturating_sub(2));
    let rows = (chats as u16).min(h);
    if w < 3 || rows == 0 {
        None
    } else {
        Some(Rect::new(area.x + 1, area.y + 1, w, rows))
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

/// Ligne bouton : `[+]` citrouille gras + `new chat` sable.
/// Sidebar étroite (< 8) : `[+]` seul, sinon emprunté, jamais copié.
fn plus_line(width: usize) -> Line<'static> {
    let avail = width.saturating_sub(PLUS_HEAD.len());
    let rest: Cow<'static, str> = if width < 8 {
        Cow::Borrowed("")
    } else if PLUS_REST.len() <= avail {
        Cow::Borrowed(PLUS_REST)
    } else {
        Cow::Owned(PLUS_REST.chars().take(avail).collect())
    };
    let head = Style::new().fg(theme::PUMPKIN).add_modifier(Modifier::BOLD);
    Line::from(vec![
        Span::styled(PLUS_HEAD, head),
        Span::styled(rest, Style::new().fg(theme::SAND)),
    ])
}

/// Point de statut + espace : 2 colonnes réservées dans la ligne.
const DOT_W: usize = 2;

/// Point de statut : vert mousse au repos, bleu feu-follet en réflexion.
fn status_dot(sending: bool) -> Span<'static> {
    let colour = if sending { theme::WISP } else { theme::MOSS };
    Span::styled("● ", Style::new().fg(colour))
}

/// Nom tronqué à `name_w` colonnes + bourrage d'espaces (largeur fixe).
fn fit_name(name: &str, name_w: usize) -> (String, String) {
    let mut count = 0usize;
    let mut end = 0usize;
    for (i, ch) in name.char_indices() {
        if count == name_w {
            break;
        }
        end = i + ch.len_utf8();
        count += 1;
    }
    (name[..end].to_string(), " ".repeat(name_w - count))
}

/// Une ligne de sidebar : `▍` + point `●` (vert au repos, bleu en
/// réflexion) + nom tronqué + `123t` + `…` si envoi + ` A{n}`.
/// Le chat courant : barre citrouille, nom os gras, fond braise.
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
    // `tokens`/`agents` ASCII : `len()` vaut la largeur ; `…` = 1 colonne.
    let used = 2 + DOT_W + tokens.len() + agents.len() + usize::from(sending);
    let (name, pad) = fit_name(name, width.saturating_sub(used));
    let (bar, name_style) = if current {
        let bold = Style::new().fg(theme::BONE).add_modifier(Modifier::BOLD);
        (Style::new().fg(theme::PUMPKIN), bold)
    } else {
        (Style::new().fg(theme::UMBER), Style::new().fg(theme::SAND))
    };
    let line = Line::from(vec![
        Span::styled("▍ ", bar),
        status_dot(sending),
        Span::styled(name, name_style),
        Span::styled(pad, name_style),
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
    fn lignes_chats_une_ligne_par_chat_visible() {
        let area = Rect::new(0, 1, 18, 10);
        assert_eq!(chat_rows_rect(area, 1), Some(Rect::new(1, 2, 16, 1)));
        assert_eq!(chat_rows_rect(area, 3), Some(Rect::new(1, 2, 16, 3)));
        assert_eq!(chat_rows_rect(area, 20), Some(Rect::new(1, 2, 16, 8))); // rogné
        assert_eq!(chat_rows_rect(Rect::new(0, 1, 4, 10), 1), None); // étroit
        assert_eq!(chat_rows_rect(Rect::new(0, 1, 18, 2), 1), None); // nain
    }

    #[test]
    fn point_vert_au_repos_bleu_en_reflexion() {
        for (sending, want) in [(false, theme::MOSS), (true, theme::WISP)] {
            let line = sidebar_line(false, "Chat 1", 12, sending, 0, 16);
            let dot = line.spans.iter().find(|s| s.content.as_ref() == "● ");
            assert_eq!(
                dot.and_then(|s| s.style.fg),
                Some(want),
                "sending={sending}"
            );
        }
    }

    #[test]
    fn draw_sidebar_montre_le_point_de_chaque_chat() {
        use ratatui::{backend::TestBackend, Terminal};
        let mut s = Sessions::new();
        s.add();
        s.get_mut(1).unwrap().sending = true;
        let mut term = Terminal::new(TestBackend::new(18, 10)).unwrap();
        term.draw(|f| {
            s.draw_sidebar(f, f.area());
        })
        .unwrap();
        let buf = term.backend().buffer();
        assert_eq!(buf[(3, 1)].symbol(), "●");
        assert_eq!(buf[(3, 1)].fg, theme::MOSS);
        assert_eq!(buf[(3, 2)].symbol(), "●");
        assert_eq!(buf[(3, 2)].fg, theme::WISP);
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
        assert!(row.starts_with("[+] new chat"), "plus row: {row:?}");
    }
}
