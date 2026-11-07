//! Écran « Bestiaire » (F6) : les 33 créatures en cartes, paginées.
//!
//! Flèches pour choisir ; la carte choisie s'éclaire à la citrouille et
//! montre sa légende. Les images passent par [`ImageBank`] (Kitty dans
//! Ghostty, demi-blocs ailleurs).

use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::bestiary::{Creature, BESTIARY};
use crate::gfx::ImageBank;
use crate::theme;

/// Taille minimale d'une carte (bordures comprises).
const CARD_W: u16 = 19;
const CARD_H: u16 = 10;

/// Sélection courante dans le bestiaire.
#[derive(Debug, Default)]
pub struct Galerie {
    selected: usize,
    per_row: usize,
}

impl Galerie {
    /// Galerie sur la première créature.
    pub fn new() -> Self {
        Self::default()
    }

    /// Déplace la sélection de `dx` cartes et `dy` rangées (cyclique).
    pub fn step(&mut self, dx: isize, dy: isize) {
        let n = BESTIARY.len() as isize;
        let per_row = self.per_row.max(1) as isize;
        let next = self.selected as isize + dx + dy * per_row;
        self.selected = next.rem_euclid(n) as usize;
    }

    /// Dessine la page qui contient la créature choisie + le pied de page.
    pub fn draw(&mut self, frame: &mut Frame, area: Rect, bank: &mut ImageBank) {
        if area.height < 3 {
            return;
        }
        let grid = Rect {
            height: area.height - 1,
            ..area
        };
        let cols = (grid.width / CARD_W).max(1);
        let rows = (grid.height / CARD_H).max(1);
        self.per_row = usize::from(cols);
        let per_page = usize::from(cols * rows);
        let page = self.selected / per_page;
        let (card_w, card_h) = (grid.width / cols, grid.height / rows);
        for slot in 0..per_page {
            let i = page * per_page + slot;
            if i >= BESTIARY.len() {
                break;
            }
            let (c, r) = ((slot % self.per_row) as u16, (slot / self.per_row) as u16);
            let rect = Rect::new(grid.x + c * card_w, grid.y + r * card_h, card_w, card_h);
            draw_card(frame, rect, i, i == self.selected, bank);
        }
        let pages = BESTIARY.len().div_ceil(per_page);
        let footer = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
        let line = footer_line(&BESTIARY[self.selected], page + 1, pages);
        frame.render_widget(line, footer);
    }
}

/// Une carte : bordure arrondie, nom en titre, sprite, numéro en bas.
fn draw_card(frame: &mut Frame, rect: Rect, i: usize, selected: bool, bank: &mut ImageBank) {
    let creature = &BESTIARY[i];
    let (border, name) = if selected {
        (
            theme::PUMPKIN,
            theme::title(creature.name, theme::PUMPKIN, theme::CANDLE),
        )
    } else {
        let style = Style::new().fg(theme::SAND);
        (
            theme::UMBER,
            Line::styled(format!(" {} ", creature.name), style),
        )
    };
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(border))
        .title(name)
        .title_bottom(Line::styled(
            format!(" {:02} ", i + 1),
            Style::new().fg(theme::ASH),
        ));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    bank.draw(frame, inner, creature);
}

/// Pied de page : légende de la créature choisie, aide clavier, page.
fn footer_line(chosen: &Creature, page: usize, pages: usize) -> Paragraph<'static> {
    let key = Style::new().fg(theme::PUMPKIN).add_modifier(Modifier::BOLD);
    let dim = Style::new().fg(theme::ASH);
    let lore = Style::new()
        .fg(theme::CANDLE)
        .add_modifier(Modifier::ITALIC);
    Paragraph::new(Line::from(vec![
        Span::styled(format!("{} ", chosen.name), key),
        Span::styled(format!("— {}   ", chosen.lore), lore),
        Span::styled("←↑↓→", key),
        Span::styled(" select  ", dim),
        Span::styled("F6", key),
        Span::styled(" back  ", dim),
        Span::styled(
            format!("page {page}/{pages} · {} creatures", BESTIARY.len()),
            dim,
        ),
    ]))
    .alignment(Alignment::Center)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn step_wraps_both_ways() {
        let mut g = Galerie::new();
        g.per_row = 4;
        g.step(-1, 0);
        assert_eq!(g.selected, BESTIARY.len() - 1);
        g.step(1, 0);
        assert_eq!(g.selected, 0);
        g.step(0, 1);
        assert_eq!(g.selected, 4);
    }

    #[test]
    fn draws_every_page_without_panic() {
        let mut bank = ImageBank::headless();
        let mut g = Galerie::new();
        let mut term = Terminal::new(TestBackend::new(80, 22)).unwrap();
        for _ in 0..BESTIARY.len() {
            term.draw(|f| g.draw(f, f.area(), &mut bank)).unwrap();
            g.step(1, 0);
        }
        assert_eq!(g.selected, 0);
    }
}
