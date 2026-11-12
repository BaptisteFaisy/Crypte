//! Post-traitement du monde 3D : bloom approché, poussière ambiante, piliers.
//!
//! Le prototype three.js utilise un `UnrealBloomPass` ; ici un bloom CPU
//! (downsample ×4, seuil, flou, recomposition additive) — l'équivalent le
//! plus proche sans GPU (voir le plan d'architecture).

use image::{Rgba, RgbaImage};

use super::proj::{project, CamParams, Vec3};
use super::raster;

/// Facteur de downsample du bloom.
const DS: u32 = 4;
/// Rayon du flou sur le tampon réduit.
const BLUR: i32 = 2;

/// Bloom approché : ce qui dépasse `threshold` de luminance irradie.
pub fn bloom(cv: &mut RgbaImage, strength: f32, threshold: f32) {
    let (w, h) = cv.dimensions();
    let (sw, sh) = (w / DS, h / DS);
    if sw < 4 || sh < 4 {
        return;
    }
    let mut buf = downsample(cv, sw, sh, threshold);
    blur3(&mut buf, sw, sh);
    blur3(&mut buf, sw, sh);
    upsample_add(cv, &buf, sw, sh, strength);
}

/// Réduit l'image et ne garde que ce qui dépasse le seuil (RGB linéaire).
fn downsample(cv: &RgbaImage, sw: u32, sh: u32, threshold: f32) -> Vec<[f32; 3]> {
    let (w, h) = cv.dimensions();
    let mut buf = vec![[0.0_f32; 3]; (sw * sh) as usize];
    for y in 0..sh {
        for x in 0..sw {
            let mut acc = [0.0_f32; 3];
            for dy in 0..DS {
                for dx in 0..DS {
                    let p = cv.get_pixel((x * DS + dx).min(w - 1), (y * DS + dy).min(h - 1));
                    let l = luma(p);
                    if l > threshold {
                        let k = l - threshold;
                        acc[0] = acc[0].max(p[0] as f32 * k);
                        acc[1] = acc[1].max(p[1] as f32 * k);
                        acc[2] = acc[2].max(p[2] as f32 * k);
                    }
                }
            }
            buf[(y * sw + x) as usize] = acc;
        }
    }
    buf
}

/// Luminance Rec.709 normalisée.
fn luma(p: &Rgba<u8>) -> f32 {
    (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0
}

/// Flou boîte 3×3 sur le tampon réduit.
fn blur3(buf: &mut [[f32; 3]], sw: u32, sh: u32) {
    let src = buf.to_vec();
    for y in 0..sh as i32 {
        for x in 0..sw as i32 {
            let mut acc = [0.0_f32; 3];
            let mut n = 0.0_f32;
            for dy in -BLUR..=BLUR {
                for dx in -BLUR..=BLUR {
                    let (xx, yy) = (x + dx, y + dy);
                    if xx < 0 || yy < 0 || xx >= sw as i32 || yy >= sh as i32 {
                        continue;
                    }
                    let s = src[(yy * sw as i32 + xx) as usize];
                    acc[0] += s[0];
                    acc[1] += s[1];
                    acc[2] += s[2];
                    n += 1.0;
                }
            }
            buf[(y * sw as i32 + x) as usize] = [acc[0] / n, acc[1] / n, acc[2] / n];
        }
    }
}

/// Recompose le tampon flouté, additivement, sur l'image d'origine.
fn upsample_add(cv: &mut RgbaImage, buf: &[[f32; 3]], sw: u32, sh: u32, strength: f32) {
    let h = cv.height();
    for y in 0..h {
        for x in 0..cv.width() {
            let b = buf[((y / DS).min(sh - 1) * sw + (x / DS).min(sw - 1)) as usize];
            if b[0] + b[1] + b[2] <= 0.01 {
                continue;
            }
            let p = cv.get_pixel(x, y);
            let add = |i: usize| (p[i] as f32 + strength * b[i]).min(255.0) as u8;
            cv.put_pixel(x, y, Rgba([add(0), add(1), add(2), 255]));
        }
    }
}

/// Poussière ambiante : points blancs épars dans le volume autour de la cible.
pub fn ambient_dust(cv: &mut RgbaImage, cam: &CamParams, w: u32, h: u32, seed: u64) {
    for j in 0..1000u64 {
        let s = seed.wrapping_add(j);
        let p = Vec3::new(
            spread(hash(s, 1), 4000.0) + cam.target.x,
            hash(s, 2) as f32 / u64::MAX as f32 * 900.0 + 20.0,
            spread(hash(s, 3), 4000.0) + cam.target.z,
        );
        if let Some((sx, sy, _)) = project(cam, p, w, h) {
            raster::over(cv, sx as i64, sy as i64, Rgba([245, 245, 243, 255]), 0.4);
        }
    }
}

/// Piliers très faints au loin (3-7 % d'opacité), fondus vers le haut.
pub fn pillars(cv: &mut RgbaImage, cam: &CamParams, w: u32, h: u32, seed: u64) {
    for j in 0..14u64 {
        let s = seed.wrapping_add(j * 7919);
        let base = Vec3::new(
            spread(hash(s, 11), 9000.0) + cam.target.x,
            0.0,
            spread(hash(s, 13), 9000.0) + cam.target.z,
        );
        let height = 300.0 + hash(s, 17) as f32 / u64::MAX as f32 * 600.0;
        let half = 20.0 + hash(s, 19) as f32 / u64::MAX as f32 * 40.0;
        let a = 0.03 + hash(s, 23) as f32 / u64::MAX as f32 * 0.04;
        let (Some(b0), Some(b1), Some(t0), Some(t1)) = (
            project(cam, base + Vec3::new(-half, 0.0, 0.0), w, h),
            project(cam, base + Vec3::new(half, 0.0, 0.0), w, h),
            project(cam, base + Vec3::new(half, height, 0.0), w, h),
            project(cam, base + Vec3::new(-half, height, 0.0), w, h),
        ) else {
            continue;
        };
        raster::quad(
            cv,
            [(b0.0, b0.1), (b1.0, b1.1), (t0.0, t0.1), (t1.0, t1.1)],
            Rgba([245, 245, 243, 255]),
            a,
        );
    }
}

/// Répartit un hash sur `[-range, range]`.
fn spread(h: u64, range: f32) -> f32 {
    (h as f32 / u64::MAX as f32 * 2.0 - 1.0) * range
}

/// Hash entier déterministe.
fn hash(x: u64, y: u64) -> u64 {
    let mut h = x ^ y.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^ (h >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([5, 5, 5, 255]))
    }

    #[test]
    fn bloom_fait_irradier_un_point_clair() {
        let mut cv = img(64, 64);
        cv.put_pixel(32, 32, Rgba([255, 255, 255, 255]));
        bloom(&mut cv, 0.7, 0.62);
        assert!(cv.get_pixel(36, 32)[0] > 5, "voisin illuminé");
    }

    #[test]
    fn bloom_neutre_sur_une_image_sombre() {
        let mut cv = img(64, 64);
        let before = cv.clone();
        bloom(&mut cv, 0.7, 0.62);
        assert_eq!(cv.as_raw(), before.as_raw());
    }

    #[test]
    fn poussiere_et_piliers_deterministes() {
        let cam = CamParams {
            target: Vec3::default(),
            dist: 1500.0,
            yaw: 0.6,
            pitch: 0.4,
            fov: 42.0_f32.to_radians(),
        };
        let mut a = img(160, 90);
        let mut b = img(160, 90);
        ambient_dust(&mut a, &cam, 160, 90, 42);
        ambient_dust(&mut b, &cam, 160, 90, 42);
        assert_eq!(a.as_raw(), b.as_raw());
        pillars(&mut a, &cam, 160, 90, 7);
        pillars(&mut b, &cam, 160, 90, 7);
        assert_eq!(a.as_raw(), b.as_raw());
        assert!(a.pixels().any(|p| p[0] > 5));
    }

    #[test]
    fn luma_rec709() {
        assert!((luma(&Rgba([255, 255, 255, 255])) - 1.0).abs() < 1e-3);
        assert!(luma(&Rgba([0, 0, 0, 255])) < 1e-3);
    }
}
