//! Web search for `/web`: backends, page fetch, cache, synthesis input.
//!
//! Chain (first non-empty wins): Tavily → Brave → DuckDuckGo → Wikipedia
//! (FR+EN). 8 s per backend, 20 s global deadline, fetch skipped when
//! snippets are rich, 10 min cache. Keys never logged. Result cache in
//! [`cache`], pure parsing in [`parse`], relevance shaping in [`rank`].

mod cache;
mod parse;
mod rank;

use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::provider::Provider;

use self::cache::{cached_search, ddg_cooling, note_ddg_challenge, store_search};
use self::parse::{parse_ddg_html, parse_json_results, parse_wiki_generator, strip_html};
use self::rank::{build_context, dedup_results, fr_first, need_fetch, pick_passages};

/// Timeout per backend call and per page fetch.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(8);
/// Results kept per search (also the numbered `[n]` source count).
pub(super) const MAX_RESULTS: usize = 5;
/// Pages fetched in parallel to enrich the context.
pub(super) const MAX_FETCHES: usize = 3;
/// Chars kept per snippet.
pub(super) const MAX_SNIPPET: usize = 500;
/// Chars kept per fetched page (best query passages, not the head).
const MAX_PAGE_CHARS: usize = 1200;
/// Total chars of the LLM context block (fair share per source).
pub(super) const MAX_CONTEXT_CHARS: usize = 6000;
/// Bytes read per page before stripping (stream cut, never over-read).
const MAX_BODY_BYTES: usize = 64 * 1024;
/// Global research deadline (search + fetch, synthesis excluded).
const RESEARCH_DEADLINE: Duration = Duration::from_secs(20);
/// DuckDuckGo HTML endpoint (free, no key, sometimes bot-blocked).
const DDG_ENDPOINT: &str = "https://html.duckduckgo.com/html/";
/// Browser user-agent, default on every request (Wikipedia 403s without
/// one; HTML endpoints reject the default reqwest one).
const BROWSER_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120 Safari/537.36";
/// Brave Search API endpoint (`X-Subscription-Token` header).
const BRAVE_ENDPOINT: &str = "https://api.search.brave.com/res/v1/web/search";
/// Tavily Search API endpoint (key inside the JSON body).
const TAVILY_ENDPOINT: &str = "https://api.tavily.com/search";

/// One web result: title + URL + snippet (all stripped, truncated).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Full research: results + LLM-ready context + backend that answered.
#[derive(Debug, Clone)]
pub struct Research {
    pub results: Vec<SearchResult>,
    pub context: String,
    pub backend: &'static str,
}

/// Final answer for the drain: display text + billed LLM usage
/// (zero tokens when shown raw, without LLM synthesis).
#[derive(Debug)]
pub struct WebAnswer {
    pub text: String,
    pub backend: &'static str,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// Search client: shared HTTP client + optional premium keys.
#[derive(Debug, Clone)]
pub struct WebClient {
    client: reqwest::Client,
    brave_key: Option<String>,
    tavily_key: Option<String>,
}

impl WebClient {
    /// From the environment: `BRAVE_API_KEY` / `TAVILY_API_KEY`
    /// (blank = absent, premium backends skipped silently).
    pub fn from_env() -> Self {
        let key = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        Self {
            client: HTTP_CLIENT
                .get_or_init(|| {
                    reqwest::Client::builder()
                        .timeout(SEARCH_TIMEOUT)
                        .user_agent(BROWSER_UA)
                        .build()
                        .unwrap_or_else(|_| reqwest::Client::new())
                })
                .clone(),
            brave_key: key("BRAVE_API_KEY"),
            tavily_key: key("TAVILY_API_KEY"),
        }
    }

    /// Sends a JSON request: timeout, HTTP check, JSON decode.
    async fn fetch_json(
        &self,
        who: &str,
        req: reqwest::RequestBuilder,
    ) -> Result<serde_json::Value> {
        let resp = timeout(SEARCH_TIMEOUT, req.send())
            .await
            .with_context(|| format!("{who} timeout"))?
            .with_context(|| format!("{who} request"))?
            .error_for_status()
            .with_context(|| format!("{who} http"))?;
        timeout(SEARCH_TIMEOUT, resp.json())
            .await
            .with_context(|| format!("{who} timeout"))?
            .with_context(|| format!("{who} json"))
    }

    /// Backend chain: Tavily → Brave → DuckDuckGo → Wikipedia.
    /// A backend that errors or returns nothing falls through silently.
    pub async fn search(&self, query: &str) -> Result<(&'static str, Vec<SearchResult>)> {
        if self.tavily_key.is_some() {
            if let Ok(found) = self.search_tavily(query).await {
                if !found.is_empty() {
                    return Ok(("tavily", found));
                }
            }
        }
        if self.brave_key.is_some() {
            if let Ok(found) = self.search_brave(query).await {
                if !found.is_empty() {
                    return Ok(("brave", found));
                }
            }
        }
        if !ddg_cooling() {
            if let Ok(found) = self.search_ddg(query).await {
                if !found.is_empty() {
                    return Ok(("duckduckgo", found));
                }
            }
        }
        let found = self.search_wikipedia(query).await?;
        if found.is_empty() {
            anyhow::bail!("no web results");
        }
        Ok(("wikipedia", found))
    }

    /// Tavily (`search_depth: basic`, LLM-optimized `content` snippets).
    async fn search_tavily(&self, query: &str) -> Result<Vec<SearchResult>> {
        let body = serde_json::json!({
            "api_key": self.tavily_key.as_deref().unwrap_or_default(),
            "query": query,
            "max_results": MAX_RESULTS,
            "search_depth": "basic",
            "include_answer": false,
        });
        let req = self.client.post(TAVILY_ENDPOINT).json(&body);
        let json = self.fetch_json("tavily", req).await?;
        let items = json
            .get("results")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(parse_json_results(&items, "content"))
    }

    /// Brave Search API (`X-Subscription-Token`, results under `/web`).
    async fn search_brave(&self, query: &str) -> Result<Vec<SearchResult>> {
        let url = reqwest::Url::parse_with_params(
            BRAVE_ENDPOINT,
            [("q", query), ("count", &MAX_RESULTS.to_string())],
        )
        .context("brave url")?;
        let req = self.client.get(url).header(
            "X-Subscription-Token",
            self.brave_key.as_deref().unwrap_or_default(),
        );
        let json = self.fetch_json("brave", req).await?;
        let items = json
            .pointer("/web/results")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(parse_json_results(&items, "description"))
    }

    /// DuckDuckGo HTML (free; a challenge page parses to zero results
    /// and arms the cooldown, falling through to Wikipedia).
    async fn search_ddg(&self, query: &str) -> Result<Vec<SearchResult>> {
        let url =
            reqwest::Url::parse_with_params(DDG_ENDPOINT, [("q", query)]).context("ddg url")?;
        let resp = timeout(SEARCH_TIMEOUT, self.client.get(url).send())
            .await
            .context("ddg timeout")?
            .context("ddg request")?;
        let resp = resp.error_for_status().context("ddg http")?;
        let html = timeout(SEARCH_TIMEOUT, resp.text())
            .await
            .context("ddg timeout")?
            .context("ddg body")?;
        if html.contains("anomaly") {
            note_ddg_challenge();
        }
        Ok(parse_ddg_html(&html))
    }

    /// Wikipedia FR + EN in parallel (free, never bot-blocked), ordered
    /// by query language (French → FR first, else EN first).
    async fn search_wikipedia(&self, query: &str) -> Result<Vec<SearchResult>> {
        let (fr, en) = tokio::join!(self.wiki_lang("fr", query), self.wiki_lang("en", query));
        let (mut out, rest) = if fr_first(query) {
            (fr.unwrap_or_default(), en.unwrap_or_default())
        } else {
            (en.unwrap_or_default(), fr.unwrap_or_default())
        };
        out.extend(rest);
        Ok(out)
    }

    /// One Wikipedia language: `generator=search` returns titles + intros
    /// in a single call (plain text, 400 chars each, ranked by `index`).
    async fn wiki_lang(&self, lang: &str, query: &str) -> Result<Vec<SearchResult>> {
        let api = format!("https://{lang}.wikipedia.org/w/api.php");
        let url = reqwest::Url::parse_with_params(
            &api,
            [
                ("action", "query"),
                ("generator", "search"),
                ("gsrsearch", query),
                ("gsrlimit", &MAX_RESULTS.to_string()),
                ("prop", "extracts"),
                ("exintro", "1"),
                ("explaintext", "1"),
                ("exchars", "400"),
                ("format", "json"),
            ],
        )
        .context("wikipedia url")?;
        let json = self.fetch_json("wikipedia", self.client.get(url)).await?;
        Ok(parse_wiki_generator(&json, lang))
    }

    /// Full pipeline: cache → search → fetch (skipped when snippets
    /// are rich) → context. Query case preserved (only the cache key is
    /// normalized). Stores the research in the cache (expired swept,
    /// oldest half evicted).
    async fn research(&self, query: &str) -> Result<Research> {
        let asked = query.trim();
        if asked.is_empty() {
            anyhow::bail!("empty query");
        }
        let key = asked.to_lowercase();
        if let Some(hit) = cached_search(&key) {
            return Ok(hit);
        }
        let (backend, found) = self.search(asked).await?;
        let results: Vec<SearchResult> =
            dedup_results(found).into_iter().take(MAX_RESULTS).collect();
        if results.is_empty() {
            anyhow::bail!("no web results");
        }
        let excerpts = self.fetch_excerpts(&results, asked).await;
        let context = build_context(&results, &excerpts);
        let found = Research {
            results,
            context,
            backend,
        };
        store_search(key, &found);
        Ok(found)
    }

    /// Top pages in parallel → best query passages each (order kept).
    /// Skipped (empty) when snippets are already rich. Fetch failures
    /// yield empty excerpts (snippets still ground the synthesis).
    async fn fetch_excerpts(&self, results: &[SearchResult], query: &str) -> Vec<String> {
        if !need_fetch(results) {
            return Vec::new();
        }
        let mut set = JoinSet::new();
        for (i, result) in results.iter().take(MAX_FETCHES).enumerate() {
            let client = self.client.clone();
            let url = result.url.clone();
            set.spawn(async move { (i, fetch_text(&client, &url).await.unwrap_or_default()) });
        }
        let mut indexed = Vec::new();
        while let Some(done) = set.join_next().await {
            indexed.push(done.unwrap_or((usize::MAX, String::new())));
        }
        indexed.sort_by_key(|(i, _)| *i);
        indexed
            .into_iter()
            .map(|(_, text)| pick_passages(&text, query, MAX_PAGE_CHARS))
            .collect()
    }
}

/// Fetches a page as plain text (chunks cut at the cap, HTML/text only,
/// empty content-type accepted). Any failure is an `Err` (fallback).
async fn fetch_text(client: &reqwest::Client, url: &str) -> Result<String> {
    let mut resp = timeout(SEARCH_TIMEOUT, client.get(url).send())
        .await
        .context("fetch timeout")?
        .context("fetch")?;
    if !resp.status().is_success() {
        anyhow::bail!("fetch http {}", resp.status());
    }
    let ctype = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_lowercase();
    let is_text = ["text", "html", "xml"].iter().any(|k| ctype.contains(k));
    if !ctype.is_empty() && !is_text {
        anyhow::bail!("fetch skip {ctype}");
    }
    let mut body = Vec::new();
    while body.len() < MAX_BODY_BYTES {
        let chunk = timeout(SEARCH_TIMEOUT, resp.chunk())
            .await
            .context("fetch body timeout")?
            .context("fetch body")?;
        let Some(chunk) = chunk else { break };
        let room = MAX_BODY_BYTES - body.len();
        body.extend_from_slice(&chunk[..chunk.len().min(room)]);
    }
    let text = strip_html(&String::from_utf8_lossy(&body));
    Ok(crate::provider::truncate(&text, MAX_PAGE_CHARS))
}

/// Synthesis prompt: grounded, cited, contradiction-aware.
fn synthesis_messages(query: &str, context: &str) -> Vec<(String, String)> {
    let system = "You are a web research assistant. Answer in the same language as the query. \
        Use ONLY the provided results, never outside knowledge. Every factual claim ends \
        with its [n] citation. If sources contradict each other, say so. Stay concise.";
    vec![
        (String::from("system"), system.to_string()),
        (
            String::from("user"),
            format!("Query: {query}\n\nWeb results:\n{context}"),
        ),
    ]
}

/// Raw display (no LLM): header + numbered sources with snippets.
fn raw_text(head: &str, results: &[SearchResult]) -> String {
    let mut text = head.to_string();
    for (i, result) in results.iter().enumerate() {
        text.push_str(&format!("\n[{}] {} — {}", i + 1, result.title, result.url));
        if !result.snippet.is_empty() {
            text.push_str(&format!("\n{}", result.snippet));
        }
    }
    text
}

/// Query → researched answer: LLM synthesis + `Sources:` footer, or raw
/// results when there is no provider (or when synthesis itself fails).
pub async fn answer(
    query: String,
    provider: Option<&Provider>,
    model: &str,
    effort: Option<&str>,
) -> Result<WebAnswer> {
    let client = WebClient::from_env();
    let found = timeout(RESEARCH_DEADLINE, client.research(&query))
        .await
        .context("research deadline")??;
    let mut sources = String::from("\n\nSources:");
    for (i, result) in found.results.iter().enumerate() {
        sources.push_str(&format!("\n[{}] {} — {}", i + 1, result.title, result.url));
    }
    let Some(llm) = provider else {
        let head = format!("top {} web results for \"{query}\":", found.results.len());
        return Ok(WebAnswer {
            text: raw_text(&head, &found.results),
            backend: found.backend,
            prompt_tokens: 0,
            completion_tokens: 0,
        });
    };
    let messages = synthesis_messages(&query, &found.context);
    match llm.complete_full(&messages, model, effort).await {
        Ok(done) => Ok(WebAnswer {
            text: format!("{}{sources}", done.text),
            backend: found.backend,
            prompt_tokens: done.prompt_tokens,
            completion_tokens: done.completion_tokens,
        }),
        Err(_) => {
            let head = format!("synthesis failed, raw results for \"{query}\":");
            Ok(WebAnswer {
                text: raw_text(&head, &found.results),
                backend: found.backend,
                prompt_tokens: 0,
                completion_tokens: 0,
            })
        }
    }
}

/// Shared HTTP client: one pool (TLS + connections reused across `/web`).
static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddg_cooldown_arms_and_covers() {
        assert!(!ddg_cooling());
        note_ddg_challenge();
        assert!(ddg_cooling());
    }

    #[test]
    fn synthesis_messages_frame_query() {
        let msgs = synthesis_messages("rust async?", "[1] Tokio — https://tokio.rs/");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].0, "system");
        assert!(msgs[0].1.contains("[n]"));
        assert!(msgs[0].1.contains("ONLY"));
        assert_eq!(msgs[1].0, "user");
        assert!(msgs[1].1.contains("rust async?"));
        assert!(msgs[1].1.contains("https://tokio.rs/"));
    }

    #[tokio::test]
    #[ignore = "live network: real backends (DDG may bot-block, Wikipedia answers)"]
    async fn live_research_returns_sources() {
        let client = WebClient::from_env();
        let found = client
            .research("Tokyo Rose radio")
            .await
            .expect("live search");
        assert!(!found.results.is_empty());
        assert!(!found.context.is_empty());
        assert!(found.results.iter().all(|r| r.url.starts_with("https")));
        let cached = client.research("tokyo rose radio").await.expect("cached");
        assert_eq!(cached.backend, found.backend);
        assert_eq!(cached.results.len(), found.results.len());
    }
}
