//! Clavier : dispatch général (`on_key`), défilement, colonne ; celles
//! du jeu vivent avec son écran (`app/haunt.rs`, seul à recevoir les
//! relâchements).

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{App, Screen};
use crate::chat::SCROLL_PAGE;

impl App {
    /// Traite une touche ; renvoie false pour quitter.
    /// Confirmation outil, flow `/connect` ou attente `/spec run` :
    /// touches dédiées (pas de quit). Hors jeu, seuls les appuis comptent
    /// (ni répétitions ni relâchements).
    pub(super) fn on_key(&mut self, key: KeyEvent) -> bool {
        let modal =
            self.tool_run.as_ref().is_some_and(|run| run.confirming) || self.connect_flow.is_some();
        if self.screen == Screen::Haunt && !modal {
            return self.on_haunt_key(key);
        }
        if key.kind != KeyEventKind::Press {
            return true;
        }
        if self.tool_run.as_ref().is_some_and(|run| run.confirming) {
            return self.on_tool_key(key);
        }
        if self.connect_flow.is_some() {
            return self.on_flow_key(key);
        }
        if self.spec_waiting.is_some() {
            return self.on_spec_key(key);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let empty = self
            .sessions
            .get(self.sessions.index())
            .map(|s| s.chat.input.is_empty())
            .unwrap_or(true);
        match self.screen {
            Screen::Settings => return self.on_settings_key(key),
            Screen::Vps => return self.on_vps_key(key.code),
            Screen::Shop => return self.on_shop_key(key.code),
            _ => {}
        }
        match key.code {
            KeyCode::Char('q') if !ctrl && empty => false,
            KeyCode::Char('n' | 'N') if ctrl => self.cmd(|s| s.add_chat()),
            KeyCode::Char('w' | 'W') if ctrl => self.cmd(|s| s.close_chat(s.sessions.index())),
            KeyCode::Char('c' | 'C') if ctrl => self.cmd(|s| s.stop_chat()),
            KeyCode::Char('v' | 'V') if ctrl => self.cmd(|s| s.paste_clip()),
            KeyCode::Char(c) if !ctrl => self.cmd(|s| s.type_char(c)),
            KeyCode::Backspace => self.cmd(|s| s.backspace()),
            KeyCode::Enter => self.cmd(|s| s.enter()),
            KeyCode::PageUp => self.cmd(|s| s.scroll_chat(-SCROLL_PAGE)),
            KeyCode::PageDown => self.cmd(|s| s.scroll_chat(SCROLL_PAGE)),
            KeyCode::Up if shift => self.cmd(|s| s.scroll_chat(-1)),
            KeyCode::Down if shift => self.cmd(|s| s.scroll_chat(1)),
            KeyCode::Up => self.cmd(|s| s.step_agent(-1)),
            KeyCode::Down => self.cmd(|s| s.step_agent(1)),
            KeyCode::Esc => self.cmd(|s| s.clear_agent_view()),
            KeyCode::End => self.cmd(|s| s.scroll_bottom()),
            KeyCode::F(5) => self.cmd(|s| s.spook()),
            KeyCode::F(6) => self.cmd(|s| s.open_shop()),
            KeyCode::F(9) => self.cmd(|s| s.open_haunt()),
            KeyCode::Left if ctrl => self.cmd(|s| s.resize_right(2)),
            KeyCode::Right if ctrl => self.cmd(|s| s.resize_right(-2)),
            KeyCode::Left => self.cmd(|s| s.move_cursor(-1)),
            KeyCode::Right => self.cmd(|s| s.move_cursor(1)),
            _ => true,
        }
    }

    /// Frappe dans la saisie (ignorée sans chat : [+] / Ctrl+N d'abord).
    fn type_char(&mut self, c: char) {
        let idx = self.sessions.index();
        if let Some(sess) = self.sessions.get_mut(idx) {
            sess.chat.input_push(c);
        }
    }

    /// Efface un caractère (ignoré sans chat).
    fn backspace(&mut self) {
        let idx = self.sessions.index();
        if let Some(sess) = self.sessions.get_mut(idx) {
            sess.chat.input_pop();
        }
    }

    /// ←/→ : déplace le curseur dans la barre (ignoré sans chat).
    fn move_cursor(&mut self, delta: isize) {
        let idx = self.sessions.index();
        if let Some(sess) = self.sessions.get_mut(idx) {
            sess.chat.move_cursor(delta);
        }
    }

    /// Saute en bas de l'historique (ignoré sans chat).
    fn scroll_bottom(&mut self) {
        let idx = self.sessions.index();
        if let Some(sess) = self.sessions.get(idx) {
            sess.chat.scroll.to_bottom();
        }
    }

    /// Entrée : déplie l'agent pointé (saisie vide + agents), sinon envoi.
    fn enter(&mut self) {
        let idx = self.sessions.index();
        let empty = self
            .sessions
            .get(idx)
            .map(|s| s.chat.input.is_empty())
            .unwrap_or(true);
        if empty {
            if let Some(sess) = self.sessions.get_mut(idx) {
                if sess.toggle_cursor_or_first() {
                    return;
                }
            }
        }
        self.send();
    }

    /// ↑↓ : déplace le curseur agent (ignoré sans chat / sans agent).
    fn step_agent(&mut self, dir: i8) {
        let idx = self.sessions.index();
        if let Some(sess) = self.sessions.get_mut(idx) {
            sess.step_agent_cursor(dir);
        }
    }

    /// Échap : curseur effacé + échanges repliés (ignoré sans chat).
    fn clear_agent_view(&mut self) {
        let idx = self.sessions.index();
        if let Some(sess) = self.sessions.get_mut(idx) {
            sess.clear_agent_view();
        }
    }

    /// Défile l'historique du chat courant (`delta` < 0 = remonter).
    pub(crate) fn scroll_chat(&mut self, delta: isize) {
        let idx = self.sessions.index();
        if let Some(sess) = self.sessions.get(idx) {
            sess.chat.scroll.by(delta);
        }
    }

    /// Ctrl+←/→ : élargit (+2) / rétrécit (−2) la colonne de droite.
    pub(crate) fn resize_right(&mut self, delta: i16) {
        self.right_delta = (self.right_delta + delta).clamp(-64, 64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Completion, ProviderConfig};

    /// App de test (clé factice, aucun appel réseau).
    fn test_app() -> App {
        App::from_config(ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        })
        .unwrap()
    }

    #[test]
    fn ctrl_v_colle_dans_la_barre_de_saisie() {
        let mut app = test_app();
        app.clip_last = String::from("collé");
        let attendu = crate::clip::resolve_paste("collé");
        let ctrl_v = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert!(app.on_key(ctrl_v));
        assert_eq!(app.sessions.current().chat.input, attendu);
    }

    #[test]
    fn ctrl_c_stoppe_le_vol_sans_quitter() {
        let mut app = test_app();
        app.sessions.get_mut(0).unwrap().sending = true;
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(app.on_key(ctrl_c)); // ne quitte pas
        assert!(!app.sessions.get(0).unwrap().sending, "vol stoppé");
    }

    #[test]
    fn ctrl_c_sans_vol_muet_puis_stop_puis_renvoi() {
        let mut app = test_app();
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(app.on_key(ctrl_c)); // rien en vol : muet
        assert!(app.sessions.current().chat.messages().is_empty());
        // Vol 1 (époque 0) stoppé : réponse tardive ignorée.
        app.sessions.get_mut(0).unwrap().sending = true;
        assert!(app.on_key(ctrl_c));
        let tardive = Completion {
            text: String::from("trop tard"),
            tool_calls: Vec::new(),
            prompt_tokens: 1,
            completion_tokens: 1,
        };
        app.drain_chat(0, 0, Ok(tardive));
        let sess = app.sessions.get(0).unwrap();
        assert!(!sess.sending, "tardive ne relance rien");
        assert_eq!(sess.ledger.total(), 0, "tardive non comptée");
        assert!(
            !sess.history.iter().any(|(_, t)| t == "trop tard"),
            "tardive non versée"
        );
        // Vol 2 (époque 1) : réponse acceptée normalement.
        app.sessions.get_mut(0).unwrap().sending = true;
        let fraiche = Completion {
            text: String::from("à temps"),
            tool_calls: Vec::new(),
            prompt_tokens: 1,
            completion_tokens: 1,
        };
        app.drain_chat(0, 1, Ok(fraiche));
        let sess = app.sessions.get(0).unwrap();
        assert!(!sess.sending);
        assert_eq!(sess.ledger.total(), 2);
        assert_eq!(sess.history.last().unwrap().1, "à temps");
    }

    #[test]
    fn fleches_naviguent_entree_deplie_echap_replie() {
        let mut app = test_app();
        let key = |c| KeyEvent::new(c, KeyModifiers::empty());
        // Sans agent : sans effet.
        assert!(app.on_key(key(KeyCode::Down)));
        assert!(app.on_key(key(KeyCode::Enter)));
        assert!(app.sessions.get(0).unwrap().agent_cursor.is_none());
        let history = vec![(String::from("user"), String::from("u"))];
        app.sessions.add_agent(0, "un", history.clone()).unwrap();
        app.sessions.add_agent(0, "deux", history).unwrap();
        let cursor = |app: &App| app.sessions.get(0).unwrap().agent_cursor;
        assert!(app.on_key(key(KeyCode::Down)));
        assert_eq!(cursor(&app), Some(1));
        assert!(app.on_key(key(KeyCode::Down)));
        assert_eq!(cursor(&app), Some(2));
        assert!(app.on_key(key(KeyCode::Down)));
        assert_eq!(cursor(&app), Some(1), "cyclique");
        assert!(app.on_key(key(KeyCode::Up)));
        assert_eq!(cursor(&app), Some(2));
        assert!(app.on_key(key(KeyCode::Enter)));
        assert_eq!(app.sessions.get(0).unwrap().expanded_agents, vec![2]);
        assert!(app.on_key(key(KeyCode::Enter)));
        assert!(app.sessions.get(0).unwrap().expanded_agents.is_empty());
        assert!(app.on_key(key(KeyCode::Esc)));
        assert!(cursor(&app).is_none());
        // Saisie non vide : Entrée envoie (commande), pas de déplié.
        app.sessions.get_mut(0).unwrap().chat.input_insert("/usage");
        assert!(app.on_key(key(KeyCode::Enter)));
        assert!(app.sessions.get(0).unwrap().expanded_agents.is_empty());
        assert!(app.sessions.get(0).unwrap().chat.input.is_empty());
    }

    #[test]
    fn sans_chat_frappe_et_entree_ignores_q_quitte_ctrl_n_recree() {
        let mut app = test_app();
        app.close_chat(0);
        assert_eq!(app.sessions.count(), 0);
        let key = |c| KeyEvent::new(c, KeyModifiers::empty());
        assert!(app.on_key(key(KeyCode::Char('x'))));
        assert!(app.on_key(key(KeyCode::Enter)));
        assert!(app.on_key(key(KeyCode::Backspace)));
        assert!(app.on_key(key(KeyCode::PageUp)));
        assert!(app.on_key(key(KeyCode::End)));
        let ctrl = |c| KeyEvent::new(c, KeyModifiers::CONTROL);
        assert!(app.on_key(ctrl(KeyCode::Char('w')))); // rien à fermer
        assert_eq!(app.sessions.count(), 0);
        assert!(!app.on_key(key(KeyCode::Char('q')))); // quitte
        assert!(app.on_key(ctrl(KeyCode::Char('n')))); // recrée
        assert_eq!(app.sessions.count(), 1);
        assert_eq!(app.sessions.current().name, "Chat 1");
    }

    #[test]
    fn fleches_gauche_droite_deplacent_le_curseur_borne() {
        let mut app = test_app();
        for c in "abc".chars() {
            app.sessions.current_mut().chat.input_push(c);
        }
        let key = |c| KeyEvent::new(c, KeyModifiers::empty());
        let at = |app: &App| app.sessions.current().chat.cursor;
        assert_eq!(at(&app), 3);
        assert!(app.on_key(key(KeyCode::Left)));
        assert_eq!(at(&app), 2);
        assert!(app.on_key(key(KeyCode::Right)));
        assert_eq!(at(&app), 3);
        assert!(app.on_key(key(KeyCode::Right)), "fin : borné");
        assert_eq!(at(&app), 3);
        for _ in 0..9 {
            assert!(app.on_key(key(KeyCode::Left)));
        }
        assert_eq!(at(&app), 0, "début : borné");
    }
}
