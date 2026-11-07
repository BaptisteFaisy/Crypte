//! Historique d'envoi : fenêtre tronquée + nommage des chats.
//!
//! `send_window` borne la copie envoyée au provider (`MAX_HISTORY`,
//! tête `system` pinnée gardée) ; `short_name` nomme un chat depuis
//! son premier message. Partagé par `send`, `/tell` et `/usage`.

/// Tours de conversation max envoyés au provider (borne le prompt).
pub(crate) const MAX_HISTORY: usize = 40;
/// Messages max gardés par chat (borne mémoire, aligné sur l'affichage).
pub(crate) const HISTORY_MAX: usize = 200;
/// Caractères du premier message repris comme nom du chat.
const NAME_LEN: usize = 18;

/// Éviction LRU : sort les plus vieux au-delà du cap, garde la tête
/// `system` pinnée (l'historique est append-only, les lus = la queue).
pub(crate) fn trim_history(history: &mut Vec<(String, String)>, pinned: bool) {
    while history.len() > HISTORY_MAX {
        let head_system = history
            .first()
            .map(|(role, _)| role == "system")
            .unwrap_or(false);
        history.remove(usize::from(pinned && head_system));
    }
}

/// Découpe paresseuse : `None` si tout tient, sinon (tête gardée, queue).
fn window_cut(len: usize, extra: usize, pinned: bool) -> Option<(bool, usize)> {
    let total = len + extra;
    if total <= MAX_HISTORY {
        return None;
    }
    let drop = total - MAX_HISTORY;
    let keep_pin = pinned && len > 0;
    Some((keep_pin, drop + usize::from(keep_pin)))
}

/// Fenêtre d'historique envoyée au provider : 40 derniers, tête
/// `system` pinnée gardée. Paresseux : ne clone que la queue gardée.
pub(crate) fn send_window(history: &[(String, String)], pinned: bool) -> Vec<(String, String)> {
    match window_cut(history.len(), 0, pinned) {
        None => history.to_vec(),
        Some((keep_pin, start)) => {
            let mut window = Vec::with_capacity(MAX_HISTORY);
            if keep_pin {
                window.push(history[0].clone());
            }
            window.extend(history[start..].iter().cloned());
            window
        }
    }
}

/// Fenêtre d'envoi + message pendant (non stocké) : même résultat que
/// clone + push + `send_window`, sans cloner la tête jetée.
pub(crate) fn send_window_with(
    history: &[(String, String)],
    pinned: bool,
    extra: (String, String),
) -> Vec<(String, String)> {
    match window_cut(history.len(), 1, pinned) {
        None => {
            let mut window = history.to_vec();
            window.push(extra);
            window
        }
        Some((keep_pin, start)) => {
            let mut window = Vec::with_capacity(MAX_HISTORY);
            if keep_pin {
                window.push(history[0].clone());
            }
            window.extend(history[start..].iter().cloned());
            window.push(extra);
            window
        }
    }
}

/// Nom de chat depuis le premier message : 18 premiers caractères trimmés.
pub(crate) fn short_name(text: &str) -> String {
    text.chars()
        .take(NAME_LEN)
        .collect::<String>()
        .trim()
        .to_string()
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

    /// Ancien algorithme (clone tout + drain) : oracle d'équivalence.
    fn oracle(
        history: &[(String, String)],
        pinned: bool,
        extra: Option<(String, String)>,
    ) -> Vec<(String, String)> {
        let mut window = history.to_vec();
        if let Some(line) = extra {
            window.push(line);
        }
        if window.len() > MAX_HISTORY {
            let drop = window.len() - MAX_HISTORY;
            if pinned && !window.is_empty() {
                window.drain(1..1 + drop);
            } else {
                window.drain(..drop);
            }
        }
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
        let extra = || (String::from("user"), String::from("pendant"));
        for pinned in [false, true] {
            for len in [0, 1, 39, 40, 41, 50, 250] {
                let history = corpus(pinned, len);
                assert_eq!(
                    send_window(&history, pinned),
                    oracle(&history, pinned, None)
                );
                assert_eq!(
                    send_window_with(&history, pinned, extra()),
                    oracle(&history, pinned, Some(extra()))
                );
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
}
