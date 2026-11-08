//! Pompe bus + observation (drain KEEP, harvest ici).
//!
//! Exécution pompe/forward = `drain.rs` KEEP byte-identique, piloté par la
//! boucle `Incoming` de backend-core : `pump_chat_inbox` (drain.rs:281,
//! verse tout + UNE relance groupée si un `trigger`), `pump_agent_inbox`
//! (drain.rs:310, premier ask), `forward_link` (drain.rs:358, relais
//! hops+1, `capped` + note au-delà de `MAX_AUTO_HOPS=8`), `auto_tell`
//! (drain.rs:375) et `auto_ask` (drain.rs:389). Règles prouvées par les
//! `flow_tests` de bus.rs (TUI, inchangés).
//!
//! Ce module = couche d'observation : moissonne les événements JSONL
//! nouvellement appendés (`harvest`, utilisé par la sonde et le pont
//! `crypte-event`) et comptabilise les cycles (`PumpDelta`).

use std::path::Path;

/// Delta de compteurs bus sur un cycle drain (relances dues aux liens).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PumpDelta {
    pub forwarded: u64,
    pub capped: u64,
}

/// Delta saturé entre deux snapshots `(forwarded, capped)`.
pub fn delta(before: (u64, u64), after: (u64, u64)) -> PumpDelta {
    PumpDelta {
        forwarded: after.0.saturating_sub(before.0),
        capped: after.1.saturating_sub(before.1),
    }
}

/// Moissonne les lignes JSON appendées au `journal` après l'octet `depuis`.
/// Fichier raccourci (rotation/réécriture, events.rs rouvre à chaque fois)
/// = reprise à 0. Lignes vides ou malformées ignorées (queue partielle).
/// Rend `(événements, nouvel_offset)`.
pub fn harvest(journal: &Path, depuis: u64) -> (Vec<serde_json::Value>, u64) {
    let contenu = std::fs::read(journal).unwrap_or_default();
    let debut = if (depuis as usize) <= contenu.len() {
        depuis as usize
    } else {
        0
    };
    let texte = String::from_utf8_lossy(&contenu[debut..]);
    let mut out = Vec::new();
    for ligne in texte.lines() {
        let tasse = ligne.trim();
        if tasse.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(tasse) {
            out.push(v);
        }
    }
    (out, contenu.len() as u64)
}
