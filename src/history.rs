//! Historique d'envoi : fenêtre tronquée + nommage des chats.
//!
//! `send_window` borne la copie envoyée au provider (`MAX_HISTORY`,
//! tête `system` pinnée gardée) ; `window_estimate` estime la même
//! fenêtre sans rien copier (jauge, `/usage`, garde d'envoi) ;
//! `short_name` nomme un chat depuis son premier message.

/// Tours de conversation max envoyés au provider (borne le prompt).
pub(crate) const MAX_HISTORY: usize = 40;
/// Caractères du premier message repris comme nom du chat.
const NAME_LEN: usize = 18;

/// Fenêtre d'historique envoyée au provider : 40 derniers, tête
/// `system` pinnée gardée. Une seule passe, capacité exacte (jamais
/// de copie complète suivie d'un `drain`).
pub(crate) fn send_window(history: &[(String, String)], pinned: bool) -> Vec<(String, String)> {
    if history.len() <= MAX_HISTORY {
        return history.to_vec();
    }
    let drop = history.len() - MAX_HISTORY;
    let mut window = Vec::with_capacity(MAX_HISTORY);
    if pinned {
        window.push(history[0].clone());
        window.extend_from_slice(&history[1 + drop..]);
    } else {
        window.extend_from_slice(&history[drop..]);
    }
    window
}

/// Estimation tokens de la fenêtre d'envoi (`+ pending` éventuel),
/// sans copie : même multi-ensemble que `send_window`, l'ordre étant
/// sans effet sur la somme. Sert la jauge, `/usage` et la garde.
pub(crate) fn window_estimate(
    history: &[(String, String)],
    pinned: bool,
    pending: Option<&str>,
) -> u64 {
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
    sum
}

/// Nom de chat depuis le premier message : 18 premiers caractères du
/// texte trimmé, en une seule allocation.
pub(crate) fn short_name(text: &str) -> String {
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
        assert_eq!(cut.len(), MAX_HISTORY);
        assert_eq!(cut[0].1, "m10");
        let mut pinned = vec![(String::from("system"), String::from("pin"))];
        pinned.extend((0..50).map(|i| (String::from("user"), format!("m{i}"))));
        let kept = send_window(&pinned, true);
        assert_eq!(kept.len(), MAX_HISTORY);
        assert_eq!(kept[0].1, "pin");
        assert_eq!(kept[1].1, "m11");
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
