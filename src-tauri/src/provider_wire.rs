//! Types filaires OpenAI du provider (`{base}/chat/completions`).
//!
//! Extrait de `provider.rs` (budget 500 lignes) : requêtes
//! (`WireMsg`, `PlainRequest`, `JsonRequest`), réponses décodées
//! (`ChatResponse`…), [`Completion`]/[`ToolCall`] publics et `truncate`.
//! Chemins stables : `provider.rs` ré-exporte les noms publics.

use serde::{Deserialize, Serialize};

/// Message filaire OpenAI (`{role, content}`).
#[derive(Debug, Serialize)]
pub(crate) struct WireMsg {
    pub(crate) role: String,
    pub(crate) content: String,
}

/// Requête sans outils (`reasoning_effort` omis si `None`).
#[derive(Debug, Serialize)]
pub(crate) struct PlainRequest<'a> {
    pub(crate) model: &'a str,
    pub(crate) messages: &'a [WireMsg],
    pub(crate) stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reasoning_effort: Option<&'a str>,
}

/// Requête outillée générique (`tools`/`tool_choice` omis sans outils).
#[derive(Debug, Serialize)]
pub(crate) struct JsonRequest<'a> {
    pub(crate) model: &'a str,
    pub(crate) messages: &'a [serde_json::Value],
    pub(crate) stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reasoning_effort: Option<&'a str>,
    #[serde(skip_serializing_if = "<[serde_json::Value]>::is_empty")]
    pub(crate) tools: &'a [serde_json::Value],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_choice: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatResponse {
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

/// Appel d'outil reçu (routé vers la tranche kiro-tools par le drain).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: String,
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
    pub(crate) fn from_response(resp: ChatResponse) -> Self {
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

/// Tronque sans couper un caractère UTF-8,
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
