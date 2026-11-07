//! Web search for `/web`: backends, page fetch, cache, synthesis input.
//!
//! Chain (first non-empty wins): Tavily → Brave → DuckDuckGo → Wikipedia
//! (FR+EN). 8 s per backend, top-3 pages in parallel, char budgets,
//! 10 min cache. Keys never logged. Pure parsing lives in [`parse`].

mod parse;

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::provider::Provider;

use self::parse::{parse_ddg_html, parse_json_results, parse_wikipedia, strip_html, wiki_hits};

/// Timeout per backend call and per page fetch.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(8);
/// Results kept per search (also the numbered `[n]` source count).
pub(super) const MAX_RESULTS: usize = 5;
/// Pages fetched in parallel to enrich the context.
const MAX_FETCHES: usize = 3;
/// Chars kept per snippet.
pub(super) const MAX_SNIPPET: usize = 500;
/// Chars kept per fetched page (after HTML strip).
const MAX_PAGE_CHARS: usize = 2000;
/// Total chars of the LLM context block.
const MAX_CONTEXT_CHARS: usize = 8000;
/// Bytes read per page before stripping (HTML head is enough).
const MAX_BODY_BYTES: usize = 64 * 1024;
/// Search cache TTL (query → full research, pages included).
const CACHE_TTL: Duration = Duration::from_secs(600);
/// Search cache capacity (oldest half evicted when full).
const CACHE_CAP: usize = 50;
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
        let client = reqwest::Client::builder()
            .timeout(SEARCH_TIMEOUT)
            .user_agent(BROWSER_UA)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client,
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
        if let Ok(found) = self.search_ddg(query).await {
            if !found.is_empty() {
                return Ok(("duckduckgo", found));
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

    /// DuckDuckGo HTML (free; an anti-bot challenge page parses to
    /// zero results, which falls through to Wikipedia).
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
        Ok(parse_ddg_html(&html))
    }

    /// Wikipedia FR + EN in parallel (free, never bot-blocked).
    async fn search_wikipedia(&self, query: &str) -> Result<Vec<SearchResult>> {
        let (fr, en) = tokio::join!(self.wiki_lang("fr", query), self.wiki_lang("en", query));
        let mut out = fr.unwrap_or_default();
        out.extend(en.unwrap_or_default());
        Ok(out)
    }

    /// One Wikipedia language: full-text search (titles + URLs) then a
    /// single `extracts` call for all intros (plain text, 400 chars each).
    async fn wiki_lang(&self, lang: &str, query: &str) -> Result<Vec<SearchResult>> {
        let api = format!("https://{lang}.wikipedia.org/w/api.php");
        let s_url = reqwest::Url::parse_with_params(
            &api,
            [
                ("action", "query"),
                ("list", "search"),
                ("srsearch", query),
                ("srlimit", &MAX_RESULTS.to_string()),
                ("format", "json"),
            ],
        )
        .context("wikipedia url")?;
        let found = self.fetch_json("wikipedia", self.client.get(s_url)).await?;
        let (titles, os) = wiki_hits(&found, lang);
        if titles.is_empty() {
            return Ok(Vec::new());
        }
        let ex_url = reqwest::Url::parse_with_params(
            &api,
            [
                ("action", "query"),
                ("prop", "extracts"),
                ("exintro", "1"),
                ("explaintext", "1"),
                ("exchars", "400"),
                ("titles", &titles.join("|")),
                ("format", "json"),
            ],
        )
        .context("wikipedia url")?;
        let ex = self
            .fetch_json("wikipedia", self.client.get(ex_url))
            .await?;
        Ok(parse_wikipedia(&os, &ex))
    }

    /// Full pipeline: cache → search → parallel top-3 fetch → context.
    /// Stores the research in the cache (expired swept, oldest half evicted).
    async fn research(&self, query: &str) -> Result<Research> {
        let key = query.trim().to_lowercase();
        if key.is_empty() {
            anyhow::bail!("empty query");
        }
        if let Some(hit) = cached_search(&key) {
            return Ok(hit);
        }
        let (backend, found) = self.search(&key).await?;
        let results: Vec<SearchResult> =
            dedup_results(found).into_iter().take(MAX_RESULTS).collect();
        if results.is_empty() {
            anyhow::bail!("no web results");
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
        let excerpts: Vec<String> = indexed.into_iter().map(|(_, text)| text).collect();
        let context = build_context(&results, &excerpts);
        let found = Research {
            results,
            context,
            backend,
        };
        if let Ok(mut guard) = SEARCH_CACHE
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
        {
            guard.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
            if guard.len() >= CACHE_CAP {
                let mut old: Vec<(Instant, String)> =
                    guard.iter().map(|(k, (at, _))| (*at, k.clone())).collect();
                old.sort();
                for (_, k) in old.into_iter().take(CACHE_CAP / 2) {
                    guard.remove(&k);
                }
            }
            guard.insert(key, (Instant::now(), found.clone()));
        }
        Ok(found)
    }
}

/// Fetches a page as plain text (HTML/text only, stripped, budgeted).
/// Any failure is an `Err` (the caller falls back to the snippet).
async fn fetch_text(client: &reqwest::Client, url: &str) -> Result<String> {
    let resp = timeout(SEARCH_TIMEOUT, client.get(url).send())
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
    if !(ctype.contains("text") || ctype.contains("html") || ctype.contains("xml")) {
        anyhow::bail!("fetch skip {ctype}");
    }
    let bytes = timeout(SEARCH_TIMEOUT, resp.bytes())
        .await
        .context("fetch body timeout")?
        .context("fetch body")?;
    let capped = &bytes[..bytes.len().min(MAX_BODY_BYTES)];
    let text = strip_html(&String::from_utf8_lossy(capped));
    Ok(crate::provider::truncate(&text, MAX_PAGE_CHARS))
}

/// Drops results with an empty/duplicate URL (first occurrence wins).
fn dedup_results(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut seen = std::collections::HashSet::new();
    results
        .into_iter()
        .filter(|r| !r.url.is_empty() && seen.insert(r.url.clone()))
        .collect()
}

/// Numbered LLM context: `[n] title — url`, snippet, page excerpt.
fn build_context(results: &[SearchResult], excerpts: &[String]) -> String {
    let mut ctx = String::new();
    for (i, result) in results.iter().enumerate() {
        ctx.push_str(&format!("[{}] {} — {}\n", i + 1, result.title, result.url));
        if !result.snippet.is_empty() {
            ctx.push_str(&format!("{}\n", result.snippet));
        }
        if let Some(excerpt) = excerpts.get(i) {
            if !excerpt.is_empty() {
                ctx.push_str(&format!("{excerpt}\n"));
            }
        }
    }
    crate::provider::truncate(&ctx, MAX_CONTEXT_CHARS)
}

/// Synthesis prompt: same language as the query, factual, cited `[n]`.
fn synthesis_messages(query: &str, context: &str) -> Vec<(String, String)> {
    let system = "You are a web research assistant. Answer in the same language as the query. \
        Be factual and concise (10 lines max). Cite sources as [n]. \
        If the results are insufficient, say so plainly.";
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

/// Cache hit (normalized query → full research), `None` if absent/expired.
fn cached_search(key: &str) -> Option<Research> {
    let cache = SEARCH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let guard = cache.lock().ok()?;
    let (at, found) = guard.get(key)?;
    (at.elapsed() < CACHE_TTL).then(|| found.clone())
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
    let found = client.research(&query).await?;
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

/// Query cache: normalized query → (insertion time, full research).
static SEARCH_CACHE: OnceLock<Mutex<HashMap<String, (Instant, Research)>>> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    fn result(title: &str, url: &str, snippet: &str) -> SearchResult {
        SearchResult {
            title: title.to_string(),
            url: url.to_string(),
            snippet: snippet.to_string(),
        }
    }

    #[test]
    fn context_dedups_numbers_and_truncates() {
        let results = vec![
            result("One", "https://a.example/", "first"),
            result("One dup", "https://a.example/", "dropped"),
            result("Empty", "", "dropped too"),
            result("Two", "https://b.example/", "second"),
        ];
        let deduped = dedup_results(results);
        assert_eq!(deduped.len(), 2);
        let ctx = build_context(&deduped, &[String::from("excerpt one"), String::new()]);
        assert!(ctx.starts_with("[1] One — https://a.example/"));
        assert!(ctx.contains("excerpt one"));
        assert!(ctx.contains("[2] Two — https://b.example/"));
        assert!(ctx.chars().count() <= MAX_CONTEXT_CHARS);
    }

    #[test]
    fn synthesis_messages_frame_query() {
        let msgs = synthesis_messages("rust async?", "[1] Tokio — https://tokio.rs/");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].0, "system");
        assert!(msgs[0].1.contains("[n]"));
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
