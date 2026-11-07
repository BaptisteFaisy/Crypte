//! `/web` caches: search results (TTL + bounded) and DDG cooldown.
//!
//! Both are process-wide (queries repeat across chats) and small:
//! 50 researches max, one timestamp for DDG.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::Research;

/// Search cache TTL (query → full research, pages included).
const CACHE_TTL: Duration = Duration::from_secs(600);
/// Search cache capacity (oldest half evicted when full).
const CACHE_CAP: usize = 50;
/// DuckDuckGo skip window after an anti-bot challenge page.
const DDG_COOLDOWN: Duration = Duration::from_secs(600);

/// Query cache: normalized query → (insertion time, full research).
static SEARCH_CACHE: OnceLock<Mutex<HashMap<String, (Instant, Research)>>> = OnceLock::new();

/// Last DuckDuckGo anti-bot challenge (skip DDG while cooling down).
static DDG_BLOCKED: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

/// Cache hit (normalized query → full research), `None` if absent/expired.
pub(super) fn cached_search(key: &str) -> Option<Research> {
    let cache = SEARCH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let guard = cache.lock().ok()?;
    let (at, found) = guard.get(key)?;
    (at.elapsed() < CACHE_TTL).then(|| found.clone())
}

/// Stores a research (expired swept, oldest half evicted when full).
pub(super) fn store_search(key: String, found: &Research) {
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
}

/// Vrai si un challenge DDG récent impose de sauter ce backend.
pub(super) fn ddg_cooling() -> bool {
    DDG_BLOCKED
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .is_some_and(|at| at.elapsed() < DDG_COOLDOWN)
}

/// Mémorise un challenge anti-bot (prochains `/web` sautent DDG).
pub(super) fn note_ddg_challenge() {
    if let Ok(mut guard) = DDG_BLOCKED.get_or_init(|| Mutex::new(None)).lock() {
        *guard = Some(Instant::now());
    }
}
