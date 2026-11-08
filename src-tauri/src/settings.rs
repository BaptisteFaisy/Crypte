//! Réglages providers : connus, clés chiffrées, customs.
//!
//! Port de la logique de `settings.rs` du TUI sans l'écran (curseur,
//! saisie, notices) : les commandes retournent `Result`, le frontend
//! affiche. L'écran ne montrait jamais que `has_key` ; ici `rows()`
//! ne porte jamais la clé non plus.

use crate::known::{is_known, known_defaults, KNOWN};
use crate::provider::ProviderConfig;
pub use crate::vault::{Credential, Store, Vault};

/// Chemin historique `settings::store` (attendu par `mbox/autorite.rs`).
pub mod store {
    pub(crate) use crate::vault::{write_private, Store};
}

/// Une ligne provider : jamais la clé, seulement `has_key`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub has_key: bool,
    pub known: bool,
}

/// Réglages + coffre chiffré.
pub struct Settings {
    store: Store,
    vault: Vault,
    rows: Vec<Row>,
}

impl Settings {
    /// Charge le store par défaut (`$XDG_DATA_HOME`) ; jamais de panique I/O.
    pub fn load() -> Self {
        match Store::default_store() {
            Ok(store) => Self::with_store(store),
            Err(_) => Self::with_store(Store::new(
                std::env::temp_dir().join("kiro-horror-tui-unavailable"),
            )),
        }
    }

    /// Charge le coffre de `store` (fichier manquant = lignes sans clé ;
    /// coffre illisible = lignes nues, l'erreur remonte via `reload`).
    pub fn with_store(store: Store) -> Self {
        let mut settings = Self {
            store,
            vault: Vault::new(),
            rows: Vec::new(),
        };
        settings.reload();
        settings
    }

    /// Relit le coffre depuis le disque (d'autres cryptes ont pu écrire).
    /// Rend l'erreur éventuelle (lignes reconstruites quand même).
    pub fn reload(&mut self) -> Option<String> {
        let err = match self.store.load() {
            Ok(vault) => {
                self.vault = vault;
                None
            }
            Err(err) => Some(format!("vault unreadable: {err:#}")),
        };
        self.rebuild();
        err
    }

    /// Lignes dans l'ordre d'affichage : connus, puis customs par nom.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Stocke la clé de `name` (connu ou custom existant).
    pub fn set_key(&mut self, name: &str, key: &str) -> Result<(), String> {
        let key = key.trim().to_string();
        if key.is_empty() {
            return Err(String::from("empty key"));
        }
        let row = self
            .rows
            .iter()
            .find(|r| r.name == name)
            .cloned()
            .ok_or_else(|| format!("unknown provider: {name}"))?;
        let mut cred = self
            .vault
            .providers
            .get(name)
            .cloned()
            .unwrap_or_else(|| Credential {
                base_url: row.base_url.clone(),
                model: row.model.clone(),
                key: String::new(),
            });
        cred.key = key;
        self.vault.providers.insert(name.to_string(), cred);
        self.rebuild();
        self.save().map_err(|err| format!("not saved: {err:#}"))
    }

    /// Change le modèle de `name` (vide = défaut connu).
    pub fn set_model(&mut self, name: &str, model: &str) -> Result<(), String> {
        let row = self
            .rows
            .iter()
            .find(|r| r.name == name)
            .cloned()
            .ok_or_else(|| format!("unknown provider: {name}"))?;
        let mut cred = self
            .vault
            .providers
            .get(name)
            .cloned()
            .unwrap_or_else(|| Credential {
                base_url: row.base_url.clone(),
                model: row.model.clone(),
                key: String::new(),
            });
        cred.model = model.trim().to_string();
        self.vault.providers.insert(name.to_string(), cred);
        self.rebuild();
        self.save().map_err(|err| format!("not saved: {err:#}"))
    }

    /// Change la base URL de `name`.
    pub fn set_base(&mut self, name: &str, base_url: &str) -> Result<(), String> {
        let row = self
            .rows
            .iter()
            .find(|r| r.name == name)
            .cloned()
            .ok_or_else(|| format!("unknown provider: {name}"))?;
        let mut cred = self
            .vault
            .providers
            .get(name)
            .cloned()
            .unwrap_or_else(|| Credential {
                base_url: row.base_url.clone(),
                model: row.model.clone(),
                key: String::new(),
            });
        cred.base_url = base_url.trim().trim_end_matches('/').to_string();
        self.vault.providers.insert(name.to_string(), cred);
        self.rebuild();
        self.save().map_err(|err| format!("not saved: {err:#}"))
    }

    /// Efface la clé stockée de `name` (endpoint gardé).
    pub fn delete_key(&mut self, name: &str) -> Result<(), String> {
        if !self.rows.iter().any(|r| r.name == name) {
            return Err(format!("unknown provider: {name}"));
        }
        let stored = self
            .vault
            .providers
            .get_mut(name)
            .is_some_and(|cred| !std::mem::take(&mut cred.key).is_empty());
        if !stored {
            return Err(format!("{name}: no key stored"));
        }
        self.rebuild();
        self.save().map_err(|err| format!("not saved: {err:#}"))
    }

    /// Ajoute (ou met à jour) un provider custom.
    pub fn add_custom(&mut self, name: &str, base_url: &str, model: &str) -> Result<(), String> {
        if is_known(name) {
            return Err(format!(
                "{name} is a known provider (use provider_set_key to store its key)"
            ));
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            || name.is_empty()
        {
            return Err(String::from("name: letters, digits, '-' or '_' only"));
        }
        if base_url.trim().is_empty() {
            return Err(String::from("base URL required"));
        }
        let cred = self.vault.providers.entry(name.to_string()).or_default();
        cred.base_url = base_url.trim().trim_end_matches('/').to_string();
        cred.model = model.trim().to_string();
        self.rebuild();
        self.save().map_err(|err| format!("not saved: {err:#}"))
    }

    /// Retire un provider custom (les connus ne se retirent pas).
    pub fn remove_custom(&mut self, name: &str) -> Result<(), String> {
        if is_known(name) {
            return Err(format!("{name} is a known provider (cannot remove)"));
        }
        if self.vault.providers.remove(name).is_none() {
            return Err(format!("unknown provider: {name}"));
        }
        if self.vault.last.as_deref() == Some(name) {
            self.vault.last = None;
        }
        self.rebuild();
        self.save().map_err(|err| format!("not saved: {err:#}"))
    }

    /// Config provider pour `name`, ou `None` sans clé stockée.
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

    /// Config du dernier provider utilisé, sinon du premier avec une clé.
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

    /// Nom du dernier provider utilisé, s'il y en a un.
    pub fn last_name(&self) -> Option<&str> {
        self.vault.last.as_deref()
    }

    /// Stocke une connexion `/connect` (credential + dernier provider).
    /// Sauvegarde toujours (un réessaie après un échec doit retoucher le
    /// disque) et rend le message à afficher (erreur incluse si échec).
    /// Pas de comparaison de secret : same-principal, pas un oracle.
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

    /// Retient `name` comme provider à reconnecter au démarrage.
    pub fn mark_connected(&mut self, name: &str) {
        if self.vault.last.as_deref() == Some(name) {
            return;
        }
        self.vault.last = Some(name.to_string());
        let _ = self.save();
    }

    /// Nombre de providers avec une clé (ligne de statut).
    pub fn keyed_count(&self) -> usize {
        self.vault
            .providers
            .values()
            .filter(|c| !c.key.is_empty())
            .count()
    }

    /// Sauvegarde le coffre.
    fn save(&self) -> anyhow::Result<()> {
        self.store.save(&self.vault)
    }

    /// Reconstruit les lignes depuis les connus + le coffre.
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
            .filter(|(name, _)| !is_known(name))
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
        self.rows = rows;
    }

    /// Sauvegarde le coffre, rend le message (+ l'erreur de sauvegarde).
    fn with_save(&mut self, msg: String) -> String {
        match self.save() {
            Ok(()) => msg,
            Err(err) => format!("{msg} — not saved: {err:#}"),
        }
    }
}

/// `value` quand renseigné, sinon le repli.
fn choose(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Réglages sur un coffre jetable (jamais le vrai `providers.enc`).
    fn settings(name: &str) -> (Settings, PathBuf) {
        let dir = std::env::temp_dir().join(format!("kh-settings-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let settings = Settings::with_store(Store::new(dir.clone()));
        (settings, dir)
    }

    #[test]
    fn rows_list_known_first_without_keys() {
        let (settings, dir) = settings("rows");
        assert_eq!(settings.rows().len(), 12);
        assert_eq!(settings.rows()[0].name, "openai");
        assert!(settings.rows().iter().all(|r| r.known && !r.has_key));
        assert_eq!(settings.keyed_count(), 0);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn set_key_then_config_for_and_last_config() {
        let (mut settings, dir) = settings("key");
        assert!(settings.set_key("openai", "  ").is_err());
        assert!(settings.set_key("zzz", "sk-x").is_err());
        settings.set_key("openai", "sk-test").unwrap();
        assert!(settings.rows()[0].has_key);
        let cfg = settings.config_for("openai").unwrap();
        assert_eq!(cfg.api_key, "sk-test");
        assert_eq!(cfg.model, "gpt-4o");
        assert!(settings.config_for("muse").is_none());
        assert_eq!(settings.last_config().unwrap().api_key, "sk-test");
        assert_eq!(settings.keyed_count(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn add_custom_validates_and_sorts() {
        let (mut settings, dir) = settings("custom");
        assert!(settings.add_custom("openai", "https://x", "").is_err());
        assert!(settings.add_custom("bad name", "https://x", "").is_err());
        assert!(settings.add_custom("mine", "  ", "").is_err());
        settings.add_custom("zeta", "https://z/", "zm").unwrap();
        settings.add_custom("alpha", "https://a", "").unwrap();
        let names: Vec<&str> = settings.rows().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(&names[12..], &["alpha", "zeta"]);
        assert!(!settings.rows()[12].known);
        assert_eq!(settings.rows()[13].base_url, "https://z");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn remove_custom_keeps_known_and_clears_last() {
        let (mut settings, dir) = settings("remove");
        assert!(settings.remove_custom("openai").is_err());
        assert!(settings.remove_custom("ghost").is_err());
        settings.add_custom("mine", "https://m", "mm").unwrap();
        settings.set_key("mine", "sk-m").unwrap();
        let cfg = settings.config_for("mine").unwrap();
        settings.remember("mine", &cfg);
        assert_eq!(settings.last_name(), Some("mine"));
        settings.remove_custom("mine").unwrap();
        assert_eq!(settings.rows().len(), 12);
        assert!(settings.last_name().is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn remember_persists_encrypted_and_marks_last() {
        let (mut settings, dir) = settings("remember");
        let cfg = ProviderConfig {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "[REDACTED]".to_string(),
            model: "gpt-4o-mini".to_string(),
        };
        let msg = settings.remember("openai", &cfg);
        assert!(msg.contains("key saved for openai (encrypted)"), "{msg}");
        let raw = std::fs::read(dir.join("providers.enc")).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("[REDACTED]"));
        let fresh = Settings::with_store(Store::new(&dir));
        assert_eq!(fresh.last_name(), Some("openai"));
        assert_eq!(fresh.config_for("openai").unwrap().model, "gpt-4o-mini");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn delete_key_erases_but_keeps_endpoint() {
        let (mut settings, dir) = settings("delkey");
        settings.set_key("groq", "sk-g").unwrap();
        settings.delete_key("groq").unwrap();
        assert!(settings.delete_key("groq").is_err());
        assert!(settings.delete_key("zzz").is_err());
        assert!(
            !settings
                .rows()
                .iter()
                .find(|r| r.name == "groq")
                .unwrap()
                .has_key
        );
        assert!(settings.config_for("groq").is_none());
        std::fs::remove_dir_all(dir).ok();
    }
}
