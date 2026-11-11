//! Image Kitty « vivante » (hub, jeu Haunt) : placeholders posés une fois
//! par ratatui-image, puis une nouvelle image PNG à chaque frame sous le
//! même identifiant (placement virtuel, silencieux). Le terminal n'a qu'à
//! remplacer la texture : animation nette, sans redessiner les cellules.
//!
//! L'encodage (PNG + base64) tourne sur trois fils à part : le fil
//! principal compose la frame suivante pendant ce temps. Encodeurs tous
//! occupés = frame sautée (jamais d'attente) ; frames numérotées, jamais
//! une plus ancienne écrite après une plus récente ; séquence écrite à
//! la frame d'après (une frame de retard).

use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{DynamicImage, ExtendedColorType, ImageEncoder, RgbaImage};
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
use ratatui_image::{
    protocol::{kitty::Kitty, Protocol},
    Image,
};

use super::iso::px;
use crate::theme;

/// Fils d'encodage par vue (HiDPI à 240 images/s : deux ne suffisent pas).
const ENCODERS: usize = 3;

/// Travail d'encodage : numéro de frame, image plein cadre, agrandissement.
type Job = (u64, RgbaImage, u32);
/// Résultat : numéro, séquence Kitty, image rendue pour réemploi.
type Done = (u64, Option<String>, RgbaImage, u32);

/// Image Kitty vivante : placeholders posés une fois (ratatui-image), puis
/// une nouvelle image PNG par frame sous le même identifiant.
pub struct View {
    id: u32,
    live: Option<(Rect, Protocol)>,
    pending: Option<String>,
    /// Images plein cadre rendues par les encodeurs, réutilisées (bords
    /// VOID posés une fois) : ni allocation ni remplissage par frame.
    spare: Vec<(RgbaImage, u32)>,
    encoders: Vec<Encoder>,
    /// Numéro de la prochaine frame, de la dernière écrite.
    next: u64,
    shown: u64,
}

/// Fil d'encodage : reçoit un `Job`, rend un `Done`.
struct Encoder {
    jobs: SyncSender<Job>,
    done: Receiver<Done>,
    busy: bool,
}

impl Encoder {
    /// Lance le fil (il s'arrête quand la vue est détruite).
    fn spawn(id: u32) -> Option<Encoder> {
        let (jobs, inbox) = mpsc::sync_channel::<Job>(1);
        let (outbox, done) = mpsc::channel();
        std::thread::Builder::new()
            .name(String::from("kitty-png"))
            .spawn(move || {
                for (n, img, scale) in inbox {
                    let seq = kitty_png(id, &img, scale);
                    if outbox.send((n, seq, img, scale)).is_err() {
                        break;
                    }
                }
            })
            .ok()?;
        Some(Encoder {
            jobs,
            done,
            busy: false,
        })
    }
}

impl View {
    /// Identifiant d'image tiré au hasard (24 bits, non nul).
    pub fn new() -> Self {
        Self {
            id: rand::random_range(1..0x00FF_FFFF),
            live: None,
            pending: None,
            spare: Vec::new(),
            encoders: Vec::new(),
            next: 1,
            shown: 0,
        }
    }

    /// Séquence Kitty de la dernière frame encodée, à écrire après le rendu.
    pub fn take_pending(&mut self) -> Option<String> {
        self.collect();
        self.pending.take()
    }

    /// Tests : attend la fin des encodages en cours (2 s au plus).
    #[cfg(test)]
    pub fn wait_pending(&mut self) -> Option<String> {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while self.encoders.iter().any(|e| e.busy) && std::time::Instant::now() < until {
            self.collect();
            std::thread::yield_now();
        }
        self.take_pending()
    }

    /// Récupère les frames encodées (sans attendre) : garde la plus
    /// récente, recycle les images.
    fn collect(&mut self) {
        let mut alive = Vec::with_capacity(self.encoders.len());
        for mut enc in std::mem::take(&mut self.encoders) {
            match enc.done.try_recv() {
                Ok((n, seq, img, scale)) => {
                    enc.busy = false;
                    if n > self.shown && seq.is_some() {
                        self.shown = n;
                        self.pending = seq;
                    }
                    self.spare.push((img, scale));
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => continue,
            }
            alive.push(enc);
        }
        self.encoders = alive;
    }

    /// Un encodeur libre (lancé au besoin) ; `None` : tous occupés.
    fn idle(&mut self) -> Option<usize> {
        if let Some(i) = self.encoders.iter().position(|e| !e.busy) {
            return Some(i);
        }
        if self.encoders.len() < ENCODERS {
            if let Some(enc) = Encoder::spawn(self.id) {
                self.encoders.push(enc);
                return Some(self.encoders.len() - 1);
            }
        }
        None
    }

    /// Image plein cadre `bw`×`bh` à ×`scale` : recyclée si possible.
    fn frame(&mut self, (bw, bh): (u32, u32), scale: u32) -> RgbaImage {
        let fits = |(img, s): &(RgbaImage, u32)| *s == scale && img.dimensions() == (bw, bh);
        match self.spare.iter().position(fits) {
            Some(i) => self.spare.swap_remove(i).0,
            None => {
                self.spare.clear();
                RgbaImage::from_pixel(bw, bh, px(theme::VOID))
            }
        }
    }

    /// Confie `frame` à l'encodeur `i` (encodage sur place s'il refuse).
    fn submit(&mut self, i: usize, frame: RgbaImage, scale: u32) {
        let n = self.next;
        self.next += 1;
        match self.encoders[i].jobs.try_send((n, frame, scale)) {
            Ok(()) => self.encoders[i].busy = true,
            Err(err) => {
                let (n, frame, scale) = match err {
                    mpsc::TrySendError::Full(job) | mpsc::TrySendError::Disconnected(job) => job,
                };
                self.shown = n;
                self.pending = kitty_png(self.id, &frame, scale);
            }
        }
    }
}

impl Default for View {
    fn default() -> Self {
        Self::new()
    }
}

/// Kitty : agrandit le canevas (×`scale`, plus proche voisin) à la taille
/// pixel exacte de la zone ; 1re frame via ratatui-image, puis PNG par frame.
pub fn present_kitty(
    view: &mut View,
    cv: &RgbaImage,
    scale: u32,
    (bw, bh): (u32, u32),
    area: Rect,
    buf: &mut Buffer,
) {
    view.collect();
    match &view.live {
        Some((rect, proto)) if *rect == area => {
            Image::new(proto).render(area, buf);
            let Some(i) = view.idle() else {
                return;
            };
            let mut frame = view.frame((bw, bh), scale);
            upscale(cv, scale, &mut frame);
            view.submit(i, frame, scale);
        }
        _ => {
            let mut frame = view.frame((bw, bh), scale);
            upscale(cv, scale, &mut frame);
            let img = DynamicImage::ImageRgba8(frame);
            let Ok(kitty) = Kitty::new(img, area, view.id, false) else {
                return;
            };
            let proto = Protocol::Kitty(kitty);
            Image::new(&proto).render(area, buf);
            view.live = Some((area, proto));
        }
    }
}

/// Pose `cv` agrandi ×`scale` (plus proche voisin) au centre de `out` :
/// chaque rangée est agrandie une fois puis recopiée `scale` fois.
fn upscale(cv: &RgbaImage, scale: u32, out: &mut RgbaImage) {
    let (bw, bh) = out.dimensions();
    let (dx, dy) = (
        (bw - cv.width() * scale) as usize / 2,
        (bh - cv.height() * scale) as usize / 2,
    );
    let (k, stride) = (scale as usize, bw as usize * 4);
    let mut line = Vec::with_capacity(cv.width() as usize * k * 4);
    let raw: &mut [u8] = out;
    for (y, row) in cv.rows().enumerate() {
        line.clear();
        for p in row {
            for _ in 0..k {
                line.extend_from_slice(&p.0);
            }
        }
        for r in 0..k {
            let start = (dy + y * k + r) * stride + dx * 4;
            raw[start..start + line.len()].copy_from_slice(&line);
        }
    }
}

/// PNG « rapide » (CompressionType::Fast + filtre Sub) : partagé par le hub
/// et le fond 3D (`space/view.rs`).
pub(crate) fn png_fast(img: &RgbaImage) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Sub)
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            ExtendedColorType::Rgba8,
        )
        .ok()?;
    Some(png)
}

/// Séquence Kitty « remplace l'image `id` » (PNG, placement virtuel,
/// silencieuse), découpée en morceaux de 4096 octets base64.
/// Filtre PNG : `Sub` aux petits agrandissements (×1, ×2 : 30-40 % plus
/// rapide, mesuré), `Adaptive` au-delà (rangées répétées, mieux compressé).
fn kitty_png(id: u32, img: &RgbaImage, scale: u32) -> Option<String> {
    let filter = if scale <= 2 {
        FilterType::Sub
    } else {
        FilterType::Adaptive
    };
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, filter)
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            ExtendedColorType::Rgba8,
        )
        .ok()?;
    let data = base64(&png);
    let chunks: Vec<&[u8]> = data.as_bytes().chunks(4096).collect();
    let mut out = String::with_capacity(data.len() + chunks.len() * 16 + 48);
    for (i, chunk) in chunks.iter().enumerate() {
        let more = u8::from(i + 1 < chunks.len());
        let head = if i == 0 {
            format!("a=T,f=100,U=1,q=2,i={id},")
        } else {
            String::new()
        };
        out.push_str(&format!("\x1b_G{head}m={more};"));
        out.push_str(std::str::from_utf8(chunk).ok()?);
        out.push_str("\x1b\\");
    }
    Some(out)
}

/// Base64 standard (RFC 4648, avec bourrage) — partagé par `space/view.rs`.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for k in 0..4 {
            if k <= c.len() {
                out.push(ABC[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn kitty_frames_reuse_one_image_id() {
        let img = RgbaImage::from_pixel(64, 40, Rgba([9, 8, 7, 255]));
        let seq = kitty_png(4242, &img, 3).unwrap();
        assert!(seq.starts_with("\x1b_Ga=T,f=100,U=1,q=2,i=4242,m="));
        assert!(seq.ends_with("\x1b\\"));
        assert_eq!(seq.matches("i=4242").count(), 1);
    }
}
