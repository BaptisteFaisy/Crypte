//! Presse-papiers : lecture système + assainissement des collages.
//!
//! Port du sous-ensemble sans TUI de `clip.rs` : base64 maison, OSC 52,
//! `sanitize_paste`, lecture via `wl-paste`/`xclip`/`xsel`/`pbpaste`.
//! La sélection souris (drag, géométrie, surlignage) ne part pas : côté
//! desktop, la sélection vit dans le WebView et la copie passe par le
//! plugin clipboard de Tauri ; `sanitize_paste` ressert à l'insertion.

/// Base64 standard (alphabet + bourrage `=`, sans retours).
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(chunk.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(chunk.get(2).copied().unwrap_or(0));
        out.push(ALPHA[(n >> 18 & 63) as usize] as char);
        out.push(ALPHA[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHA[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHA[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Séquence OSC 52 qui dépose `text` dans le presse-papiers (`c`).
pub fn osc52_seq(text: &str) -> String {
    format!("\x1b]52;c;{}\x1b\\", base64_encode(text.as_bytes()))
}

/// Aplatit un collage pour une saisie monoligne : retours/tab → espace,
/// autres contrôles (échappements…) supprimés. Idempotent.
pub fn sanitize_paste(raw: &str) -> String {
    raw.replace("\r\n", " ")
        .replace(['\r', '\n', '\t'], " ")
        .chars()
        .filter(|c| !c.is_control())
        .collect()
}

/// Commandes de lecture du presse-papiers (Wayland, X11 ×2, macOS).
const CLIP_CMDS: [(&str, &[&str]); 4] = [
    ("wl-paste", &["--no-newline"]),
    ("xclip", &["-selection", "clipboard", "-o"]),
    ("xsel", &["--clipboard", "--output"]),
    ("pbpaste", &[]),
];

/// Première commande qui rend du non-vide (`run` injecté pour les tests).
pub fn read_clipboard_with(mut run: impl FnMut(&str, &[&str]) -> Option<String>) -> Option<String> {
    CLIP_CMDS
        .iter()
        .find_map(|(prog, args)| run(prog, args).filter(|t| !t.is_empty()))
}

/// Presse-papiers système (`None` = absent/illisible/vide : repli mémoire).
pub fn read_clipboard() -> Option<String> {
    read_clipboard_with(|prog, args| {
        let out = std::process::Command::new(prog).args(args).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    })
}

/// Texte à coller : système, sinon `fallback` (toujours assaini).
pub fn resolve_paste(fallback: &str) -> String {
    sanitize_paste(read_clipboard().as_deref().unwrap_or(fallback))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_et_osc52() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode("hello".as_bytes()), "aGVsbG8=");
        assert_eq!(osc52_seq("hi"), "\x1b]52;c;aGk=\x1b\\");
    }

    #[test]
    fn sanitize_aplatit_les_retours_et_chasse_les_controles() {
        assert_eq!(sanitize_paste("a\r\nb\nc\td\x1be\x07f"), "a b c def");
        let flat = sanitize_paste("collé sans surprise");
        assert_eq!(sanitize_paste(&flat), flat, "idempotent");
        assert_eq!(sanitize_paste(""), "");
    }

    #[test]
    fn lecture_ignore_le_vide_et_prend_le_premier_non_vide() {
        let mut calls = 0;
        let hit = read_clipboard_with(|_, _| {
            calls += 1;
            if calls == 1 {
                Some(String::new())
            } else {
                Some(String::from("x"))
            }
        });
        assert_eq!(hit.as_deref(), Some("x"));
        assert_eq!(read_clipboard_with(|_, _| None), None);
    }
}
