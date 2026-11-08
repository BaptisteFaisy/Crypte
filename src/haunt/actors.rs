//! Acteurs à l'écran : cercueils flottants, butin, créatures (sortie de
//! terre découpée au sol, silhouette blanche quand elles encaissent,
//! fondu de la Dame blanche), Faucheuse, Kiro (linceul, clignotement,
//! enfoncé dans la terre, âme qui s'envole), flammes, tokens, projectiles,
//! particules.

use image::RgbaImage;

use super::boss::DYING;
use super::foes::{Foe, Kind, RISE};
use super::player::{DEATH, H, SUNK};
use super::shots::{BoltKind, Loot, Pickup, Shot, Weapon};
use super::sprites::Pair;
use super::{Haunt, DT, SUB};
use crate::canvas::{blit, put, px, Ink};
use crate::hub::font;
use crate::theme;

/// Tout ce qui bouge, de l'arrière vers l'avant.
pub fn draw(cv: &mut RgbaImage, game: &Haunt, cam: f32, top: f32) {
    for p in &game.platforms {
        let (x, y) = (p.x - cam, p.y - top);
        blit(cv, &game.sprites.raft, x as i64, y as i64, Ink::SOLID);
    }
    for p in &game.pickups {
        pickup(cv, game, p, cam, top);
    }
    for f in &game.foes {
        foe(cv, game, f, cam, top);
    }
    reaper(cv, game, cam, top);
    kiro(cv, game, cam, top);
    for f in &game.flames {
        let img = &game.sprites.flame[(u32::from(f.age) / (3 * SUB) % 2) as usize];
        let (x, y) = ((f.x - cam) as i64, (f.y - top) as i64 - 6);
        blit(cv, img, x - 4, y, Ink::SOLID);
        blit(cv, img, x, y - 1, Ink::SOLID);
    }
    for s in &game.shots {
        token(cv, s, cam, top);
    }
    for b in &game.bolts {
        let img = match b.kind {
            BoltKind::Wail => &game.sprites.wail,
            BoltKind::Scythe => &game.sprites.scythe[(u32::from(b.age) / (3 * SUB) % 4) as usize],
        };
        let (x, y) = (
            b.x - cam - img.width() as f32 / 2.0,
            b.y - top - img.height() as f32 / 2.0,
        );
        blit(cv, img, x as i64, y as i64, Ink::SOLID);
    }
    game.sparks.draw(cv, cam, top);
}

/// Pose `pair` (tourné vers `facing`) debout sur `feet`, centré en `cx`.
fn stand(cv: &mut RgbaImage, pair: &Pair, facing: f32, (cx, feet): (f32, f32), ink: Ink) {
    let img = pair.get(facing);
    let x = cx - img.width() as f32 / 2.0;
    let y = feet - (img.height() - pair.pad) as f32;
    blit(cv, img, x.round() as i64, y.round() as i64, ink);
}

/// Pose `pair` centré sur (cx, cy).
fn centre(cv: &mut RgbaImage, pair: &Pair, facing: f32, (cx, cy): (f32, f32), ink: Ink) {
    let img = pair.get(facing);
    let x = cx - img.width() as f32 / 2.0;
    let y = cy - img.height() as f32 / 2.0;
    blit(cv, img, x.round() as i64, y.round() as i64, ink);
}

/// Silhouette blanche les pas qui suivent un coup.
fn struck(flash: u8, alpha: f32) -> Ink {
    Ink {
        alpha,
        tint: (flash > 0).then(|| px(theme::BONE)),
        clip: i64::MAX,
    }
}

/// Une créature.
fn foe(cv: &mut RgbaImage, game: &Haunt, f: &Foe, cam: f32, top: f32) {
    let sp = &game.sprites;
    let (cx, cy) = (f.center().0 - cam, f.center().1 - top);
    let feet = f.body.feet() - top;
    let mut ink = struck(f.flash, f.alpha);
    let frame = (f.age / (5 * SUB) % 2) as usize;
    let pair = match f.kind {
        Kind::Bat => return centre(cv, &sp.bat[frame], f.facing, (cx, cy + 2.0), ink),
        Kind::Wisp => return centre(cv, &sp.wisp[frame], f.facing, (cx, cy), ink),
        Kind::Mummy => &sp.mummy,
        Kind::Runner => &sp.runner,
        Kind::Hand => &sp.hand,
        Kind::Lady => &sp.lady,
        Kind::Head => &sp.head,
        Kind::Coffin if f.open => &sp.coffin_open,
        Kind::Coffin => &sp.coffin,
    };
    let mut at = (cx, feet);
    if f.rise > 0 {
        ink.clip = feet.round() as i64;
        at.1 += f32::from(f.rise) / f32::from(RISE) * 11.0;
    } else if matches!(f.kind, Kind::Mummy | Kind::Runner) {
        at.1 -= (f.age / (6 * SUB) % 2) as f32;
    }
    stand(cv, pair, f.facing, at, ink);
}

/// La Faucheuse : tremble avant de plonger (yeux rouges), clignote à
/// l'agonie.
fn reaper(cv: &mut RgbaImage, game: &Haunt, cam: f32, top: f32) {
    let Some(b) = &game.boss else {
        return;
    };
    let mut x = b.x - cam;
    let mut ink = struck(b.flash, 1.0);
    let beat = game.step / u64::from(SUB);
    if b.winding() {
        x += if beat % 2 == 0 { 1.0 } else { -1.0 };
        if beat % 4 < 2 {
            ink.tint = Some(px(theme::BLOOD));
        }
    }
    if b.dying > 0 {
        ink.alpha = 1.0 - f32::from(b.dying) / f32::from(DYING);
        x += (u32::from(b.dying) / SUB % 3) as f32 - 1.0;
    }
    let img = game.sprites.reaper.get(b.facing);
    blit(cv, img, x.round() as i64, (b.y - top).round() as i64, ink);
}

/// Kiro : linceul ou nu, ondulation, clignote invulnérable, s'enfonce
/// dans la terre, âme effrayée qui monte à sa mort.
fn kiro(cv: &mut RgbaImage, game: &Haunt, cam: f32, top: f32) {
    let k = &game.kiro;
    let sp = &game.sprites;
    let (cx, feet) = (k.body.cx() - cam, k.body.feet() - top);
    if !k.alive() {
        let alpha = 1.0 - f32::from(k.dead) / f32::from(DEATH);
        return stand(cv, &sp.scared, k.facing, (cx, feet), Ink::alpha(alpha));
    }
    if k.hurt > 0 && u32::from(k.hurt) / (2 * SUB) % 2 == 1 {
        return;
    }
    let frame = (game.step / u64::from(6 * SUB) % 2) as usize;
    let pair = if k.shroud {
        &sp.shroud[frame]
    } else {
        &sp.kiro[frame]
    };
    let mut ink = Ink::SOLID;
    let mut at = (cx, feet);
    if k.sunk {
        ink.clip = feet.round() as i64;
        at.1 += H - SUNK;
    } else if k.body.ground {
        at.1 -= (game.step / u64::from(8 * SUB) % 2) as f32;
    }
    stand(cv, pair, k.facing, at, ink);
}

/// Butin : flotte, clignote avant de disparaître.
fn pickup(cv: &mut RgbaImage, game: &Haunt, p: &Pickup, cam: f32, top: f32) {
    let blink = u32::from(p.age) / (3 * SUB) % 2 == 0;
    if u32::from(p.age) > u32::from(Pickup::LIFE) - 60 * SUB && blink {
        return;
    }
    let sp = &game.sprites;
    let pair = match p.loot {
        Loot::Sharp => &sp.knife,
        Loot::Hot => &sp.lantern,
        Loot::Shroud => &sp.sheet,
        Loot::Candy => &sp.candy,
    };
    let bob = (f32::from(p.age) * DT * 0.15).sin() * 2.0 - 2.0;
    stand(cv, pair, 1.0, (p.x - cam, p.y - top + bob), Ink::SOLID);
}

/// Couleurs d'un token : fond, bord, texte.
pub fn token_colours(w: Weapon) -> [image::Rgba<u8>; 3] {
    let [a, b, c] = match w {
        Weapon::Token => [theme::CANDLE, theme::CANDLE_DARK, theme::INK],
        Weapon::Sharp => [theme::BLOOD, theme::DRIED, theme::BONE],
        Weapon::Hot => [theme::PUMPKIN, theme::PUMPKIN_DARK, theme::INK],
    };
    [px(a), px(b), px(c)]
}

/// Pastille de token en vol (traînée derrière elle).
fn token(cv: &mut RgbaImage, s: &Shot, cam: f32, top: f32) {
    let r = s.rect();
    let (x0, y0) = ((r.x - cam).round() as i64, (r.y - top).round() as i64);
    let back = -s.vx.signum() as i64;
    let [fill, edge, ink] = token_colours(s.weapon);
    for k in 1..4i64 {
        let tx = if back < 0 {
            x0 - k * 2
        } else {
            x0 + r.w as i64 + k * 2
        };
        put(cv, tx, y0 + 3, edge);
    }
    pill(cv, (x0, y0), s.word, [fill, edge, ink], 1);
}

/// Pastille arrondie (7 px de haut ×`k`) : bord, fond, mot en police
/// 3×5 (tokens en vol ×1, HUD ×2).
pub fn pill(
    cv: &mut RgbaImage,
    (x0, y0): (i64, i64),
    word: &str,
    [fill, edge, ink]: [image::Rgba<u8>; 3],
    k: i64,
) {
    let (w, h) = (font::width(word) + 4, 7i64);
    for dy in 0..h * k {
        for dx in 0..w * k {
            let (u, v) = (dx / k, dy / k);
            let (side, cap) = (u == 0 || u == w - 1, v == 0 || v == h - 1);
            if side && cap {
                continue;
            }
            put(cv, x0 + dx, y0 + dy, if side || cap { edge } else { fill });
        }
    }
    font::paint(cv, x0 + 2 * k, y0 + k, word, k, |_| ink);
}
