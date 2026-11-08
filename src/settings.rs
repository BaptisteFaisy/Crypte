//! Provider settings (`/settings`): every known OpenAI-compatible provider,
//! the API keys kept in the encrypted vault (`store`) and the screen (`ui`).
//!
//! The screen only ever shows whether a key is stored (`✓`), never the key
//! itself: the vault is decrypted in memory, and the UI renders bullets.

mod keys;
mod known;
pub(crate) mod store;
#[cfg(test)]
mod tests;
mod ui;

pub use self::known::{known_defaults, KNOWN};
pub use self::store::{Credential, Store, Vault};

use crate::provider::ProviderConfig;

/// Field edited on the settings screen (key = masked input).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Key,
    Model,
    Base,
}

impl Field {
    /// Short label shown in the input line.
    pub fn label(self) -> &'static str {
        match self {
            Field::Key => "API key",
            Field::Model => "model",
            Field::Base => "base URL",
        }
    }

    /// Only the API key is masked while typing.
    pub fn masked(self) -> bool {
        self == Field::Key
    }
}

/// Input in progress on the settings screen.
pub struct Edit {
    pub field: Field,
    pub buf: String,
}

/// One screen line: a provider and whether its key is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub has_key: bool,
    pub known: bool,
}

/// Settings screen state + its encrypted vault.
pub struct Settings {
    store: Store,
    vault: Vault,
    rows: Vec<Row>,
    pub selected: usize,
    pub edit: Option<Edit>,
    pub notice: Option<String>,
}

impl Settings {
    /// Loads the default store (`$XDG_DATA_HOME`); never panics on I/O.
    pub fn load() -> Self {
        match Store::default_store() {
            Ok(store) => Self::with_store(store),
            Err(err) => Self {
                store: Store::new(std::env::temp_dir().join("kiro-horror-tui")),
                vault: Vault::new(),
                rows: Vec::new(),
                selected: 0,
                edit: None,
                notice: Some(format!("settings store unavailable: {err}")),
            },
        }
    }

    /// Loads the vault of `store` (missing file → every row without a key).
    pub fn with_store(store: Store) -> Self {
        let mut settings = Self {
            store,
            vault: Vault::new(),
            rows: Vec::new(),
            selected: 0,
            edit: None,
            notice: None,
        };
        settings.reload();
        settings
    }

    /// Re-reads the vault from disk (other terminals may have written it).
    pub fn reload(&mut self) {
        match self.store.load() {
            Ok(vault) => self.vault = vault,
            Err(err) => self.notice = Some(format!("vault unreadable: {err:#}")),
        }
        self.rebuild();
    }

    /// Rows in display order: known providers, then custom ones by name.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Row under the cursor.
    pub fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.selected)
    }

    /// Moves the cursor `dy` rows (clamped, no wrap-around).
    pub fn step(&mut self, dy: isize) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        let next = self.selected as isize + dy;
        self.selected = next.clamp(0, last as isize) as usize;
    }

    /// Puts the cursor on `name`; false if unknown.
    pub fn select(&mut self, name: &str) -> bool {
        match self.rows.iter().position(|r| r.name == name) {
            Some(idx) => {
                self.selected = idx;
                true
            }
            None => false,
        }
    }

    /// Starts editing `field` on the selected row (key starts empty).
    pub fn begin_edit(&mut self, field: Field) -> bool {
        let Some(row) = self.selected_row() else {
            return false;
        };
        let buf = match field {
            Field::Key => String::new(),
            Field::Model => row.model.clone(),
            Field::Base => row.base_url.clone(),
        };
        self.edit = Some(Edit { field, buf });
        true
    }

    /// True when the current input must be rendered as bullets.
    pub fn masked(&self) -> bool {
        self.edit.as_ref().is_some_and(|e| e.field.masked())
    }

    /// Text of the input line (bullets when masked).
    pub fn edit_preview(&self) -> String {
        let Some(edit) = self.edit.as_ref() else {
            return String::new();
        };
        if self.masked() {
            "•".repeat(edit.buf.chars().count())
        } else {
            edit.buf.clone()
        }
    }

    /// Appends a printable character to the input.
    pub fn push_char(&mut self, c: char) {
        if let Some(edit) = self.edit.as_mut() {
            if !c.is_control() {
                edit.buf.push(c);
            }
        }
    }

    /// Removes the last character of the input.
    pub fn pop_char(&mut self) {
        if let Some(edit) = self.edit.as_mut() {
            edit.buf.pop();
        }
    }

    /// Drops the input (nothing saved).
    pub fn cancel_edit(&mut self) {
        if self.edit.take().is_some() {
            self.notice = Some(String::from("edit cancelled"));
        }
    }

    /// Saves the input into the vault (empty key/model keeps the input open).
    pub fn commit_edit(&mut self) {
        let Some(edit) = self.edit.take() else {
            return;
        };
        let Some(row) = self.selected_row().cloned() else {
            return;
        };
        let value = edit.buf.trim().to_string();
        if value.is_empty() && edit.field == Field::Key {
            self.notice = Some(String::from("empty key: type it or Esc to cancel"));
            self.edit = Some(edit);
            return;
        }
        let mut cred = self
            .vault
            .providers
            .get(&row.name)
            .cloned()
            .unwrap_or_else(|| Credential {
                base_url: row.base_url.clone(),
                model: row.model.clone(),
                key: String::new(),
            });
        let msg = match edit.field {
            Field::Key => {
                cred.key = value;
                format!("key saved for {} (encrypted)", row.name)
            }
            Field::Model => {
                cred.model = value.clone();
                format!("model for {}: {value}", row.name)
            }
            Field::Base => {
                cred.base_url = value.trim_end_matches('/').to_string();
                format!("base URL for {}: {value}", row.name)
            }
        };
        self.vault.providers.insert(row.name.clone(), cred);
        self.rebuild();
        self.notice = Some(self.with_save(msg));
    }

    /// Erases the stored key of the selected provider (endpoint kept).
    pub fn delete_key(&mut self) {
        let Some(row) = self.selected_row().cloned() else {
            return;
        };
        let stored = self
            .vault
            .providers
            .get_mut(&row.name)
            .is_some_and(|cred| !std::mem::take(&mut cred.key).is_empty());
        if !stored {
            self.notice = Some(format!("{}: no key stored", row.name));
            return;
        }
        self.rebuild();
        let msg = format!("key erased for {}", row.name);
        self.notice = Some(self.with_save(msg));
    }

    /// Adds (or updates) a custom provider: `<name> <base_url> [model]`.
    pub fn add_custom(&mut self, rest: &str) {
        let mut parts = rest.split_whitespace();
        let (Some(name), Some(base)) = (parts.next(), parts.next()) else {
            self.notice = Some(String::from(
                "usage: /settings add <name> <base_url> [model]",
            ));
            return;
        };
        if KNOWN.iter().any(|k| k.name == name) {
            self.notice = Some(format!("{name} is a known provider (Enter to set its key)"));
            return;
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            self.notice = Some(String::from("name: letters, digits, '-' or '_' only"));
            return;
        }
        let model = parts.next().unwrap_or("").to_string();
        let cred = self.vault.providers.entry(name.to_string()).or_default();
        cred.base_url = base.trim_end_matches('/').to_string();
        cred.model = model;
        self.rebuild();
        self.select(name);
        let msg = format!("{name} added: press Enter to store its key");
        self.notice = Some(self.with_save(msg));
    }

    /// Provider config for `name`, or `None` without a stored key.
    pub fn config_for(&self, name: &str) -> Option<ProviderConfig> {
        let cred = self.vault.providers.get(name)?;
        if cred.key.is_empty() {
            return None;
        }
        let (known_base, known_model) = known_defaults(name).unwrap_or(("", ""));
        let base_url = choose(&cred.base_url, known_base);
        let model = choose(&cred.model, known_model);
        if base_url.is_empty() || model.is_empty() {
            return None;
        }
        Some(ProviderConfig {
            base_url,
            api_key: cred.key.clone(),
            model,
        })
    }

    /// Config of the last used provider, else the first one with a key.
    pub fn last_config(&self) -> Option<ProviderConfig> {
        if let Some(name) = self.vault.last.as_deref() {
            if let Some(cfg) = self.config_for(name) {
                return Some(cfg);
            }
        }
        self.vault
            .providers
            .keys()
            .find_map(|name| self.config_for(name))
    }

    /// Stores a live `/connect` (credential + last provider) in the vault.
    /// Always saves (a retry after a failed write must hit the disk again)
    /// and returns the notice to show (save error included on failure).
    /// No secret comparison: same-principal dirty checks are not an oracle.
    pub fn remember(&mut self, name: &str, cfg: &ProviderConfig) -> String {
        self.vault.providers.insert(
            name.to_string(),
            Credential {
                base_url: cfg.base_url.clone(),
                model: cfg.model.clone(),
                key: cfg.api_key.clone(),
            },
        );
        self.vault.last = Some(name.to_string());
        self.rebuild();
        self.with_save(format!("key saved for {name} (encrypted)"))
    }

    /// Remembers `name` as the provider to reconnect at startup.
    pub fn mark_connected(&mut self, name: &str) {
        if self.vault.last.as_deref() == Some(name) {
            return;
        }
        self.vault.last = Some(name.to_string());
        if let Err(err) = self.store.save(&self.vault) {
            self.notice = Some(format!("not saved: {err:#}"));
        }
    }

    /// Number of providers holding a key (status line).
    pub fn keyed_count(&self) -> usize {
        self.vault
            .providers
            .values()
            .filter(|c| !c.key.is_empty())
            .count()
    }

    /// Rebuilds the rows from the known list + the vault.
    fn rebuild(&mut self) {
        let mut rows: Vec<Row> = KNOWN
            .iter()
            .map(|k| {
                let cred = self.vault.providers.get(k.name);
                let keyed = cred.is_some_and(|c| !c.key.is_empty());
                Row {
                    name: k.name.to_string(),
                    base_url: choose(cred.map(|c| c.base_url.as_str()).unwrap_or(""), k.base_url),
                    model: choose(cred.map(|c| c.model.as_str()).unwrap_or(""), k.model),
                    has_key: keyed,
                    known: true,
                }
            })
            .collect();
        let mut custom: Vec<(String, Credential)> = self
            .vault
            .providers
            .iter()
            .filter(|(name, _)| !KNOWN.iter().any(|k| k.name == name.as_str()))
            .map(|(name, cred)| (name.clone(), cred.clone()))
            .collect();
        custom.sort_by(|a, b| a.0.cmp(&b.0));
        rows.extend(custom.into_iter().map(|(name, cred)| Row {
            name,
            base_url: cred.base_url,
            model: cred.model,
            has_key: !cred.key.is_empty(),
            known: false,
        }));
        self.selected = self.selected.min(rows.len().saturating_sub(1));
        self.rows = rows;
    }

    /// Saves the vault, returning the message (+ the save error).
    fn with_save(&mut self, msg: String) -> String {
        match self.store.save(&self.vault) {
            Ok(()) => msg,
            Err(err) => format!("{msg} — not saved: {err:#}"),
        }
    }
}

/// `value` when set, else the fallback.
fn choose(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.trim().to_string()
    }
}
