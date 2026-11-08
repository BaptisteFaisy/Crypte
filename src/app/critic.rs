//! Kiro critique : chaque prompt envoyé part aussi, en parallèle et avec
//! le modèle effectif du chat, vers une consigne de critique courte et
//! désobligeante ; la réponse s'affiche dans la bulle de Kiro en balade
//! (voir [`crate::phantom`]). Tokens comptés dans l'almanach (`/usage`).
//! `/critic [on|off]` ; clic sur Kiro = le frapper, sur sa bulle = la
//! faire taire.

use anyhow::Result;
use ratatui::layout::Position;

use super::{App, Incoming, Screen};
use crate::phantom::{clean_critique, critique_messages, Hit};
use crate::provider::Completion;

impl App {
    /// Envoi réel du chat `id` : Kiro juge `prompt` (une critique à la
    /// fois ; rien si `/critic off` ou déconnecté).
    pub(super) fn critique(&mut self, id: usize, prompt: &str) {
        if !self.phantom.wants_critique() {
            return;
        }
        let Some(provider) = self.provider.clone() else {
            return;
        };
        let model = self.effective_model_for(id);
        let messages = critique_messages(prompt);
        let tx = self.tx.clone();
        self.phantom.begin_critique();
        self.pool.spawn(async move {
            let reply = provider.complete_full(&messages, &model, None).await;
            let _ = tx.send(Incoming::Critique(reply)).await;
        });
    }

    /// Critique arrivée : bulle de Kiro (erreur = il ronchonne, sans
    /// détail ; l'almanach a déjà compté les tokens).
    pub(super) fn drain_critique(&mut self, reply: Result<Completion>) {
        match reply {
            Ok(done) => {
                let tokens = done.prompt_tokens.saturating_add(done.completion_tokens);
                self.phantom
                    .critique_done(clean_critique(&done.text), tokens);
            }
            Err(_) => self.phantom.critique_failed(),
        }
    }

    /// `/critic [on|off]` : statut, ou Kiro sort / rentre.
    pub(crate) fn cmd_critic(&mut self, arg: Option<String>) {
        match arg.as_deref() {
            None => {}
            Some("on") => self.phantom.set_on(true),
            Some("off") => self.phantom.set_on(false),
            Some(val) => {
                self.push_system(format!("invalid: {val} — values: on, off"));
                return;
            }
        }
        self.push_system(self.critic_line());
    }

    /// Ligne de statut `/critic` : état, modèle, critiques et tokens.
    fn critic_line(&self) -> String {
        if !self.phantom.on() {
            return String::from("critic: off (Kiro is resting, no calls) — /critic on");
        }
        let (count, tokens) = self.phantom.stats();
        format!(
            "critic: on — Kiro roams and judges each prompt with {} \
             ({count} critiques, {tokens} tokens) · click him to hit him",
            self.effective_model_ref()
        )
    }

    /// Clic gauche en `at` sur l'écran chat : Kiro frappé (+ éclair) ou
    /// bulle tue. Vrai si le clic est consommé.
    pub(crate) fn click_phantom(&mut self, at: Position) -> bool {
        if self.screen != Screen::Chat {
            return false;
        }
        match self.phantom.hit_test(at) {
            Hit::Body => {
                self.phantom.smack(at);
                self.fx.flash();
                true
            }
            Hit::Bubble => {
                self.phantom.hush();
                true
            }
            Hit::Miss => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phantom::{Mood, Tone};
    use crate::provider::ProviderConfig;
    use ratatui::{backend::TestBackend, Terminal};

    /// App de test (clé factice ; seul le test tokio ouvre un socket,
    /// vers un port fermé).
    fn test_app() -> App {
        App::from_config(ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice-model"),
        })
        .unwrap()
    }

    /// Dernier message du chat courant.
    fn last(app: &App) -> String {
        let chat = &app.sessions.current().chat;
        chat.messages()
            .last()
            .map(|m| m.text.clone())
            .unwrap_or_default()
    }

    /// Une frame 100×30 (zones cliquables à jour).
    fn draw(app: &mut App) {
        let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
    }

    fn reply(text: &str) -> Completion {
        Completion {
            text: String::from(text),
            tool_calls: Vec::new(),
            prompt_tokens: 40,
            completion_tokens: 12,
        }
    }

    #[test]
    fn critic_command_shows_toggles_and_rejects() {
        let mut app = test_app();
        app.run_command("/critic");
        assert!(last(&app).starts_with("critic: on"), "{}", last(&app));
        assert!(last(&app).contains("factice-model"));
        app.run_command("/critic off");
        assert!(last(&app).starts_with("critic: off"));
        assert!(!app.phantom.on());
        app.run_command("/critic maybe");
        assert_eq!(last(&app), "invalid: maybe — values: on, off");
        app.run_command("/critic on");
        assert!(app.phantom.on());
    }

    #[test]
    fn critique_lands_in_kiro_bubble_and_in_the_almanac() {
        let mut app = test_app();
        app.phantom.begin_critique();
        app.tx
            .try_send(Incoming::Critique(Ok(reply(
                "\"Bold of you to call that a prompt.\"",
            ))))
            .unwrap();
        app.drain_replies();
        let speech = app.phantom.speech().expect("bulle");
        assert_eq!(speech.text, "Bold of you to call that a prompt.");
        assert_eq!(speech.tone, Tone::Critique);
        assert_eq!(app.phantom.stats(), (1, 52));
        let tonight = app.almanac.day(crate::almanac::today());
        assert_eq!((tonight.total(), tonight.calls), (52, 1));
        // Le chat n'est pas touché : la critique vit dans la bulle.
        assert!(app
            .sessions
            .current()
            .history
            .iter()
            .all(|(r, _)| r != "assistant"));
        app.phantom.begin_critique();
        app.drain_critique(Err(anyhow::anyhow!("boom: secret detail")));
        assert!(!app.phantom.speech().unwrap().text.contains("boom"));
    }

    #[test]
    fn clicking_kiro_hits_him_and_the_bubble_hushes() {
        let mut app = test_app();
        draw(&mut app);
        let body = app.phantom_body();
        let at = Position::new(body.x + 1, body.y + 1);
        app.on_mouse(click(at));
        assert!(matches!(app.phantom.mood(), Mood::Hurt(_)));
        assert_eq!(app.sessions.count(), 1, "le clic ne passe pas dessous");
        // Sa bulle (réplique) : un clic la fait taire.
        draw(&mut app);
        let bubble = app.phantom_bubble();
        app.on_mouse(click(bubble));
        assert!(app.phantom.speech().is_none());
        // Autre écran : Kiro absent, clic ignoré.
        app.screen = Screen::Shop;
        assert!(!app.click_phantom(at));
    }

    #[tokio::test]
    async fn sending_a_prompt_also_asks_kiro_once() {
        let mut app = test_app();
        app.tools_on = false;
        for c in "fix my code".chars() {
            app.sessions.current_mut().chat.input_push(c);
        }
        app.send();
        assert!(!app.phantom.wants_critique(), "critique en vol");
        app.run_command("/critic");
        assert!(last(&app).starts_with("critic: on"));
    }

    /// Clic gauche en `at`.
    fn click(at: Position) -> crossterm::event::MouseEvent {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: at.x,
            row: at.y,
            modifiers: KeyModifiers::empty(),
        }
    }

    impl App {
        /// Sprite de Kiro de la dernière frame (tests).
        fn phantom_body(&self) -> ratatui::layout::Rect {
            let body = (0..30u16)
                .flat_map(|y| (0..100u16).map(move |x| Position::new(x, y)))
                .find(|p| self.phantom.hit_test(*p) == Hit::Body);
            let first = body.expect("Kiro dessiné");
            ratatui::layout::Rect::new(first.x, first.y, 2, 2)
        }

        /// Un point de la bulle de la dernière frame (tests).
        fn phantom_bubble(&self) -> Position {
            (0..30u16)
                .flat_map(|y| (0..100u16).map(move |x| Position::new(x, y)))
                .find(|p| self.phantom.hit_test(*p) == Hit::Bubble)
                .expect("bulle dessinée")
        }
    }
}
