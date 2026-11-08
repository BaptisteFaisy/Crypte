//! Peinture partagée sur canevas RGBA (hub F7, haunt F9) : couleur de
//! thème → pixel, pose bornée, collage de sprite (opacité, teinte unie,
//! découpe sous une ligne). Jamais de filtre : chaque pixel reste net.

use image::{Rgba, RgbaImage};
use ratatui::style::Color;

/// Couleur de thème → pixel opaque (noir hors RGB).
pub fn px(c: Color) -> Rgba<u8> {
    match c {
        Color::Rgb(r, g, b) => Rgba([r, g, b, 255]),
        _ => Rgba([0, 0, 0, 255]),
    }
}

/// Pose un pixel s'il est dans le canevas.
pub fn put(cv: &mut RgbaImage, x: i64, y: i64, c: Rgba<u8>) {
    if x >= 0 && y >= 0 && (x as u32) < cv.width() && (y as u32) < cv.height() {
        cv.put_pixel(x as u32, y as u32, c);
    }
}

/// Réglages d'un collage : opacité (0..=1), teinte unie (silhouette d'un
/// coup reçu), lignes du canevas à partir de `clip` non dessinées (une
/// créature qui sort de terre).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ink {
    pub alpha: f32,
    pub tint: Option<Rgba<u8>>,
    pub clip: i64,
}

impl Ink {
    /// Collage opaque, sans teinte ni découpe.
    pub const SOLID: Ink = Ink {
        alpha: 1.0,
        tint: None,
        clip: i64::MAX,
    };

    /// Collage d'opacité `alpha`.
    pub fn alpha(alpha: f32) -> Ink {
        Ink {
            alpha,
            ..Ink::SOLID
        }
    }
}

/// Colle `img` en (x0, y0) : pixels transparents ignorés, mélange
/// linéaire avec le fond selon `ink.alpha`, résultat opaque.
pub fn blit(cv: &mut RgbaImage, img: &RgbaImage, x0: i64, y0: i64, ink: Ink) {
    let alpha = ink.alpha.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return;
    }
    let (w, h) = (i64::from(cv.width()), i64::from(cv.height()));
    for (x, y, p) in img.enumerate_pixels() {
        let (cx, cy) = (x0 + i64::from(x), y0 + i64::from(y));
        if p.0[3] == 0 || cx < 0 || cy < 0 || cx >= w || cy >= h || cy >= ink.clip {
            continue;
        }
        let src = ink.tint.unwrap_or(*p).0;
        if alpha >= 1.0 {
            cv.put_pixel(cx as u32, cy as u32, Rgba([src[0], src[1], src[2], 255]));
            continue;
        }
        let d = cv.get_pixel(cx as u32, cy as u32).0;
        let mix =
            |i: usize| (f32::from(d[i]) + (f32::from(src[i]) - f32::from(d[i])) * alpha) as u8;
        cv.put_pixel(cx as u32, cy as u32, Rgba([mix(0), mix(1), mix(2), 255]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blit_respects_alpha_tint_and_clip() {
        let mut cv = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 255]));
        let img = RgbaImage::from_pixel(2, 2, Rgba([200, 100, 50, 255]));
        blit(&mut cv, &img, 0, 0, Ink::SOLID);
        assert_eq!(cv.get_pixel(1, 1).0, [200, 100, 50, 255]);
        blit(&mut cv, &img, 2, 0, Ink::alpha(0.5));
        assert_eq!(cv.get_pixel(2, 0).0, [100, 50, 25, 255]);
        let white = Rgba([255, 255, 255, 255]);
        let ink = Ink {
            tint: Some(white),
            clip: 3,
            ..Ink::SOLID
        };
        blit(&mut cv, &img, 0, 2, ink);
        assert_eq!(*cv.get_pixel(0, 2), white);
        assert_eq!(cv.get_pixel(0, 3).0, [0, 0, 0, 255], "sous la découpe");
    }

    #[test]
    fn put_ignores_out_of_bounds() {
        let mut cv = RgbaImage::new(2, 2);
        put(&mut cv, -1, 0, Rgba([9, 9, 9, 255]));
        put(&mut cv, 0, 5, Rgba([9, 9, 9, 255]));
        assert!(cv.pixels().all(|p| p.0[3] == 0));
    }
}
