//! Historique d'envoi : fenêtre tronquée + nommage des chats.
//!
//! Port tel quel de `history.rs` du TUI (visibilités élargies pour
//! l'intégrateur : `send_window`, `window_estimate`, `trim_history`).

/// Tours de conversation max envoyés au provider (borne le prompt).
pub const MAX_HISTORY: usize = 40;
/// Messages max gardés par chat (borne mémoire, aligné sur l'affichage).
pub const HISTORY_MAX: usize = 200;
/// Caractères du premier message repris comme nom du chat.
const NAME_LEN: usize = 18;

/// Éviction LRU : sort les plus vieux au-delà du cap, garde la tête
/// `system` pinnée (l'historique est append-only, les lus = la queue).
pub fn trim_history(history: &mut Vec<(String, String)>, pinned: bool) {
    while history.len() > HISTORY_MAX {
        let head_system = history
            .first()
            .map(|(role, _)| role == "system")
            .unwrap_or(false);
        history.remove(usize::from(pinned && head_system));
    }
}

/// Découpe paresseuse : `None` si tout tient, sinon (tête gardée, queue).
fn window_cut(len: usize, pinned: bool) -> Option<(bool, usize)> {
    if len <= MAX_HISTORY {
        return None;
    }
    let drop = len - MAX_HISTORY;
    let keep_pin = pinned && len > 0;
    Some((keep_pin, drop + usize::from(keep_pin)))
}

/// Fenêtre d'historique envoyée au provider : 40 derniers, tête
/// `system` pinnée gardée, consigne des questions IA (après le pinné,
/// sinon en tête — `/compact` et `--once` ne passent pas ici).
/// Paresseux : ne clone que la queue gardée.
pub fn send_window(history: &[(String, String)], pinned: bool) -> Vec<(String, String)> {
    let mut window = match window_cut(history.len(), pinned) {
        None => history.to_vec(),
        Some((keep_pin, start)) => {
            let mut window = Vec::with_capacity(MAX_HISTORY);
            if keep_pin {
                window.push(history[0].clone());
            }
            window.extend(history[start..].iter().cloned());
            window
        }
    };
    let at = usize::from(pinned && !window.is_empty());
    window.insert(
        at,
        (
            String::from("system"),
            crate::question::QUESTION_SYSTEM.to_string(),
        ),
    );
    window
}

/// Estimation tokens de la fenêtre d'envoi (`+ pending` éventuel),
/// sans copie : même multi-ensemble que `send_window`, l'ordre étant
/// sans effet sur la somme. Sert la jauge, `/usage` et la garde.
pub fn window_estimate(history: &[(String, String)], pinned: bool, pending: Option<&str>) -> u64 {
    let total = history.len() + usize::from(pending.is_some());
    let mut sum = 0u64;
    // `MAX_HISTORY >= 2` garantit les index (`1 + drop <= len`, `drop <= len`).
    if total > MAX_HISTORY {
        let drop = total - MAX_HISTORY;
        let tail = if pinned && !history.is_empty() {
            sum += crate::tokens::estimate(&history[0].1) as u64;
            &history[1 + drop..]
        } else {
            &history[drop.min(history.len())..]
        };
        for (_, content) in tail {
            sum = sum.saturating_add(crate::tokens::estimate(content) as u64);
        }
    } else {
        for (_, content) in history {
            sum = sum.saturating_add(crate::tokens::estimate(content) as u64);
        }
    }
    if let Some(text) = pending {
        sum = sum.saturating_add(crate::tokens::estimate(text) as u64);
    }
    sum.saturating_add(crate::tokens::estimate(crate::question::QUESTION_SYSTEM) as u64)
}

/// Nom de chat depuis le premier message : 18 premiers caractères du
/// texte trimmé, en une seule allocation.
pub fn short_name(text: &str) -> String {
    text.trim().chars().take(NAME_LEN).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_window_keeps_pin_and_tail() {
        let plain: Vec<(String, String)> = (0..50)
            .map(|i| (String::from("user"), format!("m{i}")))
            .collect();
        let cut = send_window(&plain, false);
        assert_eq!(cut.len(), MAX_HISTORY + 1);
        assert_eq!(cut[0].1, crate::question::QUESTION_SYSTEM);
        assert_eq!(cut[1].1, "m10");
        let mut pinned = vec![(String::from("system"), String::from("pin"))];
        pinned.extend((0..50).map(|i| (String::from("user"), format!("m{i}"))));
        let kept = send_window(&pinned, true);
        assert_eq!(kept.len(), MAX_HISTORY + 1);
        assert_eq!(kept[0].1, "pin");
        assert_eq!(kept[1].1, crate::question::QUESTION_SYSTEM);
        assert_eq!(kept[2].1, "m11");
    }

    /// Ancien algorithme (clone tout + drain) : oracle d'équivalence.
    fn oracle(history: &[(String, String)], pinned: bool) -> Vec<(String, String)> {
        let mut window = history.to_vec();
        if window.len() > MAX_HISTORY {
            let drop = window.len() - MAX_HISTORY;
            if pinned && !window.is_empty() {
                window.drain(1..1 + drop);
            } else {
                window.drain(..drop);
            }
        }
        let at = usize::from(pinned && !window.is_empty());
        window.insert(
            at,
            (
                String::from("system"),
                crate::question::QUESTION_SYSTEM.to_string(),
            ),
        );
        window
    }

    fn corpus(pinned: bool, len: usize) -> Vec<(String, String)> {
        let mut history = Vec::new();
        if pinned {
            history.push((String::from("system"), String::from("pin")));
        }
        history.extend((0..len).map(|i| (String::from("user"), format!("m{i}"))));
        history
    }

    #[test]
    fn lazy_window_matches_eager_oracle() {
        for pinned in [false, true] {
            for len in [0, 1, 39, 40, 41, 50, 250] {
                let history = corpus(pinned, len);
                assert_eq!(send_window(&history, pinned), oracle(&history, pinned));
            }
        }
    }

    #[test]
    fn trim_history_garde_pin_et_queue_lru() {
        let mut history = vec![(String::from("system"), String::from("pin"))];
        history.extend((0..250).map(|i| (String::from("user"), format!("m{i}"))));
        trim_history(&mut history, true);
        assert_eq!(history.len(), HISTORY_MAX);
        assert_eq!(history[0].1, "pin");
        assert_eq!(history[1].1, "m51");
        assert_eq!(history[HISTORY_MAX - 1].1, "m249");
    }

    #[test]
    fn trim_history_sans_pin_sort_par_la_tete() {
        let mut history: Vec<(String, String)> = (0..250)
            .map(|i| (String::from("user"), format!("m{i}")))
            .collect();
        trim_history(&mut history, false);
        assert_eq!(history.len(), HISTORY_MAX);
        assert_eq!(history[0].1, "m50");
        let mut court = corpus(false, 40);
        trim_history(&mut court, false);
        assert_eq!(court.len(), 40);
    }

    #[test]
    fn window_estimate_matches_send_window_sum() {
        let plain: Vec<(String, String)> = (0..50)
            .map(|i| (String::from("user"), format!("message {i}")))
            .collect();
        let mut pinned = vec![(String::from("system"), String::from("pin"))];
        pinned.extend(plain.clone());
        for (history, pin) in [(&plain, false), (&pinned, true)] {
            for pending in [None, Some("hello")] {
                let mut grown = history.clone();
                if let Some(text) = pending {
                    grown.push((String::from("user"), text.to_string()));
                }
                let expected = crate::tokens::context_estimate(&send_window(&grown, pin));
                assert_eq!(window_estimate(history, pin, pending), expected);
            }
        }
    }

    #[test]
    fn short_name_trims_then_takes_18_chars() {
        assert_eq!(
            short_name("  hello world, this is long  "),
            "hello world, this "
        );
        assert_eq!(short_name("court"), "court");
    }
}
