//! Parsing web (`/web`) : HTML DuckDuckGo, JSON Tavily/Brave,
//! JSON Wikipedia, HTML → texte, entités nommées latin-1.
//!
//! Que du pur + synchrone (testé offline) ; le réseau vit dans
//! `super` (client, backends, fetch).

use super::{SearchResult, MAX_RESULTS, MAX_SNIPPET};

/// Entités nommées fréquentes (extraits FR/EN) : `&eacute;` → `é`.
/// Le reste passe par le décodage numérique ou est gardé verbatim.
const NAMED_ENTITIES: &[(&str, char)] = &[
    ("eacute", 'é'),
    ("egrave", 'è'),
    ("ecirc", 'ê'),
    ("euml", 'ë'),
    ("agrave", 'à'),
    ("aacute", 'á'),
    ("acirc", 'â'),
    ("auml", 'ä'),
    ("ugrave", 'ù'),
    ("uacute", 'ú'),
    ("ucirc", 'û'),
    ("uuml", 'ü'),
    ("igrave", 'ì'),
    ("iacute", 'í'),
    ("icirc", 'î'),
    ("iuml", 'ï'),
    ("ocirc", 'ô'),
    ("ouml", 'ö'),
    ("ccedil", 'ç'),
    ("ntilde", 'ñ'),
    ("Eacute", 'É'),
    ("Egrave", 'È'),
    ("Agrave", 'À'),
    ("Ccedil", 'Ç'),
    ("oelig", 'œ'),
    ("OElig", 'Œ'),
    ("aelig", 'æ'),
    ("oslash", 'ø'),
    ("szlig", 'ß'),
    ("yuml", 'ÿ'),
    ("euro", '€'),
    ("copy", '©'),
    ("reg", '®'),
    ("sect", '§'),
    ("deg", '°'),
    ("hellip", '…'),
    ("ndash", '–'),
    ("mdash", '—'),
    ("laquo", '«'),
    ("raquo", '»'),
    ("middot", '·'),
    ("plusmn", '±'),
    ("times", '×'),
    ("divide", '÷'),
];

/// `title` / `url` / `text_field` items → results (empty URLs dropped).
pub(super) fn parse_json_results(
    items: &[serde_json::Value],
    text_field: &str,
) -> Vec<SearchResult> {
    items
        .iter()
        .filter_map(|item| {
            let url = item.get("url")?.as_str().unwrap_or_default();
            if url.is_empty() {
                return None;
            }
            let title = item.get("title")?.as_str().unwrap_or_default();
            let snippet = item.get(text_field)?.as_str().unwrap_or_default();
            Some(SearchResult {
                title: strip_html(title),
                url: url.to_string(),
                snippet: crate::provider::truncate(&strip_html(snippet), MAX_SNIPPET),
            })
        })
        .take(MAX_RESULTS)
        .collect()
}

/// DuckDuckGo HTML: `<a class="result__a" href="…">title</a>` then the
/// nearest `result__snippet` before the next result (else empty snippet).
pub(super) fn parse_ddg_html(html: &str) -> Vec<SearchResult> {
    let mut out = Vec::new();
    let mut pos = 0;
    while out.len() < MAX_RESULTS {
        let Some(rel) = html[pos..].find("<a") else {
            break;
        };
        let start = pos + rel;
        let Some(end) = html[start..].find('>') else {
            break;
        };
        let tag = &html[start..start + end];
        pos = start + 2;
        if !tag.contains("result__a") {
            continue;
        }
        let Some(href) = tag.find("href=\"").and_then(|i| {
            let rest = &tag[i + 6..];
            rest.find('"').map(|j| rest[..j].to_string())
        }) else {
            continue;
        };
        let body = start + end + 1;
        let Some(close) = html[body..].find("</a>") else {
            continue;
        };
        let title = strip_html(&html[body..body + close]);
        if title.is_empty() {
            continue;
        }
        let url = unwrap_ddg_url(&href);
        if url.is_empty() {
            continue;
        }
        let after = body + close + 4;
        let snippet = ddg_snippet(html, after);
        out.push(SearchResult {
            title,
            url,
            snippet: crate::provider::truncate(&snippet, MAX_SNIPPET),
        });
    }
    out
}

/// Snippet after a DDG title link: nearest `result__snippet` before the
/// next result, else empty.
fn ddg_snippet(html: &str, after: usize) -> String {
    html[after..]
        .find("result__snippet")
        .and_then(|r| {
            let found = after + r;
            if html[after..found].contains("result__a") {
                return None;
            }
            let gt = html[found..].find('>')?;
            let inner = found + gt + 1;
            let stop = html[inner..].find("</a>")?;
            Some(strip_html(&html[inner..inner + stop]))
        })
        .unwrap_or_default()
}

/// Merges opensearch `[query, titles, descs, urls]` with the `extracts`
/// intros (extract wins, opensearch description is the fallback).
pub(super) fn parse_wikipedia(
    opensearch: &serde_json::Value,
    extracts: &serde_json::Value,
) -> Vec<SearchResult> {
    let empty = Vec::new();
    let titles = opensearch
        .get(1)
        .and_then(|v| v.as_array())
        .unwrap_or(&empty);
    let descs = opensearch
        .get(2)
        .and_then(|v| v.as_array())
        .unwrap_or(&empty);
    let urls = opensearch
        .get(3)
        .and_then(|v| v.as_array())
        .unwrap_or(&empty);
    let intro = |title: &str| {
        extracts
            .pointer("/query/pages")
            .and_then(|v| v.as_object())
            .and_then(|pages| {
                pages
                    .values()
                    .find(|p| p.get("title").and_then(|t| t.as_str()) == Some(title))
            })
            .and_then(|p| p.get("extract"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
    };
    titles
        .iter()
        .enumerate()
        .filter_map(|(i, value)| {
            let title = value.as_str().unwrap_or_default();
            if title.is_empty() {
                return None;
            }
            let url = urls.get(i).and_then(|u| u.as_str()).unwrap_or_default();
            let extract = intro(title);
            let desc = descs.get(i).and_then(|d| d.as_str()).unwrap_or_default();
            let snippet = if extract.is_empty() { desc } else { extract };
            Some(SearchResult {
                title: title.to_string(),
                url: url.to_string(),
                snippet: crate::provider::truncate(snippet, MAX_SNIPPET),
            })
        })
        .take(MAX_RESULTS)
        .collect()
}

/// Unwraps `//duckduckgo.com/l/?uddg=<urlencoded>` (percent-decoded),
/// keeps direct links, drops anything that is not http(s).
pub(super) fn unwrap_ddg_url(href: &str) -> String {
    let raw = match href.find("uddg=") {
        Some(i) => {
            let rest = &href[i + 5..];
            let end = rest
                .find(&['&', '"', '\''][..])
                .map(|e| &rest[..e])
                .unwrap_or(rest);
            let mut buf: Vec<u8> = Vec::with_capacity(end.len());
            let mut bytes = end.as_bytes().iter();
            while let Some(&b) = bytes.next() {
                if b == b'%' {
                    let hi = bytes.next().copied().unwrap_or(b'0');
                    let lo = bytes.next().copied().unwrap_or(b'0');
                    let hex = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
                    buf.push(hex(hi) * 16 + hex(lo));
                } else if b == b'+' {
                    buf.push(b' ');
                } else {
                    buf.push(b);
                }
            }
            String::from_utf8_lossy(&buf).into_owned()
        }
        None => href.to_string(),
    };
    if raw.starts_with("http://") || raw.starts_with("https://") {
        raw
    } else {
        String::new()
    }
}

/// Full-text hits → `(titles, opensearch-shaped value)`: page URLs built
/// (`/wiki/Title_With_Underscores`, encoded), snippets stripped.
pub(super) fn wiki_hits(found: &serde_json::Value, lang: &str) -> (Vec<String>, serde_json::Value) {
    let empty = Vec::new();
    let hits = found
        .pointer("/query/search")
        .and_then(|v| v.as_array())
        .unwrap_or(&empty);
    let base = format!("https://{lang}.wikipedia.org/wiki/");
    let mut titles = Vec::new();
    let mut names = Vec::new();
    let mut descs = Vec::new();
    let mut urls = Vec::new();
    for hit in hits {
        let Some(title) = hit.get("title").and_then(|v| v.as_str()) else {
            continue;
        };
        titles.push(title.to_string());
        names.push(serde_json::Value::String(title.to_string()));
        let raw = hit
            .get("snippet")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        descs.push(serde_json::Value::String(strip_html(raw)));
        urls.push(serde_json::Value::String(wiki_url(&base, title)));
    }
    (titles, serde_json::json!(["", names, descs, urls]))
}

/// Page URL for a Wikipedia title (spaces → `_`, rest encoded).
fn wiki_url(base: &str, title: &str) -> String {
    reqwest::Url::parse(base)
        .ok()
        .and_then(|mut url| {
            url.path_segments_mut().ok()?.push(&title.replace(' ', "_"));
            Some(url.to_string())
        })
        .unwrap_or_default()
}

/// HTML → plain text: tags dropped (a space each, no glued words),
/// scripts/styles/comments skipped, entities decoded, spaces collapsed.
/// Inline tags (`<b>`, `</a>`, …) : no separating space when dropped.
const INLINE_TAGS: &str = "|a|b|i|em|strong|span|code|abbr|q|sub|sup|u|small|mark|";

/// True if `tag` (from `<`) is an inline element (block tags separate).
fn is_inline_tag(tag: &str) -> bool {
    let rest = tag.strip_prefix('<').unwrap_or(tag);
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    let len = rest.bytes().take_while(u8::is_ascii_alphanumeric).count();
    !rest[..len].is_empty() && INLINE_TAGS.contains(&format!("|{}|", &rest[..len]))
}

pub(super) fn strip_html(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len().min(4096));
    let mut i = 0;
    while i < html.len() {
        if lower[i..].starts_with("<script") || lower[i..].starts_with("<style") {
            let needle = if lower[i..].starts_with("<script") {
                "</script>"
            } else {
                "</style>"
            };
            i = lower[i..]
                .find(needle)
                .map(|j| i + j + needle.len())
                .unwrap_or(html.len());
            out.push(' ');
        } else if html.as_bytes()[i] == b'<' {
            let rest = &html[i..];
            let end = if rest.starts_with("<!--") {
                rest.find("-->").map(|j| i + j + 3)
            } else {
                rest.find('>').map(|j| i + j + 1)
            };
            if !is_inline_tag(rest) {
                out.push(' ');
            }
            i = end.unwrap_or(html.len());
        } else if html.as_bytes()[i] == b'&' {
            let end = html[i..]
                .find(';')
                .filter(|&j| j <= 10)
                .map(|j| i + j + 1)
                .unwrap_or(i + 1);
            decode_entity(&html[i..end], &mut out);
            i = end;
        } else {
            let ch = html[i..].chars().next().unwrap_or(' ');
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Appends the decoded entity to `out`: XML basics, latin-1 table,
/// `&#123;` numerics ; unknown entities kept verbatim, never dropped.
fn decode_entity(entity: &str, out: &mut String) {
    match entity {
        "&amp;" => out.push('&'),
        "&lt;" => out.push('<'),
        "&gt;" => out.push('>'),
        "&quot;" => out.push('"'),
        "&nbsp;" | "&#160;" => out.push(' '),
        "&#39;" | "&apos;" => out.push('\''),
        _ => {
            let name = entity
                .strip_prefix('&')
                .and_then(|s| s.strip_suffix(';'))
                .unwrap_or("");
            if let Some((_, ch)) = NAMED_ENTITIES.iter().find(|(n, _)| *n == name) {
                out.push(*ch);
            } else {
                match name
                    .strip_prefix('#')
                    .and_then(|n| n.parse::<u32>().ok())
                    .and_then(char::from_u32)
                {
                    Some(ch) => out.push(ch),
                    None => out.push_str(entity),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_html_drops_tags_scripts_entities() {
        assert_eq!(strip_html("<p>Hello <b>world</b></p>"), "Hello world");
        assert_eq!(
            strip_html("a<script>var x = 1 < 2;</script>b<STYLE>p{}</STYLE>c"),
            "a b c"
        );
        assert_eq!(strip_html("<!-- hidden -->visible"), "visible");
        assert_eq!(
            strip_html("fish &amp; chips &#39;yum&#39; &#8364;"),
            "fish & chips 'yum' €"
        );
        assert_eq!(
            strip_html("<div>glued</div><div>words</div>"),
            "glued words"
        );
        assert_eq!(strip_html("caf&eacute; &unknown;"), "café &unknown;");
    }

    #[test]
    fn unwrap_ddg_url_decodes_and_filters() {
        assert_eq!(
            unwrap_ddg_url("//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa+b&rut=xx"),
            "https://example.com/a b"
        );
        assert_eq!(
            unwrap_ddg_url("https://example.com/direct"),
            "https://example.com/direct"
        );
        assert_eq!(unwrap_ddg_url("/relative/path"), "");
        assert_eq!(unwrap_ddg_url("javascript:alert(1)"), "");
    }

    #[test]
    fn parse_ddg_html_unwraps_and_snippets() {
        let html = r#"
            <div class="result">
              <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Ftokio.rs%2F&rut=1">Tokio &amp; async</a>
              <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Ftokio.rs%2F">An async <b>runtime</b> for Rust.</a>
            </div>
            <div class="result">
              <a class="result__a" href="https://example.com/plain">Plain result</a>
            </div>
            <div class="result">
              <a class="result__a" href="/internal">Skipped relative</a>
            </div>"#;
        let found = parse_ddg_html(html);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].title, "Tokio & async");
        assert_eq!(found[0].url, "https://tokio.rs/");
        assert_eq!(found[0].snippet, "An async runtime for Rust.");
        assert_eq!(found[1].url, "https://example.com/plain");
        assert_eq!(found[1].snippet, "");
        // Anti-bot challenge page → zero results (falls through to Wikipedia).
        assert!(parse_ddg_html("<html><body>anomaly challenge</body></html>").is_empty());
    }

    #[test]
    fn parse_json_results_brave_and_tavily() {
        let brave = serde_json::json!({
            "web": {"results": [
                {"title": "B <i>one</i>", "url": "https://b.example/1", "description": "desc one"},
                {"title": "no url", "url": "", "description": "dropped"},
            ]}
        });
        let items = brave
            .pointer("/web/results")
            .and_then(|v| v.as_array())
            .unwrap();
        let found = parse_json_results(items, "description");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "B one");
        assert_eq!(found[0].snippet, "desc one");
        let tavily = serde_json::json!({"results": [
            {"title": "T", "url": "https://t.example/", "content": "long content here"},
        ]});
        let items = tavily.get("results").and_then(|v| v.as_array()).unwrap();
        let found = parse_json_results(items, "content");
        assert_eq!(found[0].snippet, "long content here");
    }

    #[test]
    fn wiki_hits_builds_urls_and_strips_snippets() {
        let found = serde_json::json!({"query": {"search": [
            {"title": "C++", "snippet": "A <b>language</b>."},
            {"title": "Theorie du chaos", "snippet": ""},
            {"nope": true},
        ]}});
        let (titles, os) = wiki_hits(&found, "fr");
        assert_eq!(titles, ["C++", "Theorie du chaos"]);
        let url = |i: usize| {
            os.get(3)
                .and_then(|u| u.get(i))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
        };
        assert!(url(0).starts_with("https://fr.wikipedia.org/wiki/"));
        assert!(!url(0).contains(' '));
        assert!(url(1).ends_with("Theorie_du_chaos"));
        let desc = os.get(2).and_then(|d| d.get(0)).and_then(|v| v.as_str());
        assert_eq!(desc, Some("A language."));
    }

    #[test]
    fn parse_wikipedia_prefers_extracts() {
        let os = serde_json::json!([
            "tokio",
            ["Tokio (software)", "Tokyo Rose"],
            ["os desc", ""],
            [
                "https://en.wikipedia.org/wiki/Tokio",
                "https://en.wikipedia.org/wiki/Rose"
            ],
        ]);
        let ex = serde_json::json!({"query": {"pages": {
            "1": {"title": "Tokio (software)", "extract": ""},
            "2": {"title": "Tokyo Rose", "extract": "Allied nickname for broadcasters."},
        }}});
        let found = parse_wikipedia(&os, &ex);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].snippet, "os desc");
        assert_eq!(found[1].snippet, "Allied nickname for broadcasters.");
        assert_eq!(found[1].url, "https://en.wikipedia.org/wiki/Rose");
    }
}
