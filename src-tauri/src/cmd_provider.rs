//! Commandes providers §3 (`commands.md`) : connexion, coffre, customs.
//!
//! Le flow clavier `/connect` (clé → modèle) devient un formulaire en une
//! commande (`connect_save`) ; `connect_muse` garde l'ordre TUI (coffre,
//! puis store Muse, sinon saisie manuelle). Aucune clé ni secret n'est
//! jamais loggué, affiché ou renvoyé (cf. `vault`, `provider`).

use std::sync::Arc;

use crate::core::Core;
use crate::dto::{ConnectResult, ConnectStatus, ProviderInfo};
use crate::provider::{connect_defaults, provider_label, Provider, ProviderConfig};

impl Core {
    /// `connect_status` : connecté ? provider + modèle, sinon non connecté.
    pub fn connect_status(&self) -> ConnectStatus {
        status_of(&self.lock())
    }

    /// `connect_save` : connecte `{provider, key, model?}` (clé → modèle
    /// en un formulaire), persiste chiffré, rend le résultat + statut.
    pub fn connect_save(
        &self,
        provider: String,
        key: String,
        model: Option<String>,
    ) -> ConnectResult {
        let name = provider.trim().to_string();
        let key = key.trim().to_string();
        if key.is_empty() {
            return failed(self, "empty key, type the key");
        }
        // Garde relâchée avant le match : `failed` reverrouille (`connect_status`).
        let custom = custom_defaults(&self.lock(), &name);
        let (base, default_model) = match connect_defaults(&name) {
            Some((base, model)) => (base.to_string(), model.to_string()),
            None => match custom {
                Some(defaults) => defaults,
                None => {
                    return failed(
                        self,
                        format!("unknown provider: {name} — providers_list shows every provider"),
                    )
                }
            },
        };
        let typed = model.unwrap_or_default().trim().to_string();
        let model = if typed.is_empty() {
            default_model
        } else {
            typed
        };
        if model.is_empty() {
            return failed(self, format!("model required (no default for {name})"));
        }
        let cfg = ProviderConfig {
            base_url: base,
            api_key: key,
            model,
        };
        match self.install_saved(&name, cfg) {
            Ok(status) => ConnectResult {
                ok: true,
                error: None,
                status,
            },
            Err(msg) => ConnectResult {
                ok: false,
                error: Some(msg),
                status: self.connect_status(),
            },
        }
    }

    /// `connect_muse` : clé du coffre, sinon store Muse, sinon saisie manuelle.
    pub fn connect_muse(&self) -> ConnectResult {
        // Garde relâchée avant la branche : `install_saved` reverrouille.
        let saved = self.lock().settings.config_for("muse");
        if let Some(cfg) = saved {
            return match self.install_saved("muse", cfg) {
                Ok(status) => ConnectResult {
                    ok: true,
                    error: None,
                    status,
                },
                Err(msg) => ConnectResult {
                    ok: false,
                    error: Some(msg),
                    status: self.connect_status(),
                },
            };
        }
        let from_store = ProviderConfig::muse_auth_path().and_then(ProviderConfig::from_muse_auth);
        match from_store {
            Ok(cfg) => match self.install_saved("muse", cfg) {
                Ok(status) => ConnectResult {
                    ok: true,
                    error: None,
                    status,
                },
                Err(msg) => ConnectResult {
                    ok: false,
                    error: Some(msg),
                    status: self.connect_status(),
                },
            },
            Err(_) => failed(
                self,
                "Muse store unreadable: use connect_save with provider \"muse\"",
            ),
        }
    }

    /// `providers_list` : 12 connus + customs (jamais les clés, `has_key` seul).
    pub fn providers_list(&self) -> Result<Vec<ProviderInfo>, String> {
        let mut inner = self.lock();
        if let Some(err) = inner.settings.reload() {
            return Err(err);
        }
        Ok(inner
            .settings
            .rows()
            .iter()
            .map(ProviderInfo::from)
            .collect())
    }

    /// `provider_add` : ajoute (ou met à jour) un provider custom.
    pub fn provider_add(
        &self,
        name: String,
        base_url: String,
        model: Option<String>,
    ) -> Result<(), String> {
        self.lock().settings.add_custom(
            name.trim(),
            base_url.trim(),
            model.unwrap_or_default().trim(),
        )
    }

    /// `provider_set_key` : stocke la clé de `name` (chiffré).
    pub fn provider_set_key(&self, name: String, key: String) -> Result<(), String> {
        self.lock().settings.set_key(name.trim(), key.trim())
    }

    /// `provider_remove` : retire un provider custom (connus intouchables ;
    /// la connexion en cours est gardée).
    pub fn provider_remove(&self, name: String) -> Result<(), String> {
        self.lock().settings.remove_custom(name.trim())
    }

    /// `provider_select` : fait de `name` le provider actif (clé requise).
    pub fn provider_select(&self, name: String) -> Result<ConnectStatus, String> {
        let name = name.trim().to_string();
        let cfg = self
            .lock()
            .settings
            .config_for(&name)
            .ok_or_else(|| format!("{name}: no key stored (use provider_set_key)"))?;
        self.install_saved(&name, cfg)
    }

    /// Installe un provider résolu, le retient au coffre, confirme
    /// (TUI `apply_connected` + `connect_saved`). Échec de construction
    /// → erreur système, rien de stocké.
    fn install_saved(&self, name: &str, cfg: ProviderConfig) -> Result<ConnectStatus, String> {
        let model = cfg.model.clone();
        match Provider::new(cfg.clone()) {
            Ok(provider) => {
                let mut inner = self.lock();
                inner.provider = Some(Arc::new(provider));
                inner.provider_name = Some(name.to_string());
                inner.push_system(format!("connected to {name} ({model})"));
                let msg = inner.settings.remember(name, &cfg);
                inner.push_system(msg);
                inner.settings.mark_connected(name);
                Ok(status_of(&inner))
            }
            Err(err) => {
                let msg = format!("connection failed: {err:#}");
                self.lock().push_system(msg.clone());
                Err(msg)
            }
        }
    }
}

/// Statut de connexion courant.
fn status_of(inner: &crate::core::CoreInner) -> ConnectStatus {
    match inner.provider.as_ref() {
        Some(provider) => {
            let cfg = provider.config();
            let name = inner
                .provider_name
                .clone()
                .unwrap_or_else(|| provider_label(&cfg.base_url).to_string());
            ConnectStatus {
                connected: true,
                provider: Some(name),
                model: Some(cfg.model.clone()),
            }
        }
        None => ConnectStatus {
            connected: false,
            provider: None,
            model: None,
        },
    }
}

/// Défauts (base, modèle) d'un provider custom du coffre.
fn custom_defaults(inner: &crate::core::CoreInner, name: &str) -> Option<(String, String)> {
    inner
        .settings
        .rows()
        .iter()
        .find(|r| r.name == name && !r.known)
        .map(|r| (r.base_url.clone(), r.model.clone()))
}

/// Résultat de connexion en échec (statut courant joint).
fn failed(core: &Core, error: impl Into<String>) -> ConnectResult {
    ConnectResult {
        ok: false,
        error: Some(error.into()),
        status: core.connect_status(),
    }
}

#[cfg(test)]
mod tests {
    use crate::core::tests::test_core;
    use crate::provider::{MUSE_BASE_URL, MUSE_DEFAULT_MODEL};

    #[test]
    fn save_connecte_persiste_et_statut() {
        let (core, _channels, dir) = test_core("save");
        assert!(!core.connect_status().connected);
        let empty = core.connect_save("openai".to_string(), "  ".to_string(), None);
        assert!(!empty.ok);
        let unknown = core.connect_save("zzz".to_string(), "sk-x".to_string(), None);
        assert!(!unknown.ok);
        let result = core.connect_save("openai".to_string(), "[REDACTED]".to_string(), None);
        assert!(result.ok, "{result:?}");
        assert_eq!(result.status.provider.as_deref(), Some("openai"));
        assert_eq!(result.status.model.as_deref(), Some("gpt-4o"));
        assert!(core.connect_status().connected);
        // Persisté chiffré : rechargé comme au démarrage.
        let raw = std::fs::read(dir.join("providers.enc")).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("[REDACTED]"));
        let fresh = crate::settings::Settings::with_store(crate::settings::Store::new(&dir));
        assert_eq!(fresh.config_for("openai").unwrap().api_key, "[REDACTED]");
        // La clé ne fuit ni dans les notes chat ni dans le résultat.
        let chat: String = core
            .lock()
            .sessions
            .current()
            .chat
            .messages()
            .iter()
            .map(|m| m.text.clone())
            .collect();
        assert!(!chat.contains("[REDACTED]"));
        assert!(chat.contains("connected to openai (gpt-4o)"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn liste_ajout_cle_selection_retrait() {
        let (core, _channels, dir) = test_core("crud");
        assert_eq!(core.providers_list().unwrap().len(), 12);
        core.provider_add(
            "mine".to_string(),
            "https://m/v1".to_string(),
            Some("mm".to_string()),
        )
        .unwrap();
        assert!(core
            .provider_add("openai".to_string(), "https://x".to_string(), None)
            .is_err());
        assert!(core
            .provider_set_key("zzz".to_string(), "sk".to_string())
            .is_err());
        core.provider_set_key("mine".to_string(), "sk-m".to_string())
            .unwrap();
        let rows = core.providers_list().unwrap();
        assert_eq!(rows.len(), 13);
        assert!(rows.iter().find(|r| r.name == "mine").unwrap().has_key);
        let status = core.provider_select("mine".to_string()).unwrap();
        assert!(status.connected);
        assert_eq!(status.model.as_deref(), Some("mm"));
        assert!(core.provider_select("groq".to_string()).is_err());
        assert!(core.provider_remove("openai".to_string()).is_err());
        core.provider_remove("mine".to_string()).unwrap();
        assert_eq!(core.providers_list().unwrap().len(), 12);
        // Connexion gardée après retrait du custom.
        assert!(core.connect_status().connected);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn muse_store_illisible_demande_la_saisie() {
        let (core, _channels, dir) = test_core("muse");
        let env =
            crate::test_env::EnvScope::new(&["HOME", "KH_API_KEY", "KH_BASE_URL", "KH_MODEL"]);
        let fake = dir.join("fakehome");
        std::fs::create_dir_all(&fake).unwrap();
        env.set("HOME", &fake);
        env.unset("KH_API_KEY");
        env.unset("KH_BASE_URL");
        env.unset("KH_MODEL");
        let result = core.connect_muse();
        assert!(!result.ok);
        assert!(result.error.unwrap().contains("Muse store unreadable"));
        assert!(!result.status.connected);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn muse_coffre_puis_store() {
        let (core, _channels, dir) = test_core("muse2");
        core.provider_set_key("muse".to_string(), "sk-muse".to_string())
            .unwrap();
        let result = core.connect_muse();
        assert!(result.ok, "{result:?}");
        assert_eq!(result.status.provider.as_deref(), Some("muse"));
        assert_eq!(result.status.model.as_deref(), Some(MUSE_DEFAULT_MODEL));
        assert_eq!(
            core.lock().provider.as_ref().unwrap().config().base_url,
            MUSE_BASE_URL
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
