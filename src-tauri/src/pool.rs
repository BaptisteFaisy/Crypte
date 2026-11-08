//! Pool borné d'appels LLM : sémaphore global + canal borné.
//!
//! Port tel quel de `pool.rs` du TUI.

use std::future::Future;
use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;

/// Concurrence LLM max par défaut (surchargée par [`MAX_FLIGHT_ENV`]).
pub const DEFAULT_MAX_FLIGHT: usize = 4;
/// Réponses en attente de routage (producteurs en backpressure au-delà).
pub const INCOMING_CAP: usize = 64;
/// Variable d'environnement qui règle le plafond de concurrence.
pub const MAX_FLIGHT_ENV: &str = "CRYPTE_MAX_FLIGHT";

/// Pool de spawn : sémaphore global partageable (`Clone` cheap).
#[derive(Debug, Clone)]
pub struct SpawnPool {
    sem: Arc<Semaphore>,
    cap: usize,
}

impl SpawnPool {
    /// Pool configuré par l'environnement ([`MAX_FLIGHT_ENV`]).
    pub fn new() -> Self {
        let raw = std::env::var(MAX_FLIGHT_ENV).ok();
        Self::with_permits(parse_permits(raw.as_deref()))
    }

    /// Pool à `n` permis (`n.max(1)`, tests et usages explicites).
    pub fn with_permits(n: usize) -> Self {
        let cap = n.max(1);
        Self {
            sem: Arc::new(Semaphore::new(cap)),
            cap,
        }
    }

    /// Spawne `fut` : le permis est pris avant d'exécuter, rendu à la
    /// fin (file d'attente si le plafond est atteint, jamais de refus).
    pub fn spawn<F>(&self, fut: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let pool = self.clone();
        tokio::spawn(async move {
            let _permit = pool.acquire().await;
            fut.await
        })
    }

    /// Prend un permis (attend si plafond atteint) ; `None` si fermé.
    pub async fn acquire(&self) -> Option<OwnedSemaphorePermit> {
        self.sem.clone().acquire_owned().await.ok()
    }

    /// Plafond de concurrence (permis).
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Appels en vol (permis pris, affiché par `/usage`).
    pub fn in_flight(&self) -> usize {
        self.cap.saturating_sub(self.sem.available_permits())
    }
}

impl Default for SpawnPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse le plafond : entier ≥ 1, sinon [`DEFAULT_MAX_FLIGHT`].
fn parse_permits(raw: Option<&str>) -> usize {
    raw.and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(DEFAULT_MAX_FLIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_permits_defaut_et_garde_fous() {
        assert_eq!(parse_permits(None), DEFAULT_MAX_FLIGHT);
        assert_eq!(parse_permits(Some("6")), 6);
        assert_eq!(parse_permits(Some(" 2 ")), 2);
        assert_eq!(parse_permits(Some("0")), DEFAULT_MAX_FLIGHT);
        assert_eq!(parse_permits(Some("nope")), DEFAULT_MAX_FLIGHT);
        assert_eq!(parse_permits(Some("")), DEFAULT_MAX_FLIGHT);
    }

    #[test]
    fn with_permits_clampe_au_minimum_un() {
        assert_eq!(SpawnPool::with_permits(0).cap(), 1);
        assert_eq!(SpawnPool::with_permits(3).cap(), 3);
    }

    #[tokio::test]
    async fn spawn_ne_depasse_jamais_le_plafond() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let pool = SpawnPool::with_permits(2);
        let live = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let (pool, live, peak) = (pool.clone(), live.clone(), peak.clone());
            handles.push(pool.spawn(async move {
                let n = live.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(n, Ordering::SeqCst);
                tokio::task::yield_now().await;
                live.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        assert_eq!(peak.load(Ordering::SeqCst), 2);
        assert_eq!(pool.in_flight(), 0);
    }

    #[tokio::test]
    async fn in_flight_compte_les_permis_pris() {
        let pool = SpawnPool::with_permits(4);
        assert_eq!(pool.in_flight(), 0);
        let _a = pool.acquire().await.unwrap();
        let _b = pool.acquire().await.unwrap();
        assert_eq!(pool.in_flight(), 2);
    }
}
