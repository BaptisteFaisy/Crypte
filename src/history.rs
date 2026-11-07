//! Historique d'envoi : fenêtre tronquée + nommage des chats.
//!
//! `send_window` borne la copie envoyée au provider (`MAX_HISTORY`,
//! tête `system` pinnée gardée) ; `short_name` nomme un chat depuis
//! son premier message. Partagé par `send`, `/tell` et `/usage`.

/// Tours de conversation max envoyés au provider (borne le prompt).
pub(crate) const MAX_HISTORY: usize = 40;
/// Caractères du premier message repris comme nom du chat.
const NAME_LEN: usize = 18;

/// Fenêtre d'historique envoyée au provider : 40 derniers, tête
/// `system` pinnée gardée.
pub(crate) fn send_window(history: &[(String, String)], pinned: bool) -> Vec<(String, String)> {
    let mut window = history.to_vec();
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
}
