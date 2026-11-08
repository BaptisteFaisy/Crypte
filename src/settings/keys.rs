//! `/settings` glue: opening the screen, key handling, connecting with a
//! stored key. `impl App` lives here to keep `app.rs` under 500 lines.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::Field;
use crate::app::{App, Screen};
use crate::provider::ProviderConfig;

impl App {
    /// `/settings [name | add <name> <base_url> [model]]` : opens the screen.
    pub(crate) fn open_settings(&mut self, arg: String) {
        let arg = arg.trim().to_string();
        self.settings.reload();
        self.screen = Screen::Settings;
        if arg.is_empty() {
            return;
        }
        if arg == "add" || arg.starts_with("add ") {
            self.settings.add_custom(arg.trim_start_matches("add"));
            return;
        }
        if self.settings.select(&arg) && self.settings.begin_edit(Field::Key) {
            return;
        }
        self.settings.notice = Some(format!(
            "unknown provider: {arg} — /settings add <name> <base_url> [model]"
        ));
    }

    /// Keys on the settings screen (input mode first).
    pub(crate) fn on_settings_key(&mut self, key: KeyEvent) -> bool {
        if self.settings.edit.is_some() {
            return self.on_settings_input_key(key);
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.close_settings(),
            KeyCode::Up | KeyCode::Char('k') => self.cmd(|s| s.settings.step(-1)),
            KeyCode::Down | KeyCode::Char('j') => self.cmd(|s| s.settings.step(1)),
            KeyCode::Enter => self.cmd(|s| {
                s.settings.begin_edit(Field::Key);
            }),
            KeyCode::Char('m') => self.cmd(|s| {
                s.settings.begin_edit(Field::Model);
            }),
            KeyCode::Char('b') => self.cmd(|s| {
                s.settings.begin_edit(Field::Base);
            }),
            KeyCode::Char('d') => self.cmd(|s| s.settings.delete_key()),
            KeyCode::Char('c') => self.cmd(|s| s.connect_selected()),
            _ => true,
        }
    }

    /// Keys while an input is open: text goes to the buffer, Enter saves.
    fn on_settings_input_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return true;
        }
        match key.code {
            KeyCode::Esc => self.cmd(|s| s.settings.cancel_edit()),
            KeyCode::Enter => self.cmd(|s| s.settings.commit_edit()),
            KeyCode::Backspace => self.cmd(|s| s.settings.pop_char()),
            KeyCode::Char(c) => self.cmd(|s| s.settings.push_char(c)),
            _ => true,
        }
    }

    /// Closes the screen, handing the last notice over to the chat.
    fn close_settings(&mut self) -> bool {
        if let Some(notice) = self.settings.notice.take() {
            self.push_system(notice);
        }
        self.screen = Screen::Chat;
        true
    }

    /// Connects the selected provider if its key is stored.
    fn connect_selected(&mut self) {
        let Some(row) = self.settings.selected_row().cloned() else {
            return;
        };
        match self.settings.config_for(&row.name) {
            Some(cfg) => {
                self.connect_saved(cfg, &row.name);
                self.screen = Screen::Chat;
            }
            None => {
                self.settings.notice =
                    Some(format!("{}: no key stored (Enter to add one)", row.name));
            }
        }
    }

    /// Installs a provider resolved from the vault and remembers it.
    pub(crate) fn connect_saved(&mut self, cfg: ProviderConfig, name: &str) {
        self.apply_connected(cfg, name);
        if self.provider.is_some() {
            self.settings.mark_connected(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Store;
    use std::path::PathBuf;

    /// App wired to a throwaway vault.
    fn app_with_vault(name: &str) -> (App, PathBuf) {
        let dir = std::env::temp_dir().join(format!("kh-keys-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let mut app = App::disconnected();
        app.settings = crate::settings::Settings::with_store(Store::new(dir.clone()));
        (app, dir)
    }

    fn press(app: &mut App, code: KeyCode) {
        let key = KeyEvent::new(code, KeyModifiers::NONE);
        assert!(app.on_settings_key(key));
    }

    #[test]
    fn enter_opens_the_key_input_and_esc_closes_the_screen() {
        let (mut app, dir) = app_with_vault("open");
        app.screen = Screen::Settings;
        press(&mut app, KeyCode::Down);
        assert_eq!(app.settings.selected, 1);
        press(&mut app, KeyCode::Enter);
        assert!(app.settings.edit.is_some());
        press(&mut app, KeyCode::Esc);
        assert!(app.settings.edit.is_none());
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.screen, Screen::Chat);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn typing_a_key_saves_it_without_echo_in_the_chat() {
        let (mut app, dir) = app_with_vault("type");
        app.screen = Screen::Settings;
        press(&mut app, KeyCode::Enter);
        for c in "sk-typed".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert!(app.settings.config_for("openai").is_some());
        let chat: String = app
            .sessions
            .current()
            .chat
            .messages()
            .iter()
            .map(|m| m.text.clone())
            .collect();
        assert!(!chat.contains("sk-typed"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn settings_add_and_unknown_provider_notice() {
        let (mut app, dir) = app_with_vault("add");
        app.open_settings(String::from("add ds http://127.0.0.1:9/v1 local-model"));
        assert_eq!(app.screen, Screen::Settings);
        assert_eq!(app.settings.selected_row().unwrap().name, "ds");
        app.open_settings(String::from("zzz"));
        assert!(app
            .settings
            .notice
            .as_deref()
            .unwrap()
            .contains("unknown provider"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn connect_selected_uses_the_stored_key() {
        let (mut app, dir) = app_with_vault("connect");
        app.screen = Screen::Settings;
        press(&mut app, KeyCode::Enter);
        for c in "sk-local".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('c'));
        assert_eq!(app.screen, Screen::Chat);
        let cfg = app.provider.as_ref().unwrap().config();
        assert_eq!(cfg.api_key, "sk-local");
        assert_eq!(cfg.model, "gpt-4o");
        std::fs::remove_dir_all(dir).ok();
    }
}
