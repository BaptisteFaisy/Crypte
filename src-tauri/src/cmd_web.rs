//! Commande §4 : `web_search` (recherche + synthèse, direct).
//!
//! Chaîne Tavily → Brave → DuckDuckGo → Wikipedia, 20 s max, cache
//! 10 min (voir `web`) ; sans LLM : résultats bruts (comportement TUI
//! déconnecté). Noms de backends en casse d'affichage (contrat).
//! La compta tokens part en outbox (`WebDone`, ledger + event core).

use std::sync::Mutex;
use std::time::Duration;

use tauri::State;

use crate::state::{lock, BackendOut, ToolsBackend};
use crate::views::{SearchResultView, WebAnswerView};

/// Délai global de recherche (comme le TUI, synthèse exclue).
const RESEARCH_DEADLINE: Duration = Duration::from_secs(20);

/// `web_search` : réponse sourcée (async direct, pas d'ACK).
#[tauri::command]
pub async fn web_search(
    backend: State<'_, Mutex<ToolsBackend>>,
    chat: usize,
    query: String,
) -> Result<WebAnswerView, String> {
    if query.trim().is_empty() {
        return Err(String::from("empty query"));
    }
    let (llm, model, effort) = {
        let backend = lock(&backend)?;
        (
            backend.llm.clone(),
            backend.llm_model.clone(),
            backend.llm_effort.clone(),
        )
    };
    let asked = query.trim().to_string();
    let (found, text, prompt, completion) =
        run_search(llm.as_deref(), &model, effort.as_deref(), &asked).await?;
    let view = answer_view(&found, &text);
    lock(&backend)?.outbox.push(BackendOut::WebDone {
        chat,
        backend: view.backend.clone(),
        text,
        prompt,
        completion,
    });
    Ok(view)
}

/// Recherche (résultats + backend) puis synthèse (texte + tokens).
/// Le 2e appel repasse par le cache (10 min, gratuit).
async fn run_search(
    llm: Option<&dyn crate::provider::Llm>,
    model: &str,
    effort: Option<&str>,
    asked: &str,
) -> Result<(crate::web::Research, String, u64, u64), String> {
    let client = crate::web::WebClient::from_env();
    let found = tokio::time::timeout(RESEARCH_DEADLINE, client.research(asked))
        .await
        .map_err(|_| String::from("research deadline"))?
        .map_err(|err| format!("{err:#}"))?;
    let answered = crate::web::answer(asked.to_string(), llm, model, effort).await;
    match answered {
        Ok(done) => Ok((found, done.text, done.prompt_tokens, done.completion_tokens)),
        Err(err) => Err(format!("{err:#}")),
    }
}

/// Vue contrat (backend en casse d'affichage + sources).
fn answer_view(found: &crate::web::Research, text: &str) -> WebAnswerView {
    WebAnswerView {
        backend: display_backend(found.backend),
        text: text.to_string(),
        sources: found
            .results
            .iter()
            .map(|r| SearchResultView {
                title: r.title.clone(),
                url: r.url.clone(),
                snippet: r.snippet.clone(),
            })
            .collect(),
    }
}

/// Nom d'affichage du backend (contrat : casse `Tavily`, `Brave`…).
fn display_backend(backend: &str) -> String {
    match backend {
        "tavily" => String::from("Tavily"),
        "brave" => String::from("Brave"),
        "duckduckgo" => String::from("DuckDuckGo"),
        "wikipedia" => String::from("Wikipedia"),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backends_en_casse_d_affichage() {
        assert_eq!(display_backend("tavily"), "Tavily");
        assert_eq!(display_backend("brave"), "Brave");
        assert_eq!(display_backend("duckduckgo"), "DuckDuckGo");
        assert_eq!(display_backend("wikipedia"), "Wikipedia");
    }

    #[test]
    fn vue_reponse_serialise_sources() {
        let view = WebAnswerView {
            backend: String::from("Wikipedia"),
            text: String::from("bla\n\nSources:\n[1] x"),
            sources: vec![SearchResultView {
                title: String::from("x"),
                url: String::from("https://x.example/"),
                snippet: String::from("s"),
            }],
        };
        let raw = serde_json::to_value(&view).unwrap();
        assert_eq!(raw["backend"], "Wikipedia");
        assert_eq!(raw["sources"].as_array().unwrap().len(), 1);
        assert_eq!(raw["sources"][0]["url"], "https://x.example/");
    }
}
