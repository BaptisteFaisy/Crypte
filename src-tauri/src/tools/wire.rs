//! Types filaires des outils (schémas OpenAI `function`, appels, messages).
//!
//! [`ToolDef`] décrit un outil au modèle, [`ToolCall`] est un appel décidé
//! par lui, [`WireMsg`] un message d'échange (rôles `system`/`user`/
//! `assistant`/`tool`, appels inclus).

use serde::Serialize;
use serde_json::Value;

/// Outil appelable par le modèle (forme OpenAI : `type` + `function`).
#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    #[serde(rename = "type")]
    kind: &'static str,
    pub function: ToolFn,
}

/// Corps d'un schéma `function` : nom + description + paramètres JSON Schema.
#[derive(Debug, Clone, Serialize)]
pub struct ToolFn {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolDef {
    /// Schéma `function` complet (paramètres JSON Schema fournis).
    pub fn new(name: &str, description: &str, parameters: Value) -> Self {
        Self {
            kind: "function",
            function: ToolFn {
                name: name.to_string(),
                description: description.to_string(),
                parameters,
            },
        }
    }
}

/// Appel d'outil décidé par le modèle (`id` + nom + args JSON bruts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: String,
}

/// Message filaire (texte et/ou appels, résultat via `tool_call_id`).
#[derive(Debug, Clone, Serialize)]
pub struct WireMsg {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<WireCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// Appel rejoué au provider (forme OpenAI stricte).
#[derive(Debug, Clone, Serialize)]
pub struct WireCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: WireFn,
}

/// Fonction rejouée : nom + arguments JSON bruts.
#[derive(Debug, Clone, Serialize)]
pub struct WireFn {
    pub name: String,
    pub arguments: String,
}

impl WireMsg {
    /// Message texte (`system` / `user` / `assistant` simple).
    pub fn text(role: String, content: String) -> Self {
        Self {
            role,
            content: Some(content),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Tour `assistant` avec appels d'outils (texte + appels rejoués).
    pub fn assistant_calls(text: &str, calls: &[ToolCall]) -> Self {
        let tool_calls = calls
            .iter()
            .map(|c| WireCall {
                id: c.id.clone(),
                kind: "function",
                function: WireFn {
                    name: c.name.clone(),
                    arguments: c.args.clone(),
                },
            })
            .collect();
        Self {
            role: String::from("assistant"),
            content: (!text.is_empty()).then(|| text.to_string()),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    /// Résultat d'outil (`tool`, corrélé par `tool_call_id`).
    pub fn result(call_id: String, text: String) -> Self {
        Self {
            role: String::from("tool"),
            content: Some(text),
            tool_calls: None,
            tool_call_id: Some(call_id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn def_serialise_en_function_openai() {
        let def = ToolDef::new("ping", "test", json!({"type": "object"}));
        let raw = serde_json::to_value(&def).unwrap();
        assert_eq!(raw["type"], "function");
        assert_eq!(raw["function"]["name"], "ping");
        assert_eq!(def.function.name, "ping");
    }

    #[test]
    fn wire_omet_les_champs_vides() {
        let msg = WireMsg::text(String::from("user"), String::from("hi"));
        let raw = serde_json::to_value(&msg).unwrap();
        assert_eq!(raw, json!({"role": "user", "content": "hi"}));
        let call = ToolCall {
            id: String::from("c1"),
            name: String::from("ping"),
            args: String::from("{}"),
        };
        let turn = WireMsg::assistant_calls("", std::slice::from_ref(&call));
        let raw = serde_json::to_value(&turn).unwrap();
        assert!(raw.get("content").is_none());
        assert_eq!(raw["tool_calls"][0]["function"]["name"], "ping");
        let done = WireMsg::result(String::from("c1"), String::from("pong"));
        let raw = serde_json::to_value(&done).unwrap();
        assert_eq!(raw["tool_call_id"], "c1");
    }
}
