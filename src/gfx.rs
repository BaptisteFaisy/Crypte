//! Images dans le terminal, au pixel près : protocole graphique Kitty
//! (Ghostty, kitty, WezTerm…) détecté au démarrage, demi-blocs ailleurs.
//!
//! Le sprite (32×32) est agrandi par un facteur **entier** au plus proche
//! voisin puis posé sur un canevas de exactement la taille pixel de la
//! zone : le terminal l'affiche sans redimensionner, donc sans flou.
//! - Kitty : 1 pixel image = 1 pixel écran (taille de cellule interrogée).
//! - Demi-blocs : 1 pixel image = 1 demi-cellule (`▀`, 2 pixels par cellule).
//!
//! Chaque (créature, taille) n'est encodée et transmise qu'une fois.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::process::Command;
use std::time::{Duration, Instant};

use image::{DynamicImage, Rgba};
use ratatui::{layout::Rect, Frame};
use ratatui_image::{
    picker::{Picker, ProtocolType},
    protocol::Protocol,
    Image, Resize,
};

use crate::bestiary::{self, Creature};

/// Taille de cellule supposée sans réponse du terminal (rapport ~1:2).
const FALLBACK_FONT: (u16, u16) = (10, 20);
/// Demi-blocs : une cellule = 1×2 pixels d'image.
const HALFBLOCK_FONT: (u16, u16) = (1, 2);
/// Au-delà, les entrées expirées puis la moitié la moins récente
/// sont évincées (redimensionnements successifs).
const CACHE_MAX: usize = 96;
/// Les tailles non redessinées depuis ce délai sont évincées.
const CACHE_TTL: Duration = Duration::from_secs(600);
/// Fond des pixels transparents en demi-blocs (= `theme::VOID`, comme le
/// fond de fenêtre que Ghostty montre sous les pixels transparents Kitty).
const VOID_RGBA: Rgba<u8> = Rgba([0x0C, 0x09, 0x08, 0xFF]);
/// Délai max de la sonde terminal (parité `query_with_timeout`, 1 s).
const TTY_TIMEOUT: Duration = Duration::from_secs(1);
/// Réponses terminales : quelques centaines d'octets suffisent.
const TTY_CAP: usize = 4096;

/// Créatures déjà encodées pour le terminal, par (créature, taille de zone).
pub struct ImageBank {
    picker: Picker,
    cache: HashMap<CacheKey, CacheEntry>,
    drawn: Vec<Rect>,
}

/// (slug créature, largeur, hauteur de zone).
type CacheKey = (&'static str, u16, u16);

/// Protocole encodé + dernier dessin (ordre d'éviction LRU).
struct CacheEntry {
    proto: Protocol,
    last_used: Instant,
}

/// Réponse de la sonde : (protocole graphique, taille de cellule).
type TtyHit = (Option<ProtocolType>, Option<(u16, u16)>);

impl ImageBank {
    /// Interroge le terminal (protocole + taille de cellule). À appeler
    /// après `ratatui::init` et avant de lire les événements.
    ///
    /// Sonde maison sur `/dev/tty`, jamais sur stdin :
    /// `Picker::from_query_stdio` (ratatui-image 8.1.1) fuit un thread bloqué
    /// en `read(stdin)` après son timeout, qui vole ensuite ~1 frappe sur 2
    /// à crossterm (deux lecteurs sur fd 0). Ici aucun thread : `poll` borne
    /// la lecture, le descripteur est fermé avant la boucle d'événements.
    pub fn detect() -> Self {
        let (proto, cell) = query_tty(TTY_TIMEOUT).unwrap_or((None, None));
        let cell = cell.or_else(winsize_cell).unwrap_or(FALLBACK_FONT);
        let mut picker = Picker::from_fontsize(cell);
        // Parité ratatui-image : l'env (tmux/iTerm2) gagne sur la sonde ;
        // en revanche l'env est conservé quand la sonde reste muette.
        let env_silent = picker.protocol_type() == ProtocolType::Halfblocks;
        if let (true, Some(proto)) = (env_silent, proto) {
            picker.set_protocol_type(proto);
        }
        Self::with_picker(picker)
    }

    /// Sans terminal (tests, `--smoke`) : demi-blocs.
    pub fn headless() -> Self {
        Self::with_picker(Picker::from_fontsize(FALLBACK_FONT))
    }

    fn with_picker(picker: Picker) -> Self {
        let picker = if picker.protocol_type() == ProtocolType::Halfblocks {
            let mut half = Picker::from_fontsize(HALFBLOCK_FONT);
            half.set_protocol_type(ProtocolType::Halfblocks);
            half.set_background_color(VOID_RGBA);
            half
        } else {
            picker
        };
        Self {
            picker,
            cache: HashMap::new(),
            drawn: Vec::new(),
        }
    }

    /// Nouvelle frame : oublie les zones d'images de la précédente.
    pub fn begin_frame(&mut self) {
        self.drawn.clear();
    }

    /// Zones occupées par des images dans la frame courante (les effets
    /// n'y touchent pas).
    /// Protège `area` des effets pour cette frame (scène pixel du hub).
    pub fn protect(&mut self, area: Rect) {
        self.drawn.push(area);
    }

    pub fn drawn(&self) -> &[Rect] {
        &self.drawn
    }

    /// Taille de cellule (pixels) si le terminal parle Kitty, sinon `None`.
    pub fn kitty_cell(&self) -> Option<(u16, u16)> {
        (self.picker.protocol_type() == ProtocolType::Kitty).then(|| self.picker.font_size())
    }

    /// Dessine `creature` centrée dans `area`, au pixel près.
    pub fn draw(&mut self, frame: &mut Frame, area: Rect, creature: &'static Creature) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let key = (creature.slug, area.width, area.height);
        if let Some(hit) = self.cache.get_mut(&key) {
            hit.last_used = Instant::now();
        } else {
            let Some(proto) = self.encode(area, creature) else {
                return;
            };
            self.evict_if_full();
            let hit = CacheEntry {
                proto,
                last_used: Instant::now(),
            };
            self.cache.insert(key, hit);
        }
        if let Some(hit) = self.cache.get(&key) {
            frame.render_widget(Image::new(&hit.proto), area);
            self.drawn.push(area);
        }
    }

    /// Fait de la place : expirés d'abord, sinon la moitié la moins
    /// récente (le cache reste tiède, sans pic de ré-encodage).
    fn evict_if_full(&mut self) {
        if self.cache.len() < CACHE_MAX {
            return;
        }
        self.cache.retain(|_, e| e.last_used.elapsed() < CACHE_TTL);
        if self.cache.len() < CACHE_MAX {
            return;
        }
        let mut oldest: Vec<(CacheKey, Instant)> =
            self.cache.iter().map(|(k, e)| (*k, e.last_used)).collect();
        oldest.sort_by_key(|(_, at)| *at);
        for (key, _) in oldest.into_iter().take(CACHE_MAX / 2) {
            self.cache.remove(&key);
        }
    }

    /// Canevas pixel de la taille exacte de `area` (rien à redimensionner).
    fn encode(&self, area: Rect, creature: &Creature) -> Option<Protocol> {
        let (fw, fh) = self.picker.font_size();
        let w = u32::from(area.width) * u32::from(fw);
        let h = u32::from(area.height) * u32::from(fh);
        let img = bestiary::fit(&creature.sprite().ok()?, w, h);
        self.picker
            .new_protocol(DynamicImage::ImageRgba8(img), area, Resize::Fit(None))
            .ok()
    }
}

/// Sous tmux ? (même test que ratatui-image : la sonde y est enveloppée
/// en DCS passthrough pour atteindre le terminal extérieur).
fn is_tmux() -> bool {
    std::env::var("TERM").is_ok_and(|t| t.starts_with("tmux"))
        || std::env::var("TERM_PROGRAM").is_ok_and(|t| t == "tmux")
}

/// Octets de la sonde : Kitty (`a=q`) + attributs (sixel) + taille de
/// cellule (`[16t`), terminés par un DSR (`[5n`) qui garantit une réponse.
fn tty_query(tmux: bool) -> Vec<u8> {
    let esc = if tmux { "\x1b\x1b" } else { "\x1b" };
    let mut q = String::new();
    if tmux {
        q.push_str("\x1bPtmux;");
    }
    q.push_str(&format!("{esc}_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA{esc}\\"));
    q.push_str(&format!("{esc}[c{esc}[16t{esc}[5n"));
    if tmux {
        q.push_str("\x1b\\");
    }
    q.into_bytes()
}

/// Autorise le passthrough DCS avant la sonde (parité ratatui-image :
/// sans lui tmux avale la sonde enveloppée et le terminal ne répond pas).
fn enable_tmux_passthrough() {
    let _ = Command::new("tmux")
        .args(["set", "-p", "allow-passthrough", "on"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| child.wait());
}

/// Sonde le terminal via `/dev/tty` (écriture + lecture) : aucun octet
/// ne transite par stdin, aucun thread n'est créé. `None` = pas de
/// terminal (le fallback demi-blocs s'applique). Suppose le mode raw
/// déjà actif (`detect` ne s'appelle qu'après `ratatui::init`).
fn query_tty(timeout: Duration) -> Option<TtyHit> {
    let tmux = is_tmux();
    if tmux {
        enable_tmux_passthrough();
    }
    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;
    tty.write_all(&tty_query(tmux)).ok()?;
    tty.flush().ok()?;
    let fd = tty.as_raw_fd();
    let deadline = Instant::now() + timeout;
    let mut buf: Vec<u8> = Vec::new();
    while Instant::now() < deadline && buf.len() < TTY_CAP {
        let left = deadline.saturating_duration_since(Instant::now());
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // `poll` : EINTR relancé, erreur ou silence = fin de sonde.
        let ready = unsafe { libc::poll(&mut pfd, 1, left.as_millis() as libc::c_int) };
        if ready <= 0 {
            if ready < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            break;
        }
        let mut chunk = [0u8; 256];
        let n = tty.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(3).any(|w| w == b"[0n") {
            break;
        }
    }
    Some(parse_tty_response(&buf))
}

/// Lit la réponse : Kitty (`i=31` acquitté `OK`), sixel (paramètre `4`
/// du DA1), taille de cellule (`[6;H;Wt`). Sans `ESC` imposé : sous tmux
/// les réponses reviennent enveloppées (DCS, `ESC` dédoublés).
fn parse_tty_response(buf: &[u8]) -> TtyHit {
    let text = String::from_utf8_lossy(buf);
    let kitty = text.contains("Gi=31;OK");
    let mut sixel = false;
    for at in text.match_indices("[?") {
        let rest = &text[at.0 + 2..];
        if let Some(end) = rest.find('c') {
            let params = &rest[..end];
            if params.len() < 32
                && params.bytes().all(|b| b.is_ascii_digit() || b == b';')
                && params.split(';').any(|p| p == "4")
            {
                sixel = true;
            }
        }
    }
    let mut cell = None;
    for at in text.match_indices("[6;") {
        let rest = &text[at.0..];
        if let Some(end) = rest.find('t') {
            if end < 24 {
                let parts: Vec<&str> = rest[..end].split(';').collect();
                if let ["[6", h, w] = parts[..] {
                    if let (Ok(h), Ok(w)) = (h.parse::<u16>(), w.parse::<u16>()) {
                        if h > 0 && w > 0 {
                            cell = Some((w, h));
                        }
                    }
                }
            }
        }
    }
    let proto = if kitty {
        Some(ProtocolType::Kitty)
    } else if sixel {
        Some(ProtocolType::Sixel)
    } else {
        None
    };
    (proto, cell)
}

/// Taille de cellule via `TIOCGWINSZ` (pixels ÷ cases, parité
/// `font_size_fallback`) quand la sonde n'a pas donné de `[16t`.
fn winsize_cell() -> Option<(u16, u16)> {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) };
    if ok != 0 || ws.ws_xpixel == 0 || ws.ws_ypixel == 0 || ws.ws_col == 0 || ws.ws_row == 0 {
        return None;
    }
    Some((ws.ws_xpixel / ws.ws_col, ws.ws_ypixel / ws.ws_row))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn halfblocks_map_one_sprite_pixel_per_half_cell() {
        use ratatui::style::Color;
        let mut bank = ImageBank::headless();
        let mut term = Terminal::new(TestBackend::new(32, 16)).unwrap();
        let ghost = bestiary::get("fantome");
        for _ in 0..3 {
            bank.begin_frame();
            term.draw(|f| bank.draw(f, f.area(), ghost)).unwrap();
        }
        assert_eq!(bank.cache.len(), 1);
        assert_eq!(bank.drawn(), &[Rect::new(0, 0, 32, 16)]);
        let sprite = ghost.sprite().unwrap();
        let px = |x: u16, y: u16| {
            let [r, g, b, a] = sprite.get_pixel(u32::from(x), u32::from(y)).0;
            let [vr, vg, vb, _] = VOID_RGBA.0;
            if a == 255 {
                Color::Rgb(r, g, b)
            } else {
                Color::Rgb(vr, vg, vb)
            }
        };
        let buf = term.backend().buffer();
        for (x, y) in (0..32).flat_map(|x| (0..16).map(move |y| (x, y))) {
            let cell = &buf[(x, y)];
            let (up, low) = match cell.symbol() {
                "▀" => (cell.fg, cell.bg),
                "▄" => (cell.bg, cell.fg),
                _ => (cell.bg, cell.bg),
            };
            assert_eq!(
                (up, low),
                (px(x, 2 * y), px(x, 2 * y + 1)),
                "cellule ({x},{y})"
            );
        }
    }

    #[test]
    fn protocol_covers_exactly_the_area() {
        let bank = ImageBank::headless();
        let area = Rect::new(3, 2, 22, 9);
        let proto = bank.encode(area, bestiary::get("crane")).unwrap();
        assert_eq!(proto.area().width, area.width);
        assert_eq!(proto.area().height, area.height);
    }

    #[test]
    fn full_cache_evicts_oldest_half_not_all() {
        let mut bank = ImageBank::headless();
        let mut term = Terminal::new(TestBackend::new(128, 64)).unwrap();
        let ghost = bestiary::get("fantome");
        for w in 1..=100u16 {
            bank.begin_frame();
            term.draw(|f| bank.draw(f, Rect::new(0, 0, w, 1), ghost))
                .unwrap();
        }
        // 96 entrées puis éviction de moitié : 96 - 48 + 4 = 52.
        assert_eq!(bank.cache.len(), 52);
        assert!(bank.cache.contains_key(&("fantome", 100, 1)));
    }

    #[test]
    fn tty_response_detects_kitty_and_cell() {
        let reply = b"\x1b_Gi=31;OK\x1b\\\x1b[?62;c\x1b[6;28;14t\x1b[0n";
        let (proto, cell) = parse_tty_response(reply);
        assert_eq!(proto, Some(ProtocolType::Kitty));
        assert_eq!(cell, Some((14, 28)));
    }

    #[test]
    fn tty_response_reads_through_tmux_wrapping() {
        // tmux renvoie enveloppé DCS, ESC dédoublés : la lecture l'ignore.
        let reply = b"\x1bPtmux;\x1b\x1b_Gi=31;OK\x1b\x1b\\\x1b\x1b[6;20;10t\x1b\x1b[0n\x1b\\";
        let (proto, cell) = parse_tty_response(reply);
        assert_eq!(proto, Some(ProtocolType::Kitty));
        assert_eq!(cell, Some((10, 20)));
    }

    #[test]
    fn tty_response_detects_sixel_da() {
        let (proto, cell) = parse_tty_response(b"\x1b[?62;4c\x1b[0n");
        assert_eq!(proto, Some(ProtocolType::Sixel));
        assert_eq!(cell, None);
    }

    #[test]
    fn tty_response_silence_or_garbage_falls_back() {
        assert_eq!(parse_tty_response(b""), (None, None));
        assert_eq!(parse_tty_response(b"\x1bhonkey\x1b[0n"), (None, None));
        // DA sans sixel : pas de protocole, mais pas d'erreur.
        assert_eq!(parse_tty_response(b"\x1b[?62c\x1b[0n"), (None, None));
    }

    #[test]
    fn tty_query_wraps_for_tmux() {
        let plain = tty_query(false);
        assert!(plain.starts_with(b"\x1b_Gi=31"));
        assert!(plain.ends_with(b"\x1b[5n"));
        let tmux = tty_query(true);
        assert!(tmux.starts_with(b"\x1bPtmux;\x1b\x1b_Gi=31"));
        assert!(tmux.ends_with(b"\x1b\x1b[5n\x1b\\"));
    }
}
