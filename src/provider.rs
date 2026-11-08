//! Client HTTP non-streaming OpenAI-compatible (`POST {base}/chat/completions`).
//!
//! Contrat : `ProviderConfig { base_url, api_key, model }` (`from_env` +
//! `from_toml`), `Provider::complete` non-streaming, timeout 60 s, usage API
//! mappé vers [`Completion`], erreurs `anyhow` avec contexte (URL + modèle).
//!
//! La clé API n'est JAMAIS logguée : [`ProviderConfig`] a un `Debug` manuel
//! qui affiche `[redacted]`, et aucun message d'erreur ne l'inclut.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::tools::{ToolCall, ToolDef, WireMsg};

/// Base OpenAI (défaut global, format OpenAI-compatible).
pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
/// Modèle OpenAI par défaut (flow `/connect openai`).
pub const OPENAI_DEFAULT_MODEL: &str = "gpt-4o";
/// Base Muse (compatible OpenAI, défaut du mode `--muse-auth`).
pub const MUSE_BASE_URL: &str = "https://api.meta.ai/v1";
/// Modèle Muse par défaut (mode `--muse-auth`).
pub const MUSE_DEFAULT_MODEL: &str = "muse-spark-1.3-contributor";
/// Timeout HTTP fixe (contrat : 60 s).
const TIMEOUT_SECS: u64 = 60;
/// Longueur max des extraits de corps d'erreur HTTP.
const MAX_ERROR_BODY: usize = 2000;

/// Configuration du fournisseur LLM.
#[derive(Clone)]
pub struct ProviderConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

// Debug manuel : la clé n'apparaît jamais dans les logs ni les erreurs.
impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"[redacted]")
            .field("model", &self.model)
            .finish()
    }
}

impl ProviderConfig {
    /// Depuis l'environnement : `KH_BASE_URL` (défaut OpenAI), `KH_API_KEY`
    /// (obligatoire), `KH_MODEL` (obligatoire).
    pub fn from_env() -> Result<Self> {
        let base_url = read_env("KH_BASE_URL").unwrap_or_else(|| OPENAI_BASE_URL.to_string());
        let api_key = read_env("KH_API_KEY").context("missing KH_API_KEY (required)")?;
        let model = read_env("KH_MODEL").context("missing KH_MODEL (required)")?;
        Ok(Self {
            base_url,
            api_key,
            model,
        })
    }

    /// Depuis un fichier TOML (clés plates `base_url` / `api_key` / `model`
    /// ou section `[provider]`), avec surcharge par l'environnement
    /// (`KH_BASE_URL` / `KH_API_KEY` / `KH_MODEL` prioritaires).
    pub fn from_toml(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading file {}", path.display()))?;
        let doc: TomlDoc =
            toml::from_str(&raw).with_context(|| format!("parsing TOML {}", path.display()))?;
        let base_url = read_env("KH_BASE_URL")
            .or_else(|| doc.base_url())
            .unwrap_or_else(|| OPENAI_BASE_URL.to_string());
        let api_key = read_env("KH_API_KEY")
            .or_else(|| doc.api_key())
            .with_context(|| {
                format!(
                    "missing API key (neither KH_API_KEY nor 'api_key' in {})",
                    path.display()
                )
            })?;
        let model = read_env("KH_MODEL")
            .or_else(|| doc.model())
            .with_context(|| {
                format!(
                    "missing model (neither KH_MODEL nor 'model' in {})",
                    path.display()
                )
            })?;
        Ok(Self {
            base_url,
            api_key,
            model,
        })
    }

    /// Chemin du store Muse : `$HOME/.config/muse/auth.json`.
    pub fn muse_auth_path() -> Result<PathBuf> {
        let home = read_env("HOME").context("missing HOME (Muse auth path not found)")?;
        Ok(PathBuf::from(home).join(".config/muse/auth.json"))
    }

    /// Depuis le store Muse (`/providers/meta/api_key`), avec surcharge par
    /// l'environnement (`KH_BASE_URL` / `KH_API_KEY` / `KH_MODEL` prioritaires).
    pub fn from_muse_auth(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading file {}", path.display()))?;
        let doc: serde_json::Value = serde_json::from_str(&raw)
            .with_context(|| format!("parsing JSON {}", path.display()))?;
        let file_key = doc
            .pointer("/providers/meta/api_key")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        let base_url = read_env("KH_BASE_URL").unwrap_or_else(|| MUSE_BASE_URL.to_string());
        let api_key = read_env("KH_API_KEY").or(file_key).with_context(|| {
            format!(
                "missing API key (neither KH_API_KEY nor '/providers/meta/api_key' in {})",
                path.display()
            )
        })?;
        let model = read_env("KH_MODEL").unwrap_or_else(|| MUSE_DEFAULT_MODEL.to_string());
        Ok(Self {
            base_url,
            api_key,
            model,
        })
    }
}

/// Défauts de connexion interactive : `(base_url, modèle)` pour
/// `openai` ou `muse` (`None` = provider inconnu, valeurs valides).
pub fn connect_defaults(provider: &str) -> Option<(&'static str, &'static str)> {
    match provider {
        "openai" => Some((OPENAI_BASE_URL, OPENAI_DEFAULT_MODEL)),
        "muse" => Some((MUSE_BASE_URL, MUSE_DEFAULT_MODEL)),
        // Anything shipped by `/settings` (fireworks, groq, local, …).
        other => crate::settings::known_defaults(other),
    }
}

/// Nom court déduit de la base (`muse`/`openai`, sinon la base elle-même).
pub fn provider_label(base_url: &str) -> &str {
    match base_url.trim_end_matches('/') {
        MUSE_BASE_URL => "muse",
        OPENAI_BASE_URL => "openai",
        _ => base_url,
    }
}

/// Lit une variable d'environnement, `None` si absente ou vide/blanche.
fn read_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

// Pas de Debug ici : ces structures portent la clé en clair en mémoire.
#[derive(Default, Deserialize)]
struct TomlDoc {
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    provider: Option<TomlSection>,
}

#[derive(Default, Deserialize)]
struct TomlSection {
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

impl TomlDoc {
    fn base_url(&self) -> Option<String> {
        pick(
            &self.base_url,
            self.section().and_then(|p| p.base_url.as_ref()),
        )
    }

    fn api_key(&self) -> Option<String> {
        pick(
            &self.api_key,
            self.section().and_then(|p| p.api_key.as_ref()),
        )
    }

    fn model(&self) -> Option<String> {
        pick(&self.model, self.section().and_then(|p| p.model.as_ref()))
    }

    fn section(&self) -> Option<&TomlSection> {
        self.provider.as_ref()
    }
}

/// Première valeur non vide (clé plate prioritaire, section en repli).
fn pick(flat: &Option<String>, section: Option<&String>) -> Option<String> {
    flat.as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .or_else(|| section.map(|s| s.trim()).filter(|v| !v.is_empty()))
        .map(str::to_string)
}

/// Client non-streaming vers `{base_url}/chat/completions` (timeout 60 s).
pub struct Provider {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl Provider {
    /// Construit le client HTTP (timeout 60 s, contrat fixe). Trafic
    /// séquentiel d'un seul utilisateur : une seule connexion idle par
    /// hôte, libérée après 30 s au repos (les requêtes sont espacées de
    /// toute façon, le coût de reconnexion est nul en pratique).
    pub fn new(config: ProviderConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .pool_max_idle_per_host(1)
            .pool_idle_timeout(Duration::from_secs(30))
            .build()
            .context("building the HTTP client")?;
        Ok(Self { config, client })
    }

    /// Accès lecture seule à la configuration.
    #[allow(dead_code)]
    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }

    /// URL cible : `{base_url}/chat/completions` (slash final toléré).
    fn endpoint(&self) -> String {
        format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        )
    }

    /// Appel non-streaming : `messages` = paires `(rôle, contenu)`.
    /// Retourne le texte du premier choix + l'usage réel facturé par l'API.
    /// Délègue à [`Self::complete_full`] (modèle config, sans effort).
    pub async fn complete(&self, messages: &[(String, String)]) -> Result<Completion> {
        self.complete_full(messages, &self.config.model, None).await
    }

    /// Appel généralisé : modèle et effort de réflexion explicites.
    /// `effort` à `None` → champ `reasoning_effort` omis du JSON.
    /// Sans outils : JSON identique à l'ancien format (zéro régression).
    pub async fn complete_full(
        &self,
        messages: &[(String, String)],
        model: &str,
        effort: Option<&str>,
    ) -> Result<Completion> {
        let wire: Vec<WireMsg> = messages
            .iter()
            .map(|(role, content)| WireMsg::text(role.clone(), content.clone()))
            .collect();
        self.complete_with_tools(&wire, &[], model, effort).await
    }

    /// Appel outillé : messages filaires + schémas (`tools` omis si vide,
    /// `tool_choice: auto` sinon). Parse aussi les `tool_calls` reçus.
    pub async fn complete_with_tools(
        &self,
        messages: &[WireMsg],
        tools: &[ToolDef],
        model: &str,
        effort: Option<&str>,
    ) -> Result<Completion> {
        let url = self.endpoint();
        let body = ToolsRequest {
            model,
            messages,
            stream: false,
            reasoning_effort: effort,
            tools,
            tool_choice: (!tools.is_empty()).then_some("auto"),
        };
        let resp = self
            .client
            .post(&url)
            .bearer_auth(self.config.api_key.as_str())
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {url} (model {model})"))?;
        let status = resp.status();
        if !status.is_success() {
            let raw = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "POST {url} (model {model}) -> HTTP {status}: {}",
                truncate(&raw, MAX_ERROR_BODY)
            );
        }
        let parsed: ChatResponse = resp
            .json()
            .await
            .with_context(|| format!("decoding JSON from {url} (model {model})"))?;
        Ok(Completion::from_response(parsed))
    }
}

/// Requête outillée (`tools`/`tool_choice` omis quand aucun outil).
#[derive(Debug, Serialize)]
struct ToolsRequest<'a> {
    model: &'a str,
    messages: &'a [WireMsg],
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'a str>,
    #[serde(skip_serializing_if = "<[ToolDef]>::is_empty")]
    tools: &'a [ToolDef],
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    #[serde(default)]
    message: Option<ResponseMessage>,
}

#[derive(Debug, Default, Deserialize)]
struct ResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallIn>>,
}

/// Appel reçu (forme OpenAI : `id` + `function`, `type` ignoré).
#[derive(Debug, Deserialize)]
struct ToolCallIn {
    #[serde(default)]
    id: String,
    #[serde(default)]
    function: ToolFnIn,
}

/// Fonction reçue : nom + arguments JSON bruts.
#[derive(Debug, Default, Deserialize)]
struct ToolFnIn {
    #[serde(default)]
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Debug, Default, Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

/// Réponse textuelle + appels d'outils + usage réel facturé par l'API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

impl Completion {
    /// Mappe la réponse API (choix 0 + `tool_calls` + usage) vers le local.
    fn from_response(resp: ChatResponse) -> Self {
        let msg = resp.choices.into_iter().next().and_then(|c| c.message);
        let (text, tool_calls) = match msg {
            Some(m) => (
                m.content.unwrap_or_default(),
                m.tool_calls
                    .unwrap_or_default()
                    .into_iter()
                    .map(|c| ToolCall {
                        id: c.id,
                        name: c.function.name,
                        args: c.function.arguments,
                    })
                    .collect(),
            ),
            None => (String::new(), Vec::new()),
        };
        let (prompt_tokens, completion_tokens) = resp
            .usage
            .map(|u| (u.prompt_tokens, u.completion_tokens))
            .unwrap_or_default();
        Self {
            text,
            tool_calls,
            prompt_tokens,
            completion_tokens,
        }
    }
}

/// Tronque sans couper un caractère UTF-8 (budgets `web` inclus),
/// en une seule passe (frontière d'octets, pas de double parcours).
pub(crate) fn truncate(s: &str, max_chars: usize) -> String {
    match s.char_indices().map(|(i, _)| i).nth(max_chars) {
        Some(end) => s[..end].to_string(),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_env::EnvScope;

    /// Vars neutralisées pendant le test (restaurées par `EnvScope`).
    const KH_VARS: [&str; 3] = ["KH_API_KEY", "KH_BASE_URL", "KH_MODEL"];

    #[test]
    fn muse_auth_fixture_is_parsed() {
        let env = EnvScope::new(&KH_VARS);
        for key in KH_VARS {
            env.unset(key);
        }
        let path = std::env::temp_dir().join(format!(
            "kh-muse-auth-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            r#"{"providers":{"meta":{"api_key":"  kh-test-key  "}}}"#,
        )
        .unwrap();
        let cfg = ProviderConfig::from_muse_auth(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(cfg.base_url, MUSE_BASE_URL);
        assert_eq!(cfg.api_key, "kh-test-key");
        assert_eq!(cfg.model, MUSE_DEFAULT_MODEL);
    }

    #[test]
    fn reponse_outils_parsee_vers_completion() {
        let raw = r#"{"choices":[{"message":{"content":null,"tool_calls":[
            {"id":"c1","type":"function","function":{"name":"shell_run","arguments":"{}"}}
            ]}}],"usage":{"prompt_tokens":7,"completion_tokens":3}}"#;
        let resp: ChatResponse = serde_json::from_str(raw).unwrap();
        let done = Completion::from_response(resp);
        assert_eq!(done.text, "");
        assert_eq!(done.tool_calls.len(), 1);
        assert_eq!(done.tool_calls[0].name, "shell_run");
        assert_eq!((done.prompt_tokens, done.completion_tokens), (7, 3));
    }
}
