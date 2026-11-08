//! Presse-papiers + sélection souris : drag dans l'historique = copie
//! (OSC 52 + repli mémoire), Ctrl+V / collage terminal = insertion dans
//! la barre de saisie (ou le buffer du flow `/connect`).
//!
//! Zéro dépendance : base64 maison, lecture système via `wl-paste` /
//! `xclip` / `xsel` / `pbpaste`, écriture par séquence OSC 52 (Ghostty
//! l'applique au presse-papiers système).

use ratatui::{
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
};

use crate::app::{App, Screen};
use crate::theme::{CANDLE, VOID};

/// Largeur de la gouttière en caractères (= `chat::GUTTER`, gardée par test).
const GUTTER_W: usize = 2;

/// Sélection souris : ancrage (pression) + tête (drag), coords écran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Selection {
    pub anchor: (u16, u16),
    pub head: (u16, u16),
}

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
pub(crate) fn osc52_seq(text: &str) -> String {
    format!("\x1b]52;c;{}\x1b\\", base64_encode(text.as_bytes()))
}

/// Copie `text` vers le terminal (vide = rien, échec silencieux).
/// En test : no-op (ne vole pas le presse-papiers du dev).
pub(crate) fn osc52_copy(text: &str) {
    if !text.is_empty() {
        #[cfg(not(test))]
        {
            use std::io::Write as _;
            let _ = write!(std::io::stdout().lock(), "{}", osc52_seq(text));
        }
    }
}

/// Aplatit un collage pour la barre monoligne : retours/tab → espace,
/// autres contrôles (échappements…) supprimés. Idempotent.
pub(crate) fn sanitize_paste(raw: &str) -> String {
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
pub(crate) fn read_clipboard_with(
    mut run: impl FnMut(&str, &[&str]) -> Option<String>,
) -> Option<String> {
    CLIP_CMDS
        .iter()
        .find_map(|(prog, args)| run(prog, args).filter(|t| !t.is_empty()))
}

/// Presse-papiers système (`None` = absent/illisible/vide : repli mémoire).
pub(crate) fn read_clipboard() -> Option<String> {
    read_clipboard_with(|prog, args| {
        let out = std::process::Command::new(prog).args(args).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    })
}

/// Texte à coller : système, sinon `fallback` (toujours assaini).
pub(crate) fn resolve_paste(fallback: &str) -> String {
    sanitize_paste(read_clipboard().as_deref().unwrap_or(fallback))
}

/// Rectangle intérieur de l'historique (miroir de `chat::draw` : 3 lignes
/// de saisie + bordures). Vide si la colonne est trop petite.
pub(crate) fn history_inner(area: Rect) -> Rect {
    let hist_h = area.height.saturating_sub(crate::chat::INPUT_HEIGHT);
    if area.width < 6 || hist_h <= 2 {
        return Rect::default();
    }
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        hist_h.saturating_sub(2),
    )
}

/// Texte brut d'une ligne rendue (gouttière `▍ ` retirée si présente).
pub(crate) fn plain_text(line: &Line) -> String {
    let raw: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    raw.strip_prefix(crate::chat::GUTTER)
        .unwrap_or(&raw)
        .to_string()
}

/// Plages sélectionnées : `(ligne, début, fin)` en colonnes de texte
/// (sans gouttière), ordonnées et bornées (`lens` = largeurs visibles).
pub(crate) fn ranges(lens: &[usize], inner: Rect, sel: &Selection) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    if inner.width == 0 || inner.height == 0 || lens.is_empty() {
        return out;
    }
    let ((ax, ay), (hx, hy)) = (sel.anchor, sel.head);
    let (r0, r1) = (ay.min(hy), ay.max(hy));
    let (cf, cl) = if ay <= hy { (ax, hx) } else { (hx, ax) };
    let col = |x: u16| x.saturating_sub(inner.x.saturating_add(GUTTER_W as u16)) as usize;
    for r in r0..=r1 {
        if r < inner.y {
            continue;
        }
        let li = (r - inner.y) as usize;
        if li >= lens.len() {
            continue;
        }
        let len = lens[li];
        let (a, b) = if r0 == r1 {
            (col(ax).min(col(hx)), col(ax).max(col(hx)))
        } else if r == r0 {
            (col(cf), len)
        } else if r == r1 {
            (0, col(cl))
        } else {
            (0, len)
        };
        out.push((li, a.min(len), b.min(len)));
    }
    out
}

/// Texte des plages (`plain` = lignes visibles sans gouttière), joint par `\n`.
pub(crate) fn extract(plain: &[String], inner: Rect, sel: &Selection) -> String {
    let lens: Vec<usize> = plain.iter().map(|l| l.chars().count()).collect();
    ranges(&lens, inner, sel)
        .iter()
        .map(|(li, a, b)| {
            plain[*li]
                .chars()
                .skip(*a)
                .take(b.saturating_sub(*a))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Coupe des spans à la colonne `at` (caractères), styles préservés.
fn cut_spans(spans: Vec<Span<'static>>, at: usize) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut pos = 0;
    for span in spans {
        let len = span.content.chars().count();
        if pos + len <= at {
            left.push(span);
        } else if pos >= at {
            right.push(span);
        } else {
            let cut = at - pos;
            let head: String = span.content.chars().take(cut).collect();
            let tail: String = span.content.chars().skip(cut).collect();
            if !head.is_empty() {
                left.push(Span::styled(head, span.style));
            }
            if !tail.is_empty() {
                right.push(Span::styled(tail, span.style));
            }
        }
        pos += len;
    }
    (left, right)
}

/// Surligne la sélection dans les lignes rendues (bougie sur vide).
pub(crate) fn highlight(
    visible: &mut [Line<'static>],
    lens: &[usize],
    inner: Rect,
    sel: &Selection,
) {
    let hl = Style::new().bg(CANDLE).fg(VOID);
    for (li, a, b) in ranges(lens, inner, sel) {
        if a >= b {
            continue;
        }
        let Some(line) = visible.get_mut(li) else {
            continue;
        };
        let spans = std::mem::take(&mut line.spans);
        let (mut left, rest) = cut_spans(spans, a + GUTTER_W);
        let (mut mid, mut right) = cut_spans(rest, b - a);
        for span in mid.iter_mut() {
            span.style = span.style.patch(hl);
        }
        left.append(&mut mid);
        left.append(&mut right);
        line.spans = left;
    }
}

impl App {
    /// Collage terminal (bracketed paste) : flow actif → son buffer,
    /// écran chat → la barre, autres écrans → ignoré.
    pub(crate) fn on_paste(&mut self, text: String) {
        if self.connect_flow.is_some() {
            self.paste_flow_text(&text);
        } else if self.screen == Screen::Chat {
            self.insert_paste(&text);
        }
    }

    /// Ctrl+V : presse-papiers système (repli : dernière copie) → barre.
    pub(crate) fn paste_clip(&mut self) {
        let text = resolve_paste(&self.clip_last);
        self.insert_paste(&text);
    }

    /// Insère un collage assaini dans la barre (sans chat : ignoré).
    fn insert_paste(&mut self, raw: &str) {
        let clean = sanitize_paste(raw);
        if clean.is_empty() {
            return;
        }
        let idx = self.sessions.index();
        if let Some(sess) = self.sessions.get_mut(idx) {
            sess.chat.input_insert(&clean);
        }
    }

    /// Pression gauche : démarre une sélection dans l'historique (hors
    /// historique : efface la précédente). Sans chat : rien.
    pub(crate) fn sel_press(&mut self, col: u16, row: u16) {
        if self.screen != Screen::Chat || self.sessions.count() == 0 {
            return;
        }
        let chat = &mut self.sessions.current_mut().chat;
        let inner = history_inner(chat.area.get());
        chat.select = inner
            .contains(Position::new(col, row))
            .then_some(Selection {
                anchor: (col, row),
                head: (col, row),
            });
    }

    /// Drag : étend la sélection en cours (sans sélection : ignoré).
    pub(crate) fn sel_drag(&mut self, col: u16, row: u16) {
        if self.sessions.count() == 0 {
            return;
        }
        if let Some(sel) = self.sessions.current_mut().chat.select.as_mut() {
            sel.head = (col, row);
        }
    }

    /// Relâchement : copie la sélection (OSC 52 + repli), puis l'efface.
    /// Vide (clic simple) : rien copié. Sans chat : rien.
    pub(crate) fn sel_release(&mut self) {
        if self.sessions.count() == 0 {
            return;
        }
        let sending = self.sessions.current().sending;
        let chat = &mut self.sessions.current_mut().chat;
        let Some(sel) = chat.select.take() else {
            return;
        };
        let inner = history_inner(chat.area.get());
        let plain = chat.visible_plain(sending);
        let text = extract(&plain, inner, &sel);
        if text.is_empty() {
            return;
        }
        osc52_copy(&text);
        self.clip_last = text;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Role;
    use crate::provider::ProviderConfig;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    /// App de test (clé factice, aucun appel réseau).
    fn test_app() -> App {
        App::from_config(ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        })
        .unwrap()
    }

    /// Intérieur d'une colonne historique type (x 20..80, y 1..21).
    const INNER: Rect = Rect::new(21, 2, 58, 15);

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

    #[test]
    fn geometrie_miroir_du_rendu_et_gouttiere_gardee() {
        assert_eq!(history_inner(Rect::new(20, 1, 60, 20)), INNER);
        assert_eq!(history_inner(Rect::new(0, 0, 5, 20)), Rect::default());
        assert_eq!(history_inner(Rect::new(0, 0, 60, 4)), Rect::default());
        assert_eq!(GUTTER_W, crate::chat::GUTTER.chars().count());
    }

    #[test]
    fn plages_ordonnees_puis_extraction_jointe() {
        let lens = [5, 5, 5];
        let one = Selection {
            anchor: (23, 2),
            head: (25, 2),
        };
        assert_eq!(ranges(&lens, INNER, &one), [(0, 0, 2)]);
        let multi = Selection {
            anchor: (23, 2),
            head: (24, 4),
        };
        assert_eq!(
            ranges(&lens, INNER, &multi),
            [(0, 0, 5), (1, 0, 5), (2, 0, 1)]
        );
        let rev = Selection {
            anchor: (24, 4),
            head: (23, 2),
        };
        assert_eq!(ranges(&lens, INNER, &rev), ranges(&lens, INNER, &multi));
        assert!(ranges(&[], INNER, &one).is_empty());
        let plain = [String::from("hello"), String::new(), String::from("bye")];
        let sel = Selection {
            anchor: (23, 2),
            head: (26, 4),
        };
        assert_eq!(extract(&plain, INNER, &sel), "hello\n\nbye");
        let clic = Selection {
            anchor: (23, 2),
            head: (23, 2),
        };
        assert_eq!(extract(&plain, INNER, &clic), "");
    }

    #[test]
    fn surlignage_teinte_sans_toucher_au_texte() {
        let mut lines = vec![Line::from(vec![Span::raw("xxhello")])];
        let sel = Selection {
            anchor: (23, 2),
            head: (28, 2),
        };
        highlight(&mut lines, &[5], INNER, &sel);
        let spans = &lines[0].spans;
        let texte: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(texte, "xxhello");
        let teinte = spans
            .iter()
            .find(|s| s.content.as_ref() == "hello")
            .unwrap();
        assert_eq!(teinte.style.bg, Some(CANDLE));
        assert_eq!(teinte.style.fg, Some(VOID));
    }

    /// Événement souris de type `kind` en `(col, row)`.
    fn event(kind: MouseEventKind, col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: col,
            row,
            modifiers: crossterm::event::KeyModifiers::empty(),
        }
    }

    #[test]
    fn drag_dans_l_historique_copie_puis_efface() {
        let mut app = test_app();
        app.sessions
            .current_mut()
            .chat
            .push(Role::Assistant, "bonsoir");
        app.sessions
            .current()
            .chat
            .area
            .set(Rect::new(20, 1, 60, 20));
        let down = MouseEventKind::Down(MouseButton::Left);
        let drag = MouseEventKind::Drag(MouseButton::Left);
        let up = MouseEventKind::Up(MouseButton::Left);
        // Corps du message en ligne 3 (étiquette en 2) : sélection entière.
        app.on_mouse(event(down, 23, 3));
        assert!(app.sessions.current().chat.select.is_some());
        app.on_mouse(event(drag, 30, 3));
        app.on_mouse(event(up, 30, 3));
        assert_eq!(app.clip_last, "bonsoir");
        assert!(app.sessions.current().chat.select.is_none());
        // Hors historique : pas de sélection ; clic simple : rien copié.
        app.on_mouse(event(down, 2, 10));
        assert!(app.sessions.current().chat.select.is_none());
        app.on_mouse(event(down, 23, 3));
        app.on_mouse(event(up, 23, 3));
        assert_eq!(app.clip_last, "bonsoir", "clic simple = inchangé");
    }

    #[test]
    fn collage_terminal_vers_barre_ou_ignore_hors_chat() {
        let mut app = test_app();
        app.on_paste(String::from("a\nb"));
        assert_eq!(app.sessions.current().chat.input, "a b");
        app.on_paste(String::new());
        assert_eq!(app.sessions.current().chat.input, "a b", "vide = inchangé");
        app.screen = Screen::Shop;
        app.on_paste(String::from("zzz"));
        assert_eq!(
            app.sessions.current().chat.input,
            "a b",
            "hors chat = ignoré"
        );
    }
}
