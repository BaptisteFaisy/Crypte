//! Boutique de la crypte : les tokens consommés deviennent des pièces.
//!
//! 1 token consommé (tous chats et sous-agents inclus) = 1 pièce,
//! créditée dans le portefeuille pour la session en cours. La boutique
//! vend des bonus d'ambiance (peur, apparitions, éclair) payés en pièces.
//! Navigation au clic (onglets + articles), clavier en secours
//! (flèches + Entrée, F6 pour ouvrir depuis le chat).

use crossterm::event::KeyCode;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, Screen};
use crate::theme;

/// Effet d'ambiance appliqué à l'achat d'un article.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShopEffect {
    /// La peur chute de `n` (plancher 0).
    Calm(u8),
    /// Kiro s'effraie : éclair + apparition surprise.
    Scare,
    /// Une nouvelle créature apparaît au panneau droit.
    Apparition,
    /// Éclair seul, sans effroi.
    Flash,
}

/// Un article : nom, description, prix en pièces, effet.
pub struct ShopItem {
    /// Nom affiché.
    pub name: &'static str,
    /// Description affichée sous le nom.
    pub lore: &'static str,
    /// Prix en pièces (1 token consommé = 1 pièce).
    pub price: u64,
    /// Effet appliqué à l'achat.
    pub effect: ShopEffect,
}

/// Catalogue : 5 bonus d'ambiance, prix croissants.
pub const SHOP: &[ShopItem] = &[
    ShopItem {
        name: "Lightning flash",
        lore: "a flash with no scare",
        price: 200,
        effect: ShopEffect::Flash,
    },
    ShopItem {
        name: "Soothing candle",
        lore: "fear drops by 30",
        price: 300,
        effect: ShopEffect::Calm(30),
    },
    ShopItem {
        name: "Kiro's cry",
        lore: "Kiro panics: flash + apparition",
        price: 500,
        effect: ShopEffect::Scare,
    },
    ShopItem {
        name: "Apparition rite",
        lore: "a new creature emerges",
        price: 700,
        effect: ShopEffect::Apparition,
    },
    ShopItem {
        name: "Holy water",
        lore: "fear drops to zero",
        price: 900,
        effect: ShopEffect::Calm(100),
    },
];

/// Portefeuille : les pièces gagnées valent les tokens consommés de la
/// session, `spent` cumule les pièces dépensées. Solde = gagnées − dépensées.
#[derive(Debug, Default)]
pub struct Wallet {
    spent: u64,
}

impl Wallet {
    /// Portefeuille vide (rien dépensé).
    pub fn new() -> Self {
        Self::default()
    }

    /// Pièces dépensées cette session.
    pub fn spent(&self) -> u64 {
        self.spent
    }

    /// Solde pour `earned` pièces gagnées (jamais négatif).
    pub fn balance(&self, earned: u64) -> u64 {
        earned.saturating_sub(self.spent)
    }

    /// Paie `price` si le solde suffit (rend vrai si débité).
    pub fn spend(&mut self, earned: u64, price: u64) -> bool {
        if self.balance(earned) < price {
            return false;
        }
        self.spent = self.spent.saturating_add(price);
        true
    }
}

/// Hauteur d'une ligne d'article (nom + prix, puis description).
const ROW_H: u16 = 2;

/// État de l'écran boutique : sélection, portefeuille, message.
#[derive(Debug, Default)]
pub struct Shop {
    selected: usize,
    wallet: Wallet,
    message: String,
}

impl Shop {
    /// Boutique sur le premier article, portefeuille vide.
    pub fn new() -> Self {
        Self {
            selected: 0,
            wallet: Wallet::new(),
            message: String::new(),
        }
    }

    /// Portefeuille (solde via `balance`, dépensé via `spent`).
    pub fn wallet(&self) -> &Wallet {
        &self.wallet
    }

    /// Sélectionne l'article `idx` (hors catalogue : ignoré).
    pub fn select(&mut self, idx: usize) {
        if idx < SHOP.len() {
            self.selected = idx;
        }
    }

    /// Déplace la sélection de `delta` articles (cyclique).
    pub fn step(&mut self, delta: isize) {
        let n = SHOP.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(n) as usize;
    }

    /// Achète l'article sélectionné : débite si solvable et rend
    /// l'effet à appliquer (`None` = trop cher ou catalogue vide).
    pub fn buy_selected(&mut self, earned: u64) -> Option<ShopEffect> {
        let item = SHOP.get(self.selected)?;
        if self.wallet.spend(earned, item.price) {
            self.message = format!("{} acquired (−{} coins)", item.name, item.price);
            Some(item.effect)
        } else {
            let missing = item.price - self.wallet.balance(earned);
            self.message = format!("not enough coins (missing {missing})");
            None
        }
    }

    /// Dessine la boutique (portefeuille + articles + message) et rend
    /// les rectangles cliquables des articles (ordre du catalogue).
    pub fn draw(&mut self, frame: &mut Frame, area: Rect, earned: u64) -> Vec<Rect> {
        if area.height < 5 || area.width < 20 {
            let msg = "shop is cramped: enlarge the crypt";
            frame.render_widget(
                Paragraph::new(msg)
                    .style(Style::new().fg(theme::CANDLE))
                    .alignment(Alignment::Center),
                area,
            );
            return Vec::new();
        }
        let balance = self.wallet.balance(earned);
        let [head, list, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(ROW_H),
            Constraint::Length(2),
        ])
        .areas(area);
        frame.render_widget(header(balance, earned, self.wallet.spent()), head);
        let mut rows = Vec::with_capacity(SHOP.len());
        for (i, item) in SHOP.iter().enumerate() {
            let rect = Rect::new(
                list.x,
                list.y + rows.len() as u16 * ROW_H,
                list.width,
                ROW_H,
            );
            if rect.bottom() > list.bottom() {
                break;
            }
            frame.render_widget(item_lines(item, i == self.selected, balance), rect);
            rows.push(rect);
        }
        frame.render_widget(footer(&self.message, foot.width), foot);
        rows
    }
}

/// En-tête : solde en bougie, gagnées/dépensées en cendre.
fn header(balance: u64, earned: u64, spent: u64) -> Paragraph<'static> {
    let line = Line::from(vec![
        Span::styled("◉ wallet: ", Style::new().fg(theme::SAND)),
        Span::styled(
            format!("{balance} coins"),
            Style::new().fg(theme::CANDLE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" ({earned} earned · {spent} spent)"),
            Style::new().fg(theme::ASH),
        ),
    ]);
    Paragraph::new(line).alignment(Alignment::Center)
}

/// Un article : `▶ nom — prix ◉` puis sa description en italique.
/// L'article choisi a un fond braise ; le prix est en bougie si
/// abordable, en sang séché sinon.
fn item_lines(item: &ShopItem, selected: bool, balance: u64) -> Paragraph<'static> {
    let (prefix, name_style) = if selected {
        (
            Span::styled(
                "▶ ",
                Style::new().fg(theme::PUMPKIN).add_modifier(Modifier::BOLD),
            ),
            Style::new().fg(theme::BONE).add_modifier(Modifier::BOLD),
        )
    } else {
        (Span::raw("  "), Style::new().fg(theme::SAND))
    };
    let price_style = if balance >= item.price {
        Style::new().fg(theme::CANDLE)
    } else {
        Style::new().fg(theme::DRIED)
    };
    let lines = vec![
        Line::from(vec![
            prefix,
            Span::styled(item.name, name_style),
            Span::styled(" — ", Style::new().fg(theme::ASH)),
            Span::styled(format!("{} ◉", item.price), price_style),
        ]),
        Line::from(vec![Span::styled(
            format!("  {}", item.lore),
            Style::new().fg(theme::ASH).add_modifier(Modifier::ITALIC),
        )]),
    ];
    let paragraph = Paragraph::new(lines);
    if selected {
        paragraph.style(Style::new().bg(theme::lerp(theme::VOID, theme::DRIED, 0.55)))
    } else {
        paragraph
    }
}

/// Pied : message d'achat (ou rappel du change) + aide clavier/clic.
fn footer(message: &str, width: u16) -> Paragraph<'static> {
    let key = Style::new().fg(theme::PUMPKIN).add_modifier(Modifier::BOLD);
    let dim = Style::new().fg(theme::ASH);
    let first = if message.is_empty() {
        Line::styled(
            "1 token consumed = 1 coin",
            Style::new().fg(theme::SAND).add_modifier(Modifier::ITALIC),
        )
    } else {
        Line::styled(
            message.to_string(),
            Style::new()
                .fg(theme::CANDLE)
                .add_modifier(Modifier::ITALIC),
        )
    };
    let second = if width >= 60 {
        Line::from(vec![
            Span::styled("click", key),
            Span::styled(" buy · ", dim),
            Span::styled("↑↓", key),
            Span::styled(" select · ", dim),
            Span::styled("Enter", key),
            Span::styled(" buy", dim),
        ])
    } else {
        Line::from(vec![
            Span::styled("click/Enter", key),
            Span::styled(" buy", dim),
        ])
    };
    Paragraph::new(vec![first, second]).alignment(Alignment::Center)
}

impl App {
    /// F6 : ouvre la boutique, ou revient au chat.
    pub(crate) fn open_shop(&mut self) {
        if self.screen == Screen::Shop {
            self.screen = Screen::Chat;
        } else {
            self.screen = Screen::Shop;
        }
    }

    /// Touches boutique : flèches (choisir), Entrée (acheter),
    /// F6 (retour chat), F7 (hub), q (fuir).
    pub(crate) fn on_shop_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char('q') => false,
            KeyCode::Up => self.cmd(|s| s.shop.step(-1)),
            KeyCode::Down => self.cmd(|s| s.shop.step(1)),
            KeyCode::Enter => self.cmd(|s| s.buy_selected()),
            KeyCode::F(5) => self.cmd(|s| s.spook()),
            KeyCode::F(6) => self.cmd(|s| s.screen = Screen::Chat),
            KeyCode::F(7) => self.cmd(|s| s.open_hub()),
            _ => true,
        }
    }

    /// Achète l'article sélectionné (Entrée).
    pub(crate) fn buy_selected(&mut self) {
        let earned = self.sessions.grand_total();
        if let Some(effect) = self.shop.buy_selected(earned) {
            self.apply_shop_effect(effect);
        }
    }

    /// Clic sur l'article `idx` : sélectionne et achète aussitôt.
    pub(crate) fn buy_at(&mut self, idx: usize) {
        self.shop.select(idx);
        self.buy_selected();
    }

    /// Applique l'effet d'un achat (peur, apparition, éclair).
    fn apply_shop_effect(&mut self, effect: ShopEffect) {
        match effect {
            ShopEffect::Calm(amount) => self.fx.calm(amount),
            ShopEffect::Scare => self.spook(),
            ShopEffect::Apparition => self.apparition.advance(),
            ShopEffect::Flash => self.fx.flash(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn spend_debite_si_solvable_et_borne_le_solde() {
        let mut wallet = Wallet::new();
        assert_eq!(wallet.balance(1000), 1000);
        assert!(wallet.spend(1000, 300));
        assert_eq!((wallet.spent(), wallet.balance(1000)), (300, 700));
        assert!(!wallet.spend(1000, 800));
        assert_eq!((wallet.spent(), wallet.balance(1000)), (300, 700));
        assert_eq!(wallet.balance(100), 0, "solde jamais négatif");
    }

    #[test]
    fn buy_selected_debite_et_rend_l_effet() {
        let mut shop = Shop::new();
        let effect = shop.buy_selected(1000);
        assert_eq!(effect, Some(ShopEffect::Flash));
        assert_eq!(shop.wallet.spent(), 200);
        assert!(
            shop.message.contains("acquired"),
            "message : {}",
            shop.message
        );
    }

    #[test]
    fn buy_selected_trop_cher_ne_debite_pas() {
        let mut shop = Shop::new();
        shop.select(4); // Eau bénite, 900 pièces.
        assert_eq!(shop.buy_selected(100), None);
        assert_eq!(shop.wallet.spent(), 0);
        assert!(
            shop.message.contains("missing 800"),
            "message : {}",
            shop.message
        );
        shop.select(99); // Hors catalogue : sélection inchangée.
        assert_eq!(shop.buy_selected(100), None);
    }

    #[test]
    fn step_est_cyclique() {
        let mut shop = Shop::new();
        shop.step(-1);
        assert_eq!(shop.buy_selected(0), None); // Dernier article, trop cher.
        assert!(
            shop.message.contains("missing 900"),
            "message : {}",
            shop.message
        );
        shop.step(1);
        assert_eq!(shop.buy_selected(u64::MAX), Some(ShopEffect::Flash));
    }

    #[test]
    fn draw_rend_les_cinq_articles_sans_paniquer() {
        let mut shop = Shop::new();
        for (w, h) in [(80, 24), (70, 17), (120, 40)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| {
                let rows = shop.draw(f, f.area(), 1500);
                assert_eq!(rows.len(), SHOP.len(), "pas 5 lignes à {w}x{h}");
            })
            .unwrap();
        }
        // À l'étroit : message seul, aucune ligne cliquable.
        let mut term = Terminal::new(TestBackend::new(70, 4)).unwrap();
        term.draw(|f| {
            assert!(shop.draw(f, f.area(), 1500).is_empty());
        })
        .unwrap();
    }
}
