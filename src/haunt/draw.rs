//! Rendu : compose le canevas 960×576 (ciel précalculé, plans en
//! parallaxe, niveau éclairé sur son calque, mares d'ectoplasme
//! lumineuses, acteurs, brume, vignette, HUD) puis le présente.
//! - Kitty (Ghostty) : ×k **entier** au plus proche voisin (souvent ×1,
//!   ×2 en HiDPI), image renvoyée à chaque frame sous le même
//!   identifiant (`hub::live`), sur les seules cellules qui la portent.
//! - Ailleurs ou trop petit : réduit au plus proche voisin.
//!
//! Les tampons (`Painter`) sont réutilisés d'une frame à l'autre : à
//! 240 images/s, ni allocation ni ciel recalculé.

use image::{imageops, RgbaImage};
use ratatui::{buffer::Buffer, layout::Rect};

use super::backdrop::{self, hash, Light, Sky};
use super::level::{Tile, TILE};
use super::{actors, hud, Haunt, DT, SUB, VIEW_H, VIEW_W};
use crate::canvas::{blit, px, Ink};
use crate::hub::{live::present_kitty, render::to_cells};
use crate::theme;

/// Tampons de rendu réutilisés : ciel fixe, calque du niveau, éclairement,
/// canevas final.
#[derive(Default)]
pub struct Painter {
    sky: Option<Sky>,
    world: RgbaImage,
    factors: Vec<f32>,
    cv: RgbaImage,
}

/// Dessine le jeu dans `area` ; `cell` = taille de cellule en pixels si
/// le terminal parle Kitty (sinon demi-blocs).
pub fn render(game: &mut Haunt, area: Rect, buf: &mut Buffer, cell: Option<(u16, u16)>) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    paint(game);
    match cell {
        Some(cell) => kitty(game, area, buf, cell),
        None => {
            let (w, h) = (u32::from(area.width), u32::from(area.height) * 2);
            to_cells(&fit_down(&game.painter.cv, w, h), area, buf);
        }
    }
}

/// Canevas complet d'une frame, copié (`--haunt-shot`, tests).
pub fn snapshot(game: &mut Haunt) -> RgbaImage {
    paint(game);
    game.painter.cv.clone()
}

/// Kitty : agrandi ×k entier sur les seules cellules qui le contiennent
/// (centrées) ; le reste de la zone garde le fond de la crypte.
fn kitty(game: &mut Haunt, area: Rect, buf: &mut Buffer, (fw, fh): (u16, u16)) {
    let (fw, fh) = (u32::from(fw.max(1)), u32::from(fh.max(1)));
    let (bw, bh) = (u32::from(area.width) * fw, u32::from(area.height) * fh);
    let scale = (bw / VIEW_W).min(bh / VIEW_H);
    let view = &mut game.view;
    if scale == 0 {
        let small = fit_down(&game.painter.cv, bw, bh);
        return present_kitty(view, &small, 1, (bw, bh), area, buf);
    }
    let cols = (VIEW_W * scale).div_ceil(fw).min(u32::from(area.width)) as u16;
    let rows = (VIEW_H * scale).div_ceil(fh).min(u32::from(area.height)) as u16;
    let sub = Rect::new(
        area.x + (area.width - cols) / 2,
        area.y + (area.height - rows) / 2,
        cols,
        rows,
    );
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.reset();
                c.set_bg(theme::VOID);
            }
        }
    }
    let size = (u32::from(cols) * fw, u32::from(rows) * fh);
    present_kitty(view, &game.painter.cv, scale, size, sub, buf);
}

/// Compose la frame dans `game.painter.cv`.
fn paint(game: &mut Haunt) {
    let mut p = std::mem::take(&mut game.painter);
    p.compose(game);
    game.painter = p;
}

impl Painter {
    /// Ciel, plans, niveau éclairé, ectoplasme, acteurs, brume, HUD.
    fn compose(&mut self, game: &Haunt) {
        let (cam, top) = shake(game);
        let flash = game.flash > 0;
        let sky = self.sky.get_or_insert_with(Sky::new);
        if self.cv.dimensions() != (VIEW_W, VIEW_H) {
            self.cv = RgbaImage::new(VIEW_W, VIEW_H);
            self.world = RgbaImage::new(VIEW_W, VIEW_H);
        }
        sky.paint(&mut self.cv, flash);
        backdrop::layers(&mut self.cv, &game.sprites, cam, game.step);
        self.world.fill(0);
        decor(&mut self.world, game, cam, top);
        tiles(&mut self.world, game, cam, top);
        let lights = lights(game, cam, top);
        backdrop::light(&mut self.world, &lights, flash, &mut self.factors);
        overlay(&mut self.cv, &self.world);
        ecto(&mut self.cv, game, cam, top);
        actors::draw(&mut self.cv, game, cam, top);
        let mist = theme::SPECTRE_DARK;
        backdrop::fog(&mut self.cv, cam, game.step, 470..VIEW_H, 0.12, mist);
        hud::draw(&mut self.cv, game);
    }
}

/// Copie les pixels peints du calque (opaques) sur le canevas.
fn overlay(cv: &mut RgbaImage, layer: &RgbaImage) {
    let dst: &mut [u8] = cv;
    for (d, s) in dst.chunks_exact_mut(4).zip(layer.chunks_exact(4)) {
        if s[3] != 0 {
            d.copy_from_slice(s);
        }
    }
}

/// Vue secouée : (bord gauche, bord haut) du monde à l'écran.
fn shake(game: &Haunt) -> (f32, f32) {
    let cam = game.cam.round();
    if game.shake == 0 {
        return (cam, 0.0);
    }
    let h = hash((game.step / 2) as i64, 3);
    let amp = (u32::from(game.shake) / SUB).clamp(1, 4) as f32;
    let dx = (h % 3) as f32 - 1.0;
    let dy = ((h >> 8) % 3) as f32 - 1.0;
    (cam + dx * amp * 0.5, dy * amp * 0.5)
}

/// Colonnes de dalles visibles.
fn visible(cam: f32) -> std::ops::RangeInclusive<i64> {
    let first = (cam / TILE).floor() as i64;
    first..=first + (VIEW_W as f32 / TILE) as i64 + 1
}

/// Dalles visibles (l'ectoplasme est peint à part, lumineux).
fn tiles(layer: &mut RgbaImage, game: &Haunt, cam: f32, top: f32) {
    let level = &game.level;
    for c in visible(cam) {
        for r in 0..level.rows as i64 {
            let i = match level.tile(c, r) {
                Tile::Earth if !level.tile(c, r - 1).solid() => 0,
                Tile::Earth if hash(c, r) % 7 == 0 => 5,
                Tile::Earth => 1,
                Tile::Slab => 2,
                Tile::Brick => 3,
                Tile::Spikes => 4,
                Tile::Air | Tile::Ecto => continue,
            };
            let (x, y) = (c as f32 * TILE - cam, r as f32 * TILE - top);
            blit(
                layer,
                &game.sprites.tiles[i],
                x as i64,
                y as i64,
                Ink::SOLID,
            );
        }
    }
}

/// Décor posé : tombes, arbres morts, crânes, citrouilles, lanternes de
/// reprise (éteintes tant qu'on ne les a pas atteintes).
fn decor(layer: &mut RgbaImage, game: &Haunt, cam: f32, top: f32) {
    let sp = &game.sprites;
    for m in &game.level.marks {
        if m.x < cam - 20.0 || m.x > cam + VIEW_W as f32 + 20.0 {
            continue;
        }
        let (pair, ink) = match m.ch {
            't' => (&sp.tombs[(m.x / TILE) as usize % 3], Ink::SOLID),
            'y' => (&sp.tree, Ink::SOLID),
            'x' => (&sp.skull, Ink::SOLID),
            'o' => (&sp.pumpkin, Ink::SOLID),
            'k' if m.x <= game.checkpoint => (&sp.lantern, Ink::SOLID),
            'k' => (&sp.lantern, Ink::alpha(0.35)),
            _ => continue,
        };
        let facing = if hash(m.x as i64, 1) % 2 == 0 {
            1.0
        } else {
            -1.0
        };
        let img = pair.get(facing);
        let x = m.x - img.width() as f32 / 2.0 - cam;
        let y = m.y - (img.height() - pair.pad) as f32 - top;
        blit(layer, img, x as i64, y as i64, ink);
    }
}

/// Sources de lumière à l'écran : Kiro, citrouilles, lanternes allumées,
/// flammes.
fn lights(game: &Haunt, cam: f32, top: f32) -> Vec<Light> {
    let beat = game.step / u64::from(2 * SUB);
    let flicker = (hash(beat as i64, 9) % 100) as f32 / 100.0 * 0.12;
    let (kx, ky) = game.kiro.center();
    let mut out = vec![Light {
        x: kx - cam,
        y: ky - top,
        r: 72.0,
        k: 0.62,
    }];
    for m in &game.level.marks {
        let lit = m.ch == 'o' || (m.ch == 'k' && m.x <= game.checkpoint);
        if lit && m.x > cam - 40.0 && m.x < cam + VIEW_W as f32 + 40.0 {
            let (x, y, r, k) = (m.x - cam, m.y - 5.0 - top, 38.0, 0.5 + flicker);
            out.push(Light { x, y, r, k });
        }
    }
    for f in &game.flames {
        let (x, y, r, k) = (f.x - cam, f.y - 3.0 - top, 26.0, 0.6 + flicker);
        out.push(Light { x, y, r, k });
    }
    out
}

/// Mares d'ectoplasme : surface ondulante, profondeur sombre, bulles.
fn ecto(cv: &mut RgbaImage, game: &Haunt, cam: f32, top: f32) {
    let level = &game.level;
    let t = game.step as f32 * DT * 0.12;
    for c in visible(cam) {
        for r in 0..level.rows as i64 {
            if level.tile(c, r) != Tile::Ecto {
                continue;
            }
            let surface = level.tile(c, r - 1) != Tile::Ecto;
            for dx in 0..TILE as i64 {
                let wx = c * TILE as i64 + dx;
                let wave = if surface {
                    1.5 + ((wx as f32) * 0.5 + t).sin()
                } else {
                    0.0
                };
                let x = wx as f32 - cam;
                pool_column(cv, x as i64, r, wave, top, game.step, wx);
            }
        }
    }
}

/// Une colonne de pixels d'une dalle d'ectoplasme (`wave` > 0 : dalle
/// de surface, dont le haut ondule) ; une colonne sur onze fait monter
/// une bulle.
fn pool_column(cv: &mut RgbaImage, x: i64, r: i64, wave: f32, top: f32, step: u64, wx: i64) {
    if x < 0 || x >= VIEW_W as i64 {
        return;
    }
    let surface = wave > 0.0;
    let rise = (step / u64::from(SUB) + hash(wx, 6)) % 30;
    let bubble = (hash(wx, 5) % 11 == 0).then_some(30 - rise as i64);
    for dy in 0..TILE as i64 {
        let depth = if surface {
            dy as f32 - wave
        } else {
            dy as f32 + TILE - 1.0
        };
        if depth < 0.0 {
            continue;
        }
        let c = if depth < 1.0 {
            theme::lerp(theme::ECTO, theme::BONE, 0.25)
        } else if bubble == Some(depth as i64) {
            theme::ECTO
        } else {
            theme::lerp(
                theme::ECTO,
                theme::ECTO_DARK,
                (depth / 12.0).min(1.0) * 0.85,
            )
        };
        let y = (r as f32 * TILE + dy as f32 - top) as i64;
        if y >= 0 && y < VIEW_H as i64 {
            cv.put_pixel(x as u32, y as u32, px(c));
        }
    }
}

/// Réduction au plus proche voisin dans exactement `w`×`h`, proportions
/// gardées, centrée sur le fond de la crypte (terminal trop petit,
/// demi-blocs).
fn fit_down(cv: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let (w, h) = (w.max(1), h.max(1));
    let f = (w as f32 / VIEW_W as f32).min(h as f32 / VIEW_H as f32);
    let (sw, sh) = (
        ((VIEW_W as f32 * f) as u32).clamp(1, w),
        ((VIEW_H as f32 * f) as u32).clamp(1, h),
    );
    let small = imageops::resize(cv, sw, sh, imageops::FilterType::Nearest);
    let mut out = RgbaImage::from_pixel(w, h, px(theme::VOID));
    let (dx, dy) = ((w - sw) / 2, (h - sh) / 2);
    imageops::overlay(&mut out, &small, i64::from(dx), i64::from(dy));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::haunt::Phase;
    use std::collections::HashSet;
    use std::time::Instant;

    /// Couleurs distinctes d'un canevas (un écran vide n'en a que 2 ou 3).
    fn colours(cv: &RgbaImage) -> usize {
        cv.pixels().map(|p| p.0).collect::<HashSet<_>>().len()
    }

    #[test]
    fn every_phase_composes_a_rich_frame() {
        let mut game = Haunt::new();
        for phase in [
            Phase::Title,
            Phase::Play,
            Phase::Paused,
            Phase::Over,
            Phase::Clear,
        ] {
            game.phase = phase;
            game.tick();
            let cv = snapshot(&mut game);
            assert_eq!(cv.dimensions(), (VIEW_W, VIEW_H));
            assert!(colours(&cv) > 40, "{phase:?} : {} couleurs", colours(&cv));
        }
    }

    #[test]
    fn renders_at_any_size_kitty_or_halfblocks() {
        let mut game = Haunt::new();
        for (w, h) in [(1, 1), (20, 6), (68, 13), (118, 32), (300, 90)] {
            let area = Rect::new(1, 1, w, h);
            let mut buf = Buffer::empty(Rect::new(0, 0, w + 2, h + 2));
            render(&mut game, area, &mut buf, None);
            let painted = (0..w).filter(|&x| buf[(x + 1, 1 + h / 2)].bg != buf[(0, 0)].bg);
            assert!(painted.count() > 0, "{w}x{h} : rien en demi-blocs");
            for cell in [(10, 20), (20, 40), (7, 15)] {
                render(&mut game, area, &mut buf, Some(cell));
            }
        }
    }

    #[test]
    fn fit_down_keeps_proportions_without_smoothing() {
        let cv = snapshot(&mut Haunt::new());
        let out = fit_down(&cv, 160, 200);
        assert_eq!(out.dimensions(), (160, 200));
        let src: HashSet<_> = cv.pixels().map(|p| p.0).collect();
        let void = px(theme::VOID).0;
        let invented = out.pixels().any(|p| p.0 != void && !src.contains(&p.0));
        assert!(!invented, "aucune couleur inventée");
    }

    /// Budget d'une frame (composition + agrandissement + PNG Kitty) :
    /// `cargo test --release -- --ignored frame_budget --nocapture`.
    #[test]
    #[ignore]
    fn frame_budget() {
        let mut game = Haunt::new();
        game.new_run();
        for (label, cells, cell) in [
            ("118x32 @10x20", (118, 32), (10, 20)),
            ("118x32 @20x40 (HiDPI)", (118, 32), (20, 40)),
        ] {
            let area = Rect::new(0, 0, cells.0, cells.1);
            let mut buf = Buffer::empty(area);
            let (n, mut shown) = (240, 0);
            let t = Instant::now();
            for _ in 0..n {
                game.tick();
                render(&mut game, area, &mut buf, Some(cell));
                shown += usize::from(game.view.take_pending().is_some());
            }
            let secs = t.elapsed().as_secs_f64();
            let (drawn, sent) = (n as f64 / secs, shown as f64 / secs);
            println!("{label}: {drawn:.0} frames/s composées, {sent:.0} images/s envoyées");
        }
    }
}
