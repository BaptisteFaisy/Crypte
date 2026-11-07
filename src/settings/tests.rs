//! Screen-level tests of `Settings`: rows, vault round-trip, masked input.

use super::*;

/// Directory of the throwaway store behind `settings`.
fn dir_of(settings: &Settings) -> std::path::PathBuf {
    settings
        .store
        .data_path()
        .parent()
        .expect("store dir")
        .to_path_buf()
}

/// Settings backed by a throwaway store.
fn settings(name: &str) -> Settings {
    let dir = std::env::temp_dir().join(format!("kh-settings-{}-{name}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    Settings::with_store(Store::new(dir))
}

fn type_key(settings: &mut Settings, key: &str) {
    assert!(settings.begin_edit(Field::Key));
    for c in key.chars() {
        settings.push_char(c);
    }
    assert!(settings.masked());
    assert_eq!(settings.edit_preview(), "•".repeat(key.chars().count()));
    settings.commit_edit();
}

#[test]
fn rows_list_every_known_provider_without_keys() {
    let settings = settings("rows");
    assert_eq!(settings.rows().len(), KNOWN.len());
    assert_eq!(settings.rows()[0].name, "openai");
    assert!(settings.rows().iter().all(|r| !r.has_key));
    assert_eq!(settings.keyed_count(), 0);
    assert!(settings.last_config().is_none());
}

#[test]
fn key_is_encrypted_at_rest_and_reloaded() {
    let mut settings = settings("save");
    assert!(settings.select("fireworks"));
    type_key(&mut settings, "fw-test-key");
    assert!(!settings.masked());
    assert!(settings.selected_row().unwrap().has_key);
    assert!(settings.notice.as_deref().unwrap().contains("key saved"));
    let raw = std::fs::read(settings.store.data_path()).unwrap();
    assert!(!String::from_utf8_lossy(&raw).contains("fw-test-key"));
    let reloaded = Settings::with_store(Store::new(dir_of(&settings)));
    let cfg = reloaded.config_for("fireworks").unwrap();
    assert_eq!(cfg.api_key, "fw-test-key");
    assert_eq!(cfg.model, "accounts/fireworks/models/deepseek-v3");
    std::fs::remove_dir_all(dir_of(&settings)).ok();
}

#[test]
fn empty_key_keeps_the_input_open() {
    let mut settings = settings("empty");
    assert!(settings.begin_edit(Field::Key));
    settings.commit_edit();
    assert!(settings.edit.is_some());
    settings.cancel_edit();
    assert!(settings.edit.is_none());
    assert!(!settings.selected_row().unwrap().has_key);
}

#[test]
fn model_and_base_can_be_edited_without_key() {
    let mut settings = settings("edit");
    assert!(settings.select("openai"));
    assert!(settings.begin_edit(Field::Model));
    for _ in 0..20 {
        settings.pop_char();
    }
    for c in "gpt-4.1".chars() {
        settings.push_char(c);
    }
    settings.commit_edit();
    assert!(settings.begin_edit(Field::Base));
    settings.pop_char();
    settings.push_char('x');
    settings.commit_edit();
    let row = settings.selected_row().unwrap();
    assert_eq!(row.model, "gpt-4.1");
    assert!(!row.has_key);
    assert!(row.base_url.ends_with('x'));
    std::fs::remove_dir_all(dir_of(&settings)).ok();
}

#[test]
fn delete_erases_the_key_but_keeps_the_row() {
    let mut settings = settings("delete");
    assert!(settings.select("openai"));
    type_key(&mut settings, "sk-x");
    settings.delete_key();
    assert!(!settings.selected_row().unwrap().has_key);
    assert!(settings.config_for("openai").is_none());
    settings.delete_key();
    assert!(settings.notice.as_deref().unwrap().contains("no key"));
    std::fs::remove_dir_all(dir_of(&settings)).ok();
}

#[test]
fn custom_provider_added_then_keyed() {
    let mut settings = settings("custom");
    settings.add_custom("ds  http://127.0.0.1:18100/v1  deepseek-v4.1-flash");
    assert_eq!(settings.selected_row().unwrap().name, "ds");
    assert!(!settings.selected_row().unwrap().known);
    type_key(&mut settings, "local-key");
    let cfg = settings.config_for("ds").unwrap();
    assert_eq!(cfg.base_url, "http://127.0.0.1:18100/v1");
    assert_eq!(cfg.model, "deepseek-v4.1-flash");
    settings.add_custom("openai https://x/v1");
    assert!(settings
        .notice
        .as_deref()
        .unwrap()
        .contains("known provider"));
    settings.add_custom("nope");
    assert!(settings
        .notice
        .as_deref()
        .unwrap()
        .contains("usage: /settings add"));
    std::fs::remove_dir_all(dir_of(&settings)).ok();
}

#[test]
fn last_provider_is_reused_at_startup() {
    let mut settings = settings("last");
    assert!(settings.select("openai"));
    type_key(&mut settings, "sk-a");
    assert!(settings.select("groq"));
    type_key(&mut settings, "gsk-b");
    settings.mark_connected("groq");
    let cfg = settings.last_config().unwrap();
    assert_eq!(cfg.api_key, "gsk-b");
    assert_eq!(cfg.base_url, "https://api.groq.com/openai/v1");
    std::fs::remove_dir_all(dir_of(&settings)).ok();
}

#[test]
fn cursor_moves_within_bounds() {
    let mut settings = settings("cursor");
    settings.step(-1);
    assert_eq!(settings.selected, 0);
    settings.step(2);
    assert_eq!(settings.selected, 2);
    for _ in 0..100 {
        settings.step(1);
    }
    assert_eq!(settings.selected, KNOWN.len() - 1);
}

#[test]
fn known_defaults_and_labels() {
    assert_eq!(known_defaults("openai").unwrap().1, "gpt-4o");
    assert!(known_defaults("zzz").is_none());
    assert!(Field::Key.masked());
    assert!(!Field::Model.masked());
    assert_eq!(Field::Base.label(), "base URL");
}
