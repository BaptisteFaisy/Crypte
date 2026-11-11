//! Primitives de rasterisation du monde 3D.
//!
//! `canvas.rs` ne sait que poser des sprites opaques ; ici on compose des
//! surfaces translucides (dalles, arêtes, halos, brouillard, voile du panneau).
//! Tout travaille en `f32` écran et compose par-dessus le pixel existant.

use image::{Rgba, RgbaImage};

/// Pose une couleur avec une opacité, par-dessus le pixel existant.
pub fn over(cv: &mut RgbaImage, x: i64, y: i64, c: Rgba<u8>, a: f32) {
    let a = a.clamp(0.0, 1.0) * (c[3] as f32 / 255.0);
    if a <= 0.002 || x < 0 || y < 0 || x >= cv.width() as i64 || y >= cv.height() as i64 {
        return;
    }
    let p = cv.get_pixel(x as u32, y as u32);
    let mix = |i: usize| ((1.0 - a) * p[i] as f32 + a * c[i] as f32).round() as u8;
    cv.put_pixel(x as u32, y as u32, Rgba([mix(0), mix(1), mix(2), 255]));
}

/// Segment de 1 px (Bresenham), borné pour éviter toute boucle infinie.
pub fn line(cv: &mut RgbaImage, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgba<u8>, a: f32) {
    let (mut x, mut y) = (x0.round() as i64, y0.round() as i64);
    let (xe, ye) = (x1.round() as i64, y1.round() as i64);
    let dx = (xe - x).abs();
    let dy = -(ye - y).abs();
    let sx = if x < xe { 1 } else { -1 };
    let sy = if y < ye { 1 } else { -1 };
    let mut err = dx + dy;
    let limit = (dx.abs() + dy.abs()).max(1) * 4 + 8;
    for _ in 0..limit {
        over(cv, x, y, c, a);
        if x == xe && y == ye {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Remplit un quadrilatère convexe (les 4 sommets projetés d'une face).
pub fn quad(cv: &mut RgbaImage, p: [(f32, f32); 4], c: Rgba<u8>, a: f32) {
    if a <= 0.002 {
        return;
    }
    let (ymin, ymax) = span_y(&p);
    for y in ymin..=ymax {
        let fy = y as f32 + 0.5;
        let Some((xa, xb)) = span_x(&p, fy) else {
            continue;
        };
        for x in xa.round() as i64..=xb.round() as i64 {
            over(cv, x, y, c, a);
        }
    }
}

/// Les 4 arêtes d'un quadrilatère.
pub fn quad_edges(cv: &mut RgbaImage, p: [(f32, f32); 4], c: Rgba<u8>, a: f32) {
    for i in 0..4 {
        let j = (i + 1) % 4;
        line(cv, p[i].0, p[i].1, p[j].0, p[j].1, c, a);
    }
}

/// Halo radial additif (glow au sol, flash d'impact).
pub fn glow(cv: &mut RgbaImage, cx: f32, cy: f32, r: f32, c: Rgba<u8>, strength: f32) {
    if r <= 0.0 || strength <= 0.0 {
        return;
    }
    let (x0, x1) = ((cx - r) as i64, (cx + r) as i64 + 1);
    let (y0, y1) = ((cy - r) as i64, (cy + r) as i64 + 1);
    for y in y0.max(0)..y1.min(cv.height() as i64) {
        for x in x0.max(0)..x1.min(cv.width() as i64) {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / r;
            if d < 1.0 {
                let f = (1.0 - d) * (1.0 - d);
                over(cv, x, y, c, strength * f);
            }
        }
    }
}

/// Petit disque plein (points de la grille au sol, poussières).
pub fn dot(cv: &mut RgbaImage, cx: f32, cy: f32, r: f32, c: Rgba<u8>, a: f32) {
    glow(cv, cx, cy, r.max(0.6), c, a);
}

/// Anneau fin qui grandit (impact au sol).
pub fn ring(cv: &mut RgbaImage, cx: f32, cy: f32, r: f32, thick: f32, c: Rgba<u8>, a: f32) {
    if r <= 0.0 || a <= 0.0 {
        return;
    }
    let steps = ((r * 6.0) as usize).clamp(24, 900);
    for i in 0..steps {
        let t = i as f32 / steps as f32 * std::f32::consts::TAU;
        for k in 0..thick.max(1.0) as usize {
            let rr = r - k as f32;
            over(
                cv,
                (cx + t.cos() * rr) as i64,
                (cy + t.sin() * rr * 0.45) as i64,
                c,
                a,
            );
        }
    }
}

/// Facteur de visibilité après brouillard linéaire : 1 = net, 0 = fondu.
pub fn fog(dist: f32, near: f32, far: f32) -> f32 {
    if far <= near {
        return 1.0;
    }
    let t = ((dist - near) / (far - near)).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

/// Assombrit une zone (voile du panneau de chat, cuit dans l'image).
pub fn dim_rect(cv: &mut RgbaImage, x: i64, y: i64, w: i64, h: i64, a: f32) {
    let k = (1.0 - a.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    for yy in y.max(0)..(y + h).min(cv.height() as i64) {
        for xx in x.max(0)..(x + w).min(cv.width() as i64) {
            let p = cv.get_pixel(xx as u32, yy as u32);
            cv.put_pixel(
                xx as u32,
                yy as u32,
                Rgba([
                    (p[0] as f32 * k) as u8,
                    (p[1] as f32 * k) as u8,
                    (p[2] as f32 * k) as u8,
                    255,
                ]),
            );
        }
    }
}

/// Flou boîte séparable sur une zone (l'approximation du `backdrop-filter`).
pub fn blur_rect(cv: &mut RgbaImage, x: i64, y: i64, w: i64, h: i64, radius: u32) {
    if radius == 0 {
        return;
    }
    let (x0, y0) = (x.max(0), y.max(0));
    let (x1, y1) = (
        (x + w).min(cv.width() as i64),
        (y + h).min(cv.height() as i64),
    );
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let mut buf = region(cv, x0, y0, x1, y1);
    blur_pass(&mut buf, x1 - x0, y1 - y0, radius, true);
    blur_pass(&mut buf, x1 - x0, y1 - y0, radius, false);
    put_region(cv, &buf, x0, y0, x1, y1);
}

/// Vignette : assombrit les bords de l'image.
pub fn vignette(cv: &mut RgbaImage, strength: f32) {
    let (w, h) = (cv.width() as f32, cv.height() as f32);
    let max = ((w / 2.0).powi(2) + (h / 2.0).powi(2)).sqrt();
    for y in 0..cv.height() as i64 {
        for x in 0..cv.width() as i64 {
            let d = (((x as f32 - w / 2.0).powi(2) + (y as f32 - h / 2.0).powi(2)).sqrt()) / max;
            let a = strength * (d * d).clamp(0.0, 1.0);
            if a > 0.004 {
                over(cv, x, y, Rgba([0, 0, 0, 255]), a);
            }
        }
    }
}

/// Étendue verticale d'un quad (bornes entières, dans l'image).
fn span_y(p: &[(f32, f32); 4]) -> (i64, i64) {
    let ys: Vec<i64> = p.iter().map(|v| v.1.round() as i64).collect();
    (
        ys.iter().min().copied().unwrap_or(0),
        ys.iter().max().copied().unwrap_or(0),
    )
}

/// Intersection d'une scanline avec le contour du quad : `(x_min, x_max)`.
fn span_x(p: &[(f32, f32); 4], y: f32) -> Option<(f32, f32)> {
    let mut xs: Vec<f32> = Vec::with_capacity(4);
    for i in 0..4 {
        let (a, b) = (p[i], p[(i + 1) % 4]);
        let (lo, hi) = if a.1 <= b.1 { (a, b) } else { (b, a) };
        if y >= lo.1 && y < hi.1 && (hi.1 - lo.1).abs() > 1e-6 {
            let t = (y - lo.1) / (hi.1 - lo.1);
            xs.push(lo.0 + (hi.0 - lo.0) * t);
        }
    }
    if xs.len() < 2 {
        return None;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some((xs[0], xs[xs.len() - 1]))
}

/// Copie une zone rectangulaire dans un tampon plat RGBA.
fn region(cv: &RgbaImage, x0: i64, y0: i64, x1: i64, y1: i64) -> Vec<[f32; 3]> {
    let mut v = Vec::with_capacity(((x1 - x0) * (y1 - y0)) as usize * 3);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = cv.get_pixel(x as u32, y as u32);
            v.push([p[0] as f32, p[1] as f32, p[2] as f32]);
        }
    }
    v
}

/// Une passe de flou boîte (horizontale si `horizontal`).
fn blur_pass(buf: &mut [[f32; 3]], w: i64, h: i64, r: u32, horizontal: bool) {
    let (len, outer) = if horizontal { (w, h) } else { (h, w) };
    let src = buf.to_vec();
    let rad = r as i64;
    for o in 0..outer {
        for i in 0..len {
            let (mut s0, mut s1, mut s2, mut n) = (0.0_f32, 0.0, 0.0, 0.0);
            for k in -rad..=rad {
                let j = i + k;
                if j < 0 || j >= len {
                    continue;
                }
                let idx = idx_of(j, o, w, horizontal);
                s0 += src[idx][0];
                s1 += src[idx][1];
                s2 += src[idx][2];
                n += 1.0;
            }
            let dst = idx_of(i, o, w, horizontal);
            buf[dst] = [s0 / n, s1 / n, s2 / n];
        }
    }
}

/// Index dans le tampon plat selon l'axe de la passe.
fn idx_of(i: i64, o: i64, w: i64, horizontal: bool) -> usize {
    let (x, y) = if horizontal { (i, o) } else { (o, i) };
    (y * w + x) as usize
}

/// Recopie un tampon plat dans l'image.
fn put_region(cv: &mut RgbaImage, buf: &[[f32; 3]], x0: i64, y0: i64, x1: i64, y1: i64) {
    let mut i = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let v = buf[i];
            i += 1;
            cv.put_pixel(
                x as u32,
                y as u32,
                Rgba([v[0] as u8, v[1] as u8, v[2] as u8, 255]),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);
    const ORANGE: Rgba<u8> = Rgba([255, 91, 31, 255]);

    fn img(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([5, 5, 5, 255]))
    }

    fn lum(cv: &RgbaImage, x: u32, y: u32) -> u8 {
        cv.get_pixel(x, y)[0]
    }

    #[test]
    fn over_compose_selon_l_alpha() {
        let mut cv = img(4, 4);
        over(&mut cv, 1, 1, WHITE, 0.5);
        assert!(lum(&cv, 1, 1) > 100 && lum(&cv, 1, 1) < 200);
        over(&mut cv, 1, 1, WHITE, 0.0);
        assert_eq!(lum(&cv, 0, 0), 5);
    }

    #[test]
    fn line_trace_et_reste_bornée() {
        let mut cv = img(32, 32);
        line(&mut cv, 2.0, 2.0, 28.0, 20.0, WHITE, 1.0);
        assert_eq!(lum(&cv, 2, 2), 255);
        assert_eq!(lum(&cv, 28, 20), 255);
        line(&mut cv, -50.0, -50.0, 5000.0, 5000.0, WHITE, 1.0);
    }

    #[test]
    fn quad_remplit_sa_surface() {
        let mut cv = img(40, 40);
        quad(
            &mut cv,
            [(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)],
            ORANGE,
            1.0,
        );
        assert_eq!(lum(&cv, 20, 20), 255);
        assert_eq!(cv.get_pixel(20, 20)[1], 91);
        assert_eq!(lum(&cv, 2, 2), 5);
    }

    #[test]
    fn quad_vide_ne_dessine_rien() {
        let mut cv = img(20, 20);
        quad(&mut cv, [(0.0, 0.0); 4], WHITE, 0.0);
        assert_eq!(lum(&cv, 0, 0), 5);
    }

    #[test]
    fn halo_plus_fort_au_centre() {
        let mut cv = img(40, 40);
        glow(&mut cv, 20.0, 20.0, 12.0, WHITE, 0.8);
        assert!(lum(&cv, 20, 20) > lum(&cv, 28, 20));
        assert_eq!(lum(&cv, 1, 1), 5);
    }

    #[test]
    fn brouillard_fond_avec_la_distance() {
        assert_eq!(fog(0.0, 100.0, 1000.0), 1.0);
        assert_eq!(fog(2000.0, 100.0, 1000.0), 0.0);
        let mid = fog(550.0, 100.0, 1000.0);
        assert!(mid > 0.0 && mid < 1.0);
        assert_eq!(fog(10.0, 500.0, 100.0), 1.0);
    }

    #[test]
    fn voile_assombrit_et_flou_lisse() {
        let mut cv = img(40, 40);
        quad(
            &mut cv,
            [(5.0, 5.0), (35.0, 5.0), (35.0, 35.0), (5.0, 35.0)],
            WHITE,
            1.0,
        );
        dim_rect(&mut cv, 10, 10, 20, 20, 0.5);
        assert!(lum(&cv, 15, 15) < 200 && lum(&cv, 15, 15) > 60);
        let mut cv2 = img(40, 40);
        over(&mut cv2, 20, 20, WHITE, 1.0);
        blur_rect(&mut cv2, 0, 0, 40, 40, 2);
        assert!(lum(&cv2, 20, 20) < 255 && lum(&cv2, 19, 20) > 0);
    }

    #[test]
    fn anneau_et_vignette() {
        let mut cv = img(60, 60);
        ring(&mut cv, 30.0, 30.0, 20.0, 2.0, ORANGE, 1.0);
        assert!(cv.pixels().any(|p| p[0] > 200));
        vignette(&mut cv, 0.8);
        assert!(lum(&cv, 0, 0) < lum(&cv, 30, 30));
    }

    #[test]
    fn arêtes_d_un_quad() {
        let mut cv = img(40, 40);
        quad_edges(
            &mut cv,
            [(8.0, 8.0), (32.0, 8.0), (32.0, 32.0), (8.0, 32.0)],
            WHITE,
            1.0,
        );
        assert_eq!(lum(&cv, 20, 8), 255);
        assert_eq!(lum(&cv, 20, 20), 5);
    }
}
