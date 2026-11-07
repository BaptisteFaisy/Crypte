//! Image Kitty « vivante » du hub : placeholders posés une fois par
//! ratatui-image, puis une nouvelle image PNG à chaque frame sous le même
//! identifiant (placement virtuel, silencieux). Le terminal n'a qu'à
//! remplacer la texture : animation nette, sans redessiner les cellules.

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{DynamicImage, ExtendedColorType, ImageEncoder, RgbaImage};
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
use ratatui_image::{
    protocol::{kitty::Kitty, Protocol},
    Image,
};

use super::iso::px;
use crate::theme;

/// Image Kitty vivante : placeholders posés une fois (ratatui-image), puis
/// une nouvelle image PNG par frame sous le même identifiant.
pub struct View {
    id: u32,
    live: Option<(Rect, Protocol)>,
    pending: Option<String>,
}

impl View {
    /// Identifiant d'image tiré au hasard (24 bits, non nul).
    pub fn new() -> Self {
        Self {
            id: rand::random_range(1..0x00FF_FFFF),
            live: None,
            pending: None,
        }
    }

    /// Séquence Kitty de la dernière frame, à écrire après le rendu.
    pub fn take_pending(&mut self) -> Option<String> {
        self.pending.take()
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
    let mut img = RgbaImage::from_pixel(bw, bh, px(theme::VOID));
    let (dx, dy) = (
        (bw - cv.width() * scale) / 2,
        (bh - cv.height() * scale) / 2,
    );
    for (x, y, p) in cv.enumerate_pixels() {
        for k in 0..scale * scale {
            img.put_pixel(dx + x * scale + k % scale, dy + y * scale + k / scale, *p);
        }
    }
    match &view.live {
        Some((rect, proto)) if *rect == area => {
            Image::new(proto).render(area, buf);
            view.pending = kitty_png(view.id, &img);
        }
        _ => {
            let Ok(kitty) = Kitty::new(DynamicImage::ImageRgba8(img), area, view.id, false) else {
                return;
            };
            let proto = Protocol::Kitty(kitty);
            Image::new(&proto).render(area, buf);
            view.live = Some((area, proto));
        }
    }
}

/// Séquence Kitty « remplace l'image `id` » (PNG, placement virtuel,
/// silencieuse), découpée en morceaux de 4096 octets base64.
fn kitty_png(id: u32, img: &RgbaImage) -> Option<String> {
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Adaptive)
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

/// Base64 standard (RFC 4648, avec bourrage).
fn base64(bytes: &[u8]) -> String {
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
        let seq = kitty_png(4242, &img).unwrap();
        assert!(seq.starts_with("\x1b_Ga=T,f=100,U=1,q=2,i=4242,m="));
        assert!(seq.ends_with("\x1b\\"));
        assert_eq!(seq.matches("i=4242").count(), 1);
    }
}
