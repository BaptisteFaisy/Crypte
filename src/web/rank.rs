//! Pertinence `/web` : passages, canonique, budgets, langue (pur, testé).
//!
//! Façonne les résultats bruts en contexte LLM : dédup par URL canonique,
//! passages les plus pertinents par page, budgets équitables par source,
//! décision de fetch, ordre FR/EN. Le réseau vit dans `super`.

use super::{SearchResult, MAX_CONTEXT_CHARS, MAX_FETCHES};

/// Snippets cumulés (top fetch) sous ce seuil → les pages sont fetchées.
/// Au-dessus, les snippets suffisent (Tavily, Brave, intros Wikipedia).
pub(super) const NEED_FETCH_SNIPPETS: usize = 1200;

/// Vrai si les snippets sont trop minces : il faut fetcher les pages.
pub(super) fn need_fetch(results: &[SearchResult]) -> bool {
    results
        .iter()
        .take(MAX_FETCHES)
        .map(|r| r.snippet.len())
        .sum::<usize>()
        < NEED_FETCH_SNIPPETS
}

/// URL canonique pour la dédup : host minuscule sans `www.`/`m.`,
/// fragment et query de tracking virés, `/` final sauf à la racine.
/// Inparsable → minuscules brutes (jamais de panique).
fn canonical_url(url: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(url) else {
        return url.trim().to_lowercase();
    };
    if parsed.scheme() == "http" {
        let _ = parsed.set_scheme("https");
    }
    if let Some(host) = parsed.host_str().map(str::to_lowercase) {
        let bare = host
            .strip_prefix("www.")
            .or_else(|| host.strip_prefix("m."))
            .unwrap_or(&host);
        let _ = parsed.set_host(Some(bare));
    }
    parsed.set_fragment(None);
    let is_tracking =
        |k: &str| k == "fbclid" || k == "gclid" || k == "msclkid" || k.starts_with("utm_");
    if parsed.query().is_some() {
        let kept: Vec<(String, String)> = parsed
            .query_pairs()
            .filter(|(k, _)| !is_tracking(k))
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        if kept.is_empty() {
            parsed.set_query(None);
        } else {
            parsed
                .query_pairs_mut()
                .clear()
                .extend_pairs(kept.iter().map(|(k, v)| (k, v)));
        }
    }
    if parsed.path().ends_with('/') && parsed.path().len() > 1 {
        let trimmed = parsed.path().trim_end_matches('/').to_string();
        parsed.set_path(&trimmed);
    }
    parsed.to_string()
}

/// Dédup par URL canonique (première occurrence, URL d'origine gardée).
pub(super) fn dedup_results(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut seen = std::collections::HashSet::new();
    results
        .into_iter()
        .filter(|r| !r.url.is_empty() && seen.insert(canonical_url(&r.url)))
        .collect()
}

/// Contexte à budgets équitables : un bloc `[n] titre — url` + snippet +
/// excerpt par source, chacun plafonné au 1/n du budget (en-têtes jamais
/// coupés puisque la coupe tombe en queue de bloc, aucune source affamée).
pub(super) fn build_context(results: &[SearchResult], excerpts: &[String]) -> String {
    if results.is_empty() {
        return String::new();
    }
    let share = MAX_CONTEXT_CHARS / results.len();
    results
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut block = format!("[{}] {} — {}\n", i + 1, r.title, r.url);
            if !r.snippet.is_empty() {
                block.push_str(&r.snippet);
                block.push('\n');
            }
            if let Some(ex) = excerpts.get(i) {
                if !ex.is_empty() {
                    block.push_str(ex);
                    block.push('\n');
                }
            }
            crate::provider::truncate(&block, share)
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Passages les plus pertinents : phrases scorées par mots distincts de
/// la requête (mots >2 lettres), meilleures jusqu'au budget (ordre stable).
/// Aucun match → tête du texte (repli, jamais vide si texte non vide).
pub(super) fn pick_passages(text: &str, query: &str, budget: usize) -> String {
    let mut words: Vec<String> = query
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .filter(|w| w.chars().count() > 2)
        .collect();
    words.sort();
    words.dedup();
    let score = |s: &str| {
        let low = s.to_lowercase();
        words.iter().filter(|w| low.contains(w.as_str())).count()
    };
    let mut scored: Vec<(usize, &str)> = text
        .split_inclusive(['.', '!', '?'])
        .map(|s| (score(s), s.trim()))
        .filter(|(_, s)| !s.is_empty())
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out = String::new();
    for (points, sentence) in scored {
        if points == 0 || out.chars().count() + sentence.chars().count() + 1 > budget {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(sentence);
    }
    if out.is_empty() {
        return crate::provider::truncate(text, budget);
    }
    out
}

/// Requête française ? (accents ou mots-outils) → Wikipedia FR d'abord,
/// sinon EN d'abord. Heuristique fruste, repli sûr dans les deux cas.
pub(super) fn fr_first(query: &str) -> bool {
    const ACCENTS: &str = "éèêëàâäùûüîïôöçœæ";
    const MARKERS: &[&str] = &[
        " le ", " la ", " les ", " des ", " une ", "comment", "pourquoi", "quel", "quelle",
        "combien", "quoi",
    ];
    let padded = format!(" {} ", query.to_lowercase());
    padded.chars().any(|c| ACCENTS.contains(c)) || MARKERS.iter().any(|m| padded.contains(m))
}

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
    fn canonical_collapses_tracking_variants() {
        let base = canonical_url("https://example.com/Page?a=1");
        assert_eq!(
            canonical_url("http://www.example.com/Page/?a=1&utm_source=x#frag"),
            base
        );
        assert_eq!(
            canonical_url("https://m.example.com/Page?fbclid=ab&a=1"),
            base
        );
        assert_ne!(canonical_url("https://example.com/Other?a=1"), base);
        assert_ne!(canonical_url("https://example.com/Page?a=2"), base);
        assert_eq!(canonical_url("not a url"), "not a url");
        assert_eq!(
            canonical_url("https://example.com/"),
            "https://example.com/"
        );
    }

    #[test]
    fn dedup_uses_canonical_form() {
        let results = vec![
            result("One", "https://a.example/p?utm_x=1", "first"),
            result("One dup", "http://www.a.example/p/", "dropped"),
            result("Two", "https://b.example/", "second"),
        ];
        let deduped = dedup_results(results);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].url, "https://a.example/p?utm_x=1");
    }

    #[test]
    fn context_is_fair_and_bounded() {
        let big = "x".repeat(5000);
        let results = vec![
            result("One", "https://a.example/", &big),
            result("Two", "https://b.example/", "s2"),
            result("Three", "https://c.example/", "s3"),
            result("Four", "https://d.example/", "s4"),
            result("Five", "https://e.example/", "s5"),
        ];
        let ctx = build_context(&results, &[]);
        for n in 1..=5 {
            assert!(ctx.contains(&format!("[{n}]")), "header [{n}] kept");
        }
        assert!(
            ctx.contains("https://e.example/"),
            "last source not starved"
        );
        assert!(ctx.chars().count() <= MAX_CONTEXT_CHARS);
        assert_eq!(build_context(&[], &[]), "");
    }

    #[test]
    fn passages_prefer_query_sentences() {
        let page = "Home Products Contact Legal cookies policy. \
            Tokio is an async runtime for the Rust language. \
            It powers fast networking services with work-stealing schedulers.";
        let picked = pick_passages(page, "tokio async runtime", 200);
        assert!(picked.contains("async runtime"), "answer kept: {picked}");
        assert!(!picked.contains("cookies"), "nav dropped: {picked}");
        assert!(picked.chars().count() <= 200);
        assert_eq!(pick_passages("", "tokio", 200), "");
        assert_eq!(
            pick_passages("Just a header.", "zzz qqq", 200),
            "Just a header."
        );
    }

    #[test]
    fn need_fetch_threshold() {
        let rich = vec![result("a", "https://a.example/", &"s".repeat(500)); 3];
        assert!(!need_fetch(&rich));
        let poor = vec![result("a", "https://a.example/", "short"); 3];
        assert!(need_fetch(&poor));
        assert!(need_fetch(&[]));
    }

    #[test]
    fn fr_first_detects_french() {
        assert!(fr_first("comment câbler un moteur ?"));
        assert!(fr_first("pourquoi le ciel est bleu"));
        assert!(!fr_first("how to wire a motor"));
        assert!(!fr_first("tokyo rose radio"));
    }
}
