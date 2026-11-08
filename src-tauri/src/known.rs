//! Providers embarqués : nom, base OpenAI-compatible, modèle par défaut.
//!
//! Port tel quel de `settings/known.rs` du TUI (KEEP). Les endpoints
//! customs s'ajoutent à l'exécution via `provider_add`.

use crate::provider::{OPENAI_BASE_URL, OPENAI_DEFAULT_MODEL};

/// Un provider embarqué : nom, base OpenAI-compatible, modèle par défaut.
pub struct Known {
    pub name: &'static str,
    pub base_url: &'static str,
    pub model: &'static str,
}

/// Providers proposés par `/settings` (les customs s'ajoutent par-dessus).
pub const KNOWN: [Known; 12] = [
    Known {
        name: "openai",
        base_url: OPENAI_BASE_URL,
        model: OPENAI_DEFAULT_MODEL,
    },
    Known {
        name: "muse",
        base_url: crate::provider::MUSE_BASE_URL,
        model: crate::provider::MUSE_DEFAULT_MODEL,
    },
    Known {
        name: "fireworks",
        base_url: "https://api.fireworks.ai/inference/v1",
        model: "accounts/fireworks/models/deepseek-v3",
    },
    Known {
        name: "deepseek",
        base_url: "https://api.deepseek.com/v1",
        model: "deepseek-chat",
    },
    Known {
        name: "openrouter",
        base_url: "https://openrouter.ai/api/v1",
        model: "deepseek/deepseek-chat",
    },
    Known {
        name: "groq",
        base_url: "https://api.groq.com/openai/v1",
        model: "llama-3.3-70b-versatile",
    },
    Known {
        name: "mistral",
        base_url: "https://api.mistral.ai/v1",
        model: "mistral-large-latest",
    },
    Known {
        name: "xai",
        base_url: "https://api.x.ai/v1",
        model: "grok-4",
    },
    Known {
        name: "cerebras",
        base_url: "https://api.cerebras.ai/v1",
        model: "llama-3.3-70b",
    },
    Known {
        name: "together",
        base_url: "https://api.together.xyz/v1",
        model: "deepseek-ai/DeepSeek-V3",
    },
    Known {
        name: "inferhub",
        base_url: "https://api.inferhub.ai/v1",
        model: "deepseek-v4.1-flash",
    },
    Known {
        name: "local",
        base_url: "http://127.0.0.1:11434/v1",
        model: "llama3.2",
    },
];

/// Défauts (base, modèle) d'un provider connu.
pub fn known_defaults(name: &str) -> Option<(&'static str, &'static str)> {
    KNOWN
        .iter()
        .find(|k| k.name == name)
        .map(|k| (k.base_url, k.model))
}

/// Vrai si `name` est un des 12 embarqués.
pub fn is_known(name: &str) -> bool {
    KNOWN.iter().any(|k| k.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_is_first_and_defaults_are_complete() {
        assert_eq!(KNOWN[0].name, "openai");
        assert_eq!(KNOWN[0].base_url, "https://api.openai.com/v1");
        assert_eq!(KNOWN[0].model, "gpt-4o");
        for known in KNOWN.iter() {
            assert!(!known.name.is_empty());
            assert!(known.base_url.starts_with("http"), "{}", known.name);
            assert!(!known.model.is_empty());
        }
    }

    #[test]
    fn known_defaults_looks_up_by_exact_name() {
        assert_eq!(known_defaults("openai").unwrap().1, "gpt-4o");
        assert_eq!(known_defaults("muse").unwrap().0, "https://api.meta.ai/v1");
        assert!(known_defaults("OpenAI").is_none());
        assert!(known_defaults("zzz").is_none());
        assert!(is_known("groq"));
        assert!(!is_known("zzz"));
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = KNOWN.iter().map(|k| k.name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count);
    }
}
