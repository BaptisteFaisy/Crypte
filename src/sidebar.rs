//! Colonnes cercueils + discussion : liste des chats + bouton [+]
//! nouveau chat (`draw_sidebar`), chat courant ou invite vide
//! (`draw_main`, aucun chat : dernier fermé).
//!
//! `Sessions::draw_sidebar` rend une ligne par chat (point `●` vert au
//! repos / bleu en réflexion + nom + `…` si envoi en cours +
//! ` A{n}` si sous-agents actifs + bouton `[×]` sang pour fermer), le chat
//! courant mis en relief, puis le bouton [+] (clic = nouveau chat,
//! clic [×] = fermer, voir `mouse`).

use std::borrow::Cow;

use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::chat::ChatView;
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
    /// Bouton [×] de chaque chat (clic = fermer, `None` si masqué).
    pub closes: Vec<Option<Rect>>,
}

impl Sessions {
    /// Sidebar « cercueils » : une ligne par chat (point `●` vert au
    /// repos / bleu en réflexion + nom + `…` si envoi + ` A{n}` + bouton
    /// `[×]`), le chat courant marqué d'une barre citrouille, puis le
    /// bouton [+] (clic = nouveau chat). Rend ses zones cliquables
    /// (`None` si rognées ou si la sidebar est trop étroite).
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
                closes: Vec::new(),
            };
        }
        let inner_h = area.height.saturating_sub(2) as usize;
        let mut lines: Vec<Line<'static>> = Vec::with_capacity(self.count() + 1);
        let mut closes: Vec<Option<Rect>> = Vec::with_capacity(self.count());
        for (i, c) in self.iter().enumerate() {
            let (line, fits) = sidebar_line(
                i == self.index(),
                &c.name,
                c.sending,
                self.running_agents(i),
                c.pending.is_some(),
                inner_w,
            );
            lines.push(line);
            closes.push(close_rect(area, i, inner_h, fits));
        }
        let plus = plus_rect(area, self.count());
        if plus.is_some() {
            lines.push(plus_line(inner_w));
        }
        frame.render_widget(Paragraph::new(lines).block(block), area);
        SidebarHits {
            chats: chat_rows_rect(area, self.count()),
            plus,
            closes,
        }
    }

    /// Colonne discussion : chat courant, ou invite vide si aucun chat
    /// (dernier fermé : Ctrl+N ou [+] pour recommencer).
    pub fn draw_main(&mut self, tick: u64, frame: &mut Frame, area: Rect) {
        if self.count() == 0 {
            let hint = Paragraph::new("no chat — Ctrl+N or click [+] new chat")
                .style(Style::new().fg(theme::SAND))
                .alignment(Alignment::Center);
            frame.render_widget(hint, area);
            return;
        }
        let sess = self.current_mut();
        let view = ChatView {
            title: &sess.name,
            tick,
            sending: sess.sending,
            question_pending: sess.pending.is_some(),
            agents: crate::agent_view::AgentCtx {
                list: &sess.agents,
                cursor: sess.agent_cursor,
                expanded: &sess.expanded_agents,
            },
        };
        sess.chat.draw(frame, area, view);
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

/// Bouton `[×]` : 3 cases finissant 1 colonne avant la bordure droite
/// (marge anti-bord), ligne `idx` (`None` si masqué — rogné ou étroit).
fn close_rect(area: Rect, idx: usize, inner_h: usize, fits: bool) -> Option<Rect> {
    if fits && idx < inner_h {
        Some(Rect::new(
            area.x + area.width - 2 - CLOSE_BTN_W as u16,
            area.y + 1 + idx as u16,
            CLOSE_BTN_W as u16,
            1,
        ))
    } else {
        None
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

/// Sidebar : 10 cellules min (noms encore lisibles).
const SIDEBAR_MIN: u16 = 10;

/// Largeur auto de la sidebar : 22 si ≥ 110 col., 18 sinon, 14 compact.
pub fn side_auto(total: u16, compact: bool) -> u16 {
    if compact {
        14
    } else if total >= 110 {
        22
    } else {
        18
    }
}

/// Largeur de la sidebar : auto + delta du drag, clampée (≥ 10, chat
/// ≥ 30/36 et colonne droite ≥ 16 préservés, miroir de `right_width`).
pub fn side_width(total: u16, delta: i16, compact: bool) -> u16 {
    let keep = if compact { 30 } else { 36 + 16 };
    let max = total.saturating_sub(keep).max(SIDEBAR_MIN);
    let tuned = (side_auto(total, compact) as i16 + delta).clamp(SIDEBAR_MIN as i16, 80) as u16;
    tuned.min(max)
}

/// Poignée de redimensionnement : 3 crans `┃` sur le bord droit de la
/// sidebar, citrouille si active (survol/drag), ombre sinon.
pub fn draw_side_grip(frame: &mut Frame, side: Rect, active: bool) {
    if side.height < 5 {
        return;
    }
    let x = side.x + side.width.saturating_sub(1);
    let mid = side.y + side.height / 2;
    let fg = if active { theme::PUMPKIN } else { theme::UMBER };
    for dy in 0..3 {
        let y = mid + dy - 1;
        if y > side.y && y < side.y + side.height - 1 {
            if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                cell.set_symbol("┃").set_fg(fg);
            }
        }
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

/// Bouton de fermeture : `[×]` sang gras + 1 espace de marge au bord.
const CLOSE_TXT: &str = "[×] ";
/// Largeur du bouton cliquable `[×]` (3 cases, marge exclue).
const CLOSE_BTN_W: usize = 3;
/// Colonnes réservées à la fermeture (bouton + marge anti-bord).
const CLOSE_W: usize = 4;

/// Une ligne de sidebar : `▍` + point `●` (vert au repos, bleu en
/// réflexion) + nom tronqué + `…` si envoi + ` A{n}` + ` ?` si question
/// IA en attente (bougie) + `[×]` + marge. Le chat courant : barre
/// citrouille, nom os gras, fond braise.
/// (Les compteurs tokens vivent dans `/usage`, pas ici.)
/// Rend aussi si la croix tient (`false` : omise, pas de clic).
fn sidebar_line(
    current: bool,
    name: &str,
    sending: bool,
    agents: usize,
    question: bool,
    width: usize,
) -> (Line<'static>, bool) {
    let agents = if agents > 0 {
        format!(" A{agents}")
    } else {
        String::new()
    };
    let suffix = if sending { "…" } else { "" };
    let mark = if question { " ?" } else { "" };
    // `agents` ASCII : `len()` vaut la largeur ; `…`/`×`/`?` = 1 colonne.
    let used = 2 + DOT_W + agents.len() + usize::from(sending) + mark.len() + CLOSE_W;
    let (name, pad) = fit_name(name, width.saturating_sub(used));
    let (bar, name_style) = if current {
        let bold = Style::new().fg(theme::BONE).add_modifier(Modifier::BOLD);
        (Style::new().fg(theme::PUMPKIN), bold)
    } else {
        (Style::new().fg(theme::UMBER), Style::new().fg(theme::SAND))
    };
    let fits = used <= width;
    let mut spans = vec![
        Span::styled("▍ ", bar),
        status_dot(sending),
        Span::styled(name, name_style),
        Span::styled(pad, name_style),
        Span::styled(agents, Style::new().fg(theme::ECTO)),
        Span::styled(suffix, Style::new().fg(theme::CANDLE)),
        Span::styled(mark, Style::new().fg(theme::CANDLE)),
    ];
    if fits {
        let close = Style::new().fg(theme::BLOOD).add_modifier(Modifier::BOLD);
        spans.push(Span::styled(CLOSE_TXT, close));
    }
    let line = Line::from(spans);
    let line = if current {
        line.style(Style::new().bg(theme::lerp(theme::VOID, theme::DRIED, 0.55)))
    } else {
        line
    };
    (line, fits)
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
            let (line, _) = sidebar_line(false, "Chat 1", sending, 0, false, 16);
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
    fn ligne_chat_sans_compteur_tokens() {
        use ratatui::{backend::TestBackend, Terminal};
        let mut s = Sessions::new();
        s.current_mut().ledger.add(100, 23); // 123 tokens au compteur
        let mut term = Terminal::new(TestBackend::new(18, 10)).unwrap();
        term.draw(|f| {
            s.draw_sidebar(f, f.area());
        })
        .unwrap();
        let buf = term.backend().buffer();
        let row: String = (1u16..17).map(|x| buf[(x, 1)].symbol()).collect();
        assert!(row.ends_with("[×] "), "bouton manquant : {row:?}");
        assert!(!row.contains("123"), "compteur tokens affiché : {row:?}");
    }

    #[test]
    fn croix_sang_en_fin_de_ligne_si_ca_tient() {
        let (line, fits) = sidebar_line(false, "Chat 1", false, 0, false, 16);
        assert!(fits);
        let last = line.spans.last().unwrap();
        assert_eq!(last.content.as_ref(), "[×] ");
        assert_eq!(last.style.fg, Some(theme::BLOOD));
        // Trop étroit : bouton omis, pas de clic.
        let (tight, fits) = sidebar_line(false, "Chat 1", false, 0, false, 7);
        assert!(!fits);
        assert!(tight.spans.iter().all(|s| s.content.as_ref() != "[×] "));
    }

    #[test]
    fn draw_sidebar_memorise_une_croix_par_chat_visible() {
        use ratatui::{backend::TestBackend, Terminal};
        let mut s = Sessions::new();
        s.add();
        let mut term = Terminal::new(TestBackend::new(18, 10)).unwrap();
        let mut hits = None;
        term.draw(|f| {
            hits = Some(s.draw_sidebar(f, f.area()));
        })
        .unwrap();
        // Bouton [×] (x=13..=15) + marge (x=16), lignes 1 et 2.
        assert_eq!(
            hits.as_ref().unwrap().closes,
            vec![Some(Rect::new(13, 1, 3, 1)), Some(Rect::new(13, 2, 3, 1))]
        );
        let buf = term.backend().buffer();
        for y in [1, 2] {
            assert_eq!(buf[(13, y)].symbol(), "[");
            assert_eq!(buf[(14, y)].symbol(), "×");
            assert_eq!(buf[(15, y)].symbol(), "]");
            assert_eq!(buf[(16, y)].symbol(), " ", "marge au bord absente");
        }
        // Étroit : croix masquées, aucun rect.
        let mut term = Terminal::new(TestBackend::new(7, 10)).unwrap();
        term.draw(|f| {
            hits = Some(s.draw_sidebar(f, f.area()));
        })
        .unwrap();
        assert_eq!(hits.unwrap().closes, vec![None, None]);
    }

    #[test]
    fn largeur_auto_delta_et_clamp() {
        assert_eq!(side_width(120, 0, false), 22);
        assert_eq!(side_width(100, 0, false), 18);
        assert_eq!(side_width(70, 0, true), 14);
        assert_eq!(side_width(120, 10, false), 32); // drag élargit
        assert_eq!(side_width(120, -4, false), 18); // drag rétrécit
        assert_eq!(side_width(120, -64, false), 10); // plancher
                                                     // Plafond : chat 36 + droit 16 préservés (120 − 52 = 68).
        assert_eq!(side_width(120, 64, false), 68);
        // Compact : chat 30 préservé (70 − 30 = 40).
        assert_eq!(side_width(70, 64, true), 40);
        // Poignée : 3 crans sur le bord droit, citrouille si active.
        use ratatui::{backend::TestBackend, Terminal};
        let mut term = Terminal::new(TestBackend::new(30, 10)).unwrap();
        term.draw(|f| draw_side_grip(f, Rect::new(0, 0, 20, 10), true))
            .unwrap();
        let buf = term.backend().buffer();
        for y in 4..=6 {
            assert_eq!(buf[(19, y)].symbol(), "┃", "cran manquant en y={y}");
            assert_eq!(buf[(19, y)].fg, theme::PUMPKIN);
        }
    }

    #[test]
    fn sans_chat_seul_le_bouton_plus_en_premiere_ligne() {
        use crate::sessions::CloseOutcome;
        use ratatui::{backend::TestBackend, Terminal};
        let mut s = Sessions::new();
        assert_eq!(s.close(0), CloseOutcome::Closed);
        let mut term = Terminal::new(TestBackend::new(18, 10)).unwrap();
        let mut hits = None;
        term.draw(|f| {
            hits = Some(s.draw_sidebar(f, f.area()));
        })
        .unwrap();
        let hits = hits.unwrap();
        assert!(hits.chats.is_none(), "aucune ligne de chat");
        assert!(hits.closes.is_empty(), "aucune croix");
        let plus = hits.plus.expect("[+] affiché");
        assert_eq!((plus.x, plus.y), (1, 1)); // première ligne intérieure
        let buf = term.backend().buffer();
        let row: String = (1u16..17).map(|x| buf[(x, 1)].symbol()).collect();
        assert!(row.starts_with("[+] new chat"), "plus row: {row:?}");
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
