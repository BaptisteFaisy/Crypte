//! Souris : molette = défilement de l'historique, clic gauche = bouton [+]
//! (nouveau chat), bouton [×] (fermer le chat), lignes de la sidebar
//! (choisir le chat), onglets de l'en-tête (changer d'écran), articles
//! de la boutique (acheter), drag du bord droit de la sidebar
//! (redimensionner la colonne).
//!
//! La capture souris est activée par `main` (désactivée en sortie) ;
//! les rectangles sont mémorisés à chaque frame (`App::plus_btn`,
//! `App::close_btns`, `App::side_rows`, `App::side_area`, `App::tab_hits`,
//! `App::shop_rows`, `ChatState::area`).

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use crate::app::{App, Screen};
use crate::chat::SCROLL_STEP;

impl App {
    /// Molette : défiler le chat ; pression gauche : poignée sidebar
    /// (drag) ou clic ; drag : redimensionner la sidebar ; release :
    /// fin du drag ; mouvement : éclairer la poignée survolée.
    /// Clic : [+] = nouveau chat, [×] = fermer, ligne = choisir,
    /// onglet = écran, article = acheter (le reste est ignoré).
    pub(crate) fn on_mouse(&mut self, mev: MouseEvent) {
        match mev.kind {
            MouseEventKind::ScrollUp => return self.wheel(mev, -SCROLL_STEP),
            MouseEventKind::ScrollDown => return self.wheel(mev, SCROLL_STEP),
            MouseEventKind::Down(MouseButton::Left) => self.press(mev.column, mev.row),
            MouseEventKind::Drag(MouseButton::Left) => self.drag_to(mev.column),
            MouseEventKind::Up(MouseButton::Left) => self.resizing_side = None,
            MouseEventKind::Moved => self.hover(mev.column, mev.row),
            _ => {}
        }
    }

    /// Pression gauche : poignée sidebar = début de drag, sinon clic
    /// (une pression ailleurs annule aussi un drag resté coincé).
    fn press(&mut self, column: u16, row: u16) {
        if self.side_grab(column, row) {
            return;
        }
        self.resizing_side = None;
        self.click(Position::new(column, row));
    }

    /// Début de drag sur la poignée (bord droit sidebar + bord gauche
    /// chat, 2 colonnes) : mémorise le décalage grab→bord (anti-saut)
    /// et applique le delta. Rend vrai si la poignée est touchée.
    fn side_grab(&mut self, column: u16, row: u16) -> bool {
        let Some(edge) = self.side_edge(row) else {
            return false;
        };
        if column < edge || column > edge + 1 {
            return false;
        }
        self.resizing_side = Some(edge as i16 - column as i16);
        self.drag_to(column);
        true
    }

    /// Drag bouton gauche : le delta suit la souris (largeur − auto,
    /// borné ±64 comme `right_delta`, clamp terminal au rendu).
    /// Sans drag en cours : ignoré (pas de surprise).
    fn drag_to(&mut self, column: u16) {
        let Some(grab) = self.resizing_side else {
            return;
        };
        let Some(side) = self.side_area else {
            return;
        };
        let width = column as i16 + grab - side.x as i16 + 1;
        self.side_delta = (width - self.side_auto as i16).clamp(-64, 64);
    }

    /// Survol sans bouton : éclaire la poignée sous le pointeur.
    fn hover(&mut self, column: u16, row: u16) {
        self.side_hot = self
            .side_edge(row)
            .is_some_and(|edge| column >= edge && column <= edge + 1);
    }

    /// Colonne du bord droit de la sidebar à cette ligne (`None` hors
    /// écran chat, sans sidebar rendue, ou ligne hors sidebar).
    fn side_edge(&self, row: u16) -> Option<u16> {
        if self.screen != Screen::Chat {
            return None;
        }
        self.side_area.and_then(|r| {
            (row >= r.y && row < r.y + r.height).then(|| r.x + r.width.saturating_sub(1))
        })
    }

    /// Clic gauche : [+] = nouveau chat, [×] = fermer le chat, onglet =
    /// changer d'écran, article = acheter, ligne = choisir le chat
    /// (le reste est ignoré).
    fn click(&mut self, at: Position) {
        if self.plus_btn.is_some_and(|r| r.contains(at)) {
            self.add_chat();
            return;
        }
        if let Some(idx) = self
            .close_btns
            .iter()
            .position(|r| r.is_some_and(|r| r.contains(at)))
        {
            self.close_chat(idx);
            return;
        }
        if let Some(tab) = self
            .tab_hits
            .iter()
            .find(|(_, r)| r.contains(at))
            .map(|(s, _)| *s)
        {
            self.goto_tab(tab);
            return;
        }
        if self.screen == Screen::Shop {
            if let Some(idx) = self.shop_rows.iter().position(|r| r.contains(at)) {
                self.buy_at(idx);
                return;
            }
        }
        if let Some(rows) = self.side_rows {
            if rows.contains(at) {
                self.sessions.select((at.y - rows.y) as usize);
            }
        }
    }

    /// Un cran de molette : défile l'historique du chat courant, à
    /// condition d'être sur l'écran chat et dans sa colonne.
    fn wheel(&mut self, mev: MouseEvent, delta: isize) {
        if self.screen != Screen::Chat {
            return;
        }
        let chat = &self.sessions.current().chat;
        if !chat.area.get().contains(Position::new(mev.column, mev.row)) {
            return;
        }
        chat.scroll.by(delta);
    }

    /// Clic sur un onglet : va à son écran (chat, boutique).
    fn goto_tab(&mut self, tab: Screen) {
        match tab {
            Screen::Chat => self.show(Screen::Chat),
            Screen::Settings => self.show(Screen::Settings),
            Screen::Vps => self.show(Screen::Vps),
            Screen::Shop if self.screen == Screen::Haunt => self.show(Screen::Shop),
            Screen::Shop => self.open_shop(),
            Screen::Haunt if self.screen == Screen::Haunt => {}
            Screen::Haunt => self.open_haunt(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderConfig;
    use crossterm::event::KeyModifiers;
    use ratatui::layout::Rect;

    /// App de test (clé factice, aucun appel réseau).
    fn test_app() -> App {
        App::from_config(ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        })
        .unwrap()
    }

    /// Clic gauche en `(col, row)`.
    fn click(col: u16, row: u16) -> MouseEvent {
        event(MouseEventKind::Down(MouseButton::Left), col, row)
    }

    /// Événement souris de type `kind` en `(col, row)`.
    fn event(kind: MouseEventKind, col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: col,
            row,
            modifiers: KeyModifiers::empty(),
        }
    }

    #[test]
    fn clic_plus_cree_un_chat_clic_ailleurs_ignore() {
        let mut app = test_app();
        app.plus_btn = Some(Rect::new(1, 2, 16, 1));
        app.on_mouse(click(5, 2));
        assert_eq!(app.sessions.count(), 2);
        app.on_mouse(click(50, 20));
        assert_eq!(app.sessions.count(), 2);
        app.plus_btn = None;
        app.on_mouse(click(5, 2));
        assert_eq!(app.sessions.count(), 2);
    }

    #[test]
    fn drag_poignee_ajuste_le_delta_sans_saut_ni_chat() {
        let drag = |c: u16, r: u16| event(MouseEventKind::Drag(MouseButton::Left), c, r);
        let mut app = test_app();
        app.side_area = Some(Rect::new(0, 1, 18, 22));
        app.side_auto = 18;
        app.plus_btn = Some(Rect::new(1, 2, 16, 1));
        app.on_mouse(click(17, 10)); // bord droit : grab, pas de chat
        assert_eq!(app.sessions.count(), 1);
        assert!(app.resizing_side.is_some());
        assert_eq!(app.side_delta, 0, "grab sans saut");
        app.on_mouse(drag(25, 10));
        assert_eq!(app.side_delta, 8);
        app.on_mouse(drag(10, 10));
        assert_eq!(app.side_delta, -7);
        app.on_mouse(event(MouseEventKind::Up(MouseButton::Left), 10, 10));
        app.on_mouse(drag(30, 10)); // après release : figé
        assert_eq!(app.side_delta, -7);
        // Grab sur la 2e colonne (bord du chat) : sans saut non plus.
        app.on_mouse(click(18, 10));
        assert_eq!(app.side_delta, 0);
        // Clic [+] juste à côté (dernière colonne intérieure) : nouveau chat.
        app.on_mouse(click(16, 2));
        assert_eq!(app.sessions.count(), 2);
        assert!(app.resizing_side.is_none(), "pression ailleurs annule");
    }

    #[test]
    fn survol_eclaire_poignee_et_hors_chat_ignore() {
        let mut app = test_app();
        app.side_area = Some(Rect::new(0, 1, 18, 22));
        app.side_auto = 18;
        app.on_mouse(event(MouseEventKind::Moved, 17, 10));
        assert!(app.side_hot);
        app.on_mouse(event(MouseEventKind::Moved, 40, 10));
        assert!(!app.side_hot);
        // Hors écran chat : ni grab ni survol.
        app.screen = Screen::Shop;
        app.on_mouse(click(17, 10));
        assert!(app.resizing_side.is_none());
        assert_eq!(app.side_delta, 0);
        app.on_mouse(event(MouseEventKind::Moved, 17, 10));
        assert!(!app.side_hot);
        // Drag sans grab préalable : ignoré.
        app.screen = Screen::Chat;
        app.on_mouse(event(MouseEventKind::Drag(MouseButton::Left), 30, 10));
        assert_eq!(app.side_delta, 0);
    }

    #[test]
    fn seuls_les_clics_gauches_comptent() {
        let mut app = test_app();
        app.plus_btn = Some(Rect::new(1, 2, 16, 1));
        for kind in [
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Down(MouseButton::Middle),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Moved,
            MouseEventKind::ScrollUp,
        ] {
            app.on_mouse(MouseEvent {
                kind,
                column: 5,
                row: 2,
                modifiers: KeyModifiers::empty(),
            });
        }
        assert_eq!(app.sessions.count(), 1);
    }

    #[test]
    fn molette_defile_le_chat_survole_uniquement() {
        let mut app = test_app();
        // Colonne de discussion mémorisée : x 20..80, y 1..21.
        app.sessions
            .current()
            .chat
            .area
            .set(Rect::new(20, 1, 60, 20));
        let offset = |app: &App| app.sessions.current().chat.scroll.offset();
        app.on_mouse(event(MouseEventKind::ScrollUp, 30, 5));
        assert_eq!(offset(&app), SCROLL_STEP as usize);
        app.on_mouse(event(MouseEventKind::ScrollDown, 30, 5));
        assert_eq!(offset(&app), 0, "retour au bas");
        // Hors de la colonne (sidebar, panneau droit) : ignoré.
        app.on_mouse(event(MouseEventKind::ScrollUp, 2, 5));
        assert_eq!(offset(&app), 0);
        app.on_mouse(event(MouseEventKind::ScrollUp, 95, 5));
        assert_eq!(offset(&app), 0);
        // Autre écran : ignoré même dans la colonne.
        app.screen = Screen::Settings;
        app.on_mouse(event(MouseEventKind::ScrollUp, 30, 5));
        assert_eq!(offset(&app), 0);
    }

    #[test]
    fn clic_ligne_sidebar_choisit_le_chat() {
        let mut app = test_app();
        app.add_chat();
        app.add_chat();
        app.side_rows = Some(Rect::new(1, 2, 16, 3));
        app.on_mouse(click(5, 3)); // 2e ligne → chat 2
        assert_eq!(app.sessions.index(), 1);
        app.on_mouse(click(5, 2)); // 1re ligne → chat 1
        assert_eq!(app.sessions.index(), 0);
        app.on_mouse(click(5, 5)); // hors bloc → inchangé
        assert_eq!(app.sessions.index(), 0);
        app.side_rows = None;
        app.on_mouse(click(5, 3)); // plus de sidebar → inchangé
        assert_eq!(app.sessions.index(), 0);
    }

    #[test]
    fn clic_croix_ferme_le_chat_corps_de_ligne_choisit() {
        let mut app = test_app();
        app.add_chat();
        app.add_chat();
        app.side_rows = Some(Rect::new(1, 2, 16, 3));
        app.close_btns = vec![
            Some(Rect::new(16, 2, 1, 1)),
            Some(Rect::new(16, 3, 1, 1)),
            None, // croix masquée : le clic choisit, comme avant
        ];
        app.on_mouse(click(16, 3)); // [×] du chat 2 → fermé
        assert_eq!(app.sessions.count(), 2);
        app.on_mouse(click(5, 3)); // corps de ligne → choisir
        assert_eq!(app.sessions.index(), 1);
        app.on_mouse(click(16, 4)); // pas de croix ici → inchangé
        assert_eq!(app.sessions.count(), 2);
    }

    #[test]
    fn clic_croix_du_dernier_chat_motive_le_refus() {
        let mut app = test_app();
        app.side_rows = Some(Rect::new(1, 2, 16, 1));
        app.close_btns = vec![Some(Rect::new(16, 2, 1, 1))];
        app.on_mouse(click(16, 2));
        assert_eq!(app.sessions.count(), 1);
        let last = app.sessions.current().chat.messages().last().unwrap();
        assert_eq!(last.text, "cannot close the last chat");
    }

    #[test]
    fn rendu_puis_clic_croix_ferme_le_bon_chat() {
        use ratatui::{backend::TestBackend, Terminal};
        let mut app = test_app();
        app.add_chat();
        let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        // La zone mémorisée porte bien le bouton [×] dessiné.
        let cross = app.close_btns[1].expect("[×] du chat 2 mémorisé");
        let buf = term.backend().buffer();
        assert_eq!(cross.width, 3);
        assert_eq!(buf[(cross.x, cross.y)].symbol(), "[");
        assert_eq!(buf[(cross.x + 1, cross.y)].symbol(), "×");
        assert_eq!(buf[(cross.x + 2, cross.y)].symbol(), "]");
        assert_eq!(buf[(cross.x + 3, cross.y)].symbol(), " ", "marge au bord");
        app.on_mouse(click(cross.x + 1, cross.y));
        assert_eq!(app.sessions.count(), 1);
        assert_eq!(app.sessions.current().name, "Chat 1");
    }

    #[test]
    fn clic_onglet_change_d_ecran_comme_la_touche() {
        let mut app = test_app();
        app.tab_hits = vec![
            (Screen::Chat, Rect::new(10, 0, 7, 1)),
            (Screen::Settings, Rect::new(45, 0, 12, 1)),
        ];
        app.screen = Screen::Settings;
        app.on_mouse(click(11, 0)); // onglet chat → chat
        assert_eq!(app.screen, Screen::Chat);
        app.on_mouse(click(46, 0)); // onglet settings → settings
        assert_eq!(app.screen, Screen::Settings);
        app.on_mouse(click(70, 0)); // hors onglets → inchangé
        assert_eq!(app.screen, Screen::Settings);
    }

    #[test]
    fn clic_onglet_boutique_bascule_comme_f6() {
        let mut app = test_app();
        app.tab_hits = vec![(Screen::Shop, Rect::new(50, 0, 11, 1))];
        app.on_mouse(click(52, 0));
        assert_eq!(app.screen, Screen::Shop);
        app.on_mouse(click(52, 0)); // Déjà dessus : retour au chat.
        assert_eq!(app.screen, Screen::Chat);
    }

    #[test]
    fn clic_article_achete_si_solvable() {
        let mut app = test_app();
        app.screen = Screen::Shop;
        app.shop_rows = vec![Rect::new(0, 2, 70, 2), Rect::new(0, 4, 70, 2)];
        app.on_mouse(click(10, 2)); // 0 pièce : refusé, rien débité.
        assert_eq!(app.shop.wallet().spent(), 0);
        app.sessions.current_mut().ledger.record(500, 500, 1.0); // 1000 gagnées.
        app.on_mouse(click(10, 4)); // Bougie apaisante, 300 pièces.
        assert_eq!(app.shop.wallet().spent(), 300);
        app.on_mouse(click(10, 20)); // Hors articles : inchangé.
        assert_eq!(app.shop.wallet().spent(), 300);
    }

    #[test]
    fn rendu_puis_clic_ligne_onglet_et_plus() {
        use ratatui::{backend::TestBackend, Terminal};
        let mut app = test_app();
        app.add_chat();
        let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        // Lignes mémorisées : clic 2e ligne → chat 2.
        let rows = app.side_rows.expect("lignes mémorisées");
        assert_eq!(rows.height, 2);
        app.on_mouse(click(rows.x, rows.y + 1));
        assert_eq!(app.sessions.index(), 1);
        // Onglets mémorisés : chat + boutique + jeu, clic → écran.
        assert_eq!(app.tab_hits.len(), 3);
        let shop = app
            .tab_hits
            .iter()
            .find(|(s, _)| *s == Screen::Shop)
            .unwrap()
            .1;
        app.on_mouse(click(shop.x, shop.y));
        assert_eq!(app.screen, Screen::Shop);
        let chat = app
            .tab_hits
            .iter()
            .find(|(s, _)| *s == Screen::Chat)
            .unwrap()
            .1;
        app.on_mouse(click(chat.x, chat.y));
        assert_eq!(app.screen, Screen::Chat);
        let haunt = app.tab_hits.iter().find(|(s, _)| *s == Screen::Haunt);
        let haunt = haunt.unwrap().1;
        app.on_mouse(click(haunt.x, haunt.y));
        assert_eq!(app.screen, Screen::Haunt);
        app.on_mouse(click(chat.x, chat.y));
        assert_eq!(app.screen, Screen::Chat);
        // [+] mémorisé : clic → 3e chat.
        let plus = app.plus_btn.expect("plus mémorisé");
        app.on_mouse(click(plus.x, plus.y));
        assert_eq!(app.sessions.count(), 3);
    }
}
