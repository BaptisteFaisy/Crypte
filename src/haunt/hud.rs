//! HUD et écrans : bandeau (score, vies, sorte de tokens + tokens
//! envoyés, bougie), barre de vie de la Faucheuse, messages, écrans titre,
//! pause, mort, fin de partie, victoire. Police 3×5 du hub, agrandie par
//! facteur entier, titres en dégradé vertical avec ombre.

use image::{Rgba, RgbaImage};
use ratatui::style::Color;

use super::actors::{pill, token_colours};
use super::boss::HP;
use super::{Haunt, Phase, CANDLE, HZ, SUB, VIEW_H, VIEW_W};
use crate::canvas::{put, px};
use crate::hub::font;
use crate::theme;

/// Petit fantôme des vies (5×6).
const GHOST: [&str; 6] = [".###.", "#####", "#.#.#", "#####", "#####", "#.#.#"];

/// HUD puis, selon la phase, l'écran par-dessus.
pub fn draw(cv: &mut RgbaImage, game: &Haunt) {
    band(cv, game);
    if game.boss.is_some() {
        boss_bar(cv, game);
    }
    if let Some((text, _)) = game.toast {
        title(cv, 30, text, 2, (theme::CANDLE, theme::PUMPKIN));
    }
    match game.phase {
        Phase::Title => title_screen(cv, game),
        Phase::Paused => {
            dim(cv, 0.55);
            title(cv, 70, "PAUSED", 4, (theme::BONE, theme::SAND));
            line(cv, 104, "P OR ENTER TO RESUME", theme::SAND);
        }
        Phase::Over => end_screen(cv, game, "GAME OVER", "THE CRYPT KEEPS YOU"),
        Phase::Clear => end_screen(cv, game, "THE REAPER FALLS", "THE CRYPT SLEEPS... FOR NOW"),
        Phase::Play if u32::from(game.kiro.dead) > 12 * SUB => {
            title(cv, 70, "YOU ARE DUST", 3, (theme::BLOOD, theme::DRIED));
            line(cv, 100, game.cause, theme::SAND);
        }
        Phase::Play => {}
    }
}

/// Bandeau du haut : score, vies, tokens, bougie.
fn band(cv: &mut RgbaImage, game: &Haunt) {
    for y in 0..11 {
        for x in 0..VIEW_W {
            let d = cv.get_pixel(x, y).0;
            let k = |c: u8| (f32::from(c) * 0.35) as u8;
            cv.put_pixel(x, y, Rgba([k(d[0]), k(d[1]), k(d[2]), 255]));
        }
    }
    text(cv, 4, 3, &format!("SCORE {:06}", game.score), theme::BONE);
    for i in 0..game.lives {
        let colour = if i + 1 == game.lives && !game.kiro.shroud {
            theme::SPECTRE
        } else {
            theme::BONE
        };
        ghost(cv, 58 + i64::from(i) * 7, 3, px(colour));
    }
    let colours = token_colours(game.kiro.weapon);
    pill(cv, 118, 1, "TOK", colours);
    text(cv, 136, 3, &format!("SENT {:04}", game.sent), theme::SAND);
    candle(cv, game);
    if game.best > 0 {
        text(cv, 190, 3, &format!("BEST {:06}", game.best), theme::ASH);
    }
}

/// Bougie qui raccourcit + temps restant (m:ss).
fn candle(cv: &mut RgbaImage, game: &Haunt) {
    let secs = game.candle / HZ;
    let low = secs < 30;
    let wax = (game.candle * 7 / CANDLE) as i64;
    for dy in 0..=wax {
        put(cv, 284, 9 - dy, px(theme::BONE));
        put(cv, 285, 9 - dy, px(theme::SAND));
    }
    let beat = game.step / u64::from(SUB);
    let flame = if beat / 3 % 2 == 0 {
        theme::CANDLE
    } else {
        theme::PUMPKIN
    };
    put(cv, 284, 8 - wax, px(flame));
    let colour = if low && beat / 8 % 2 == 0 {
        theme::BLOOD
    } else {
        theme::BONE
    };
    text(
        cv,
        290,
        3,
        &format!("{}:{:02}", secs / 60, secs % 60),
        colour,
    );
}

/// Barre de vie de la Faucheuse, en bas de l'écran.
fn boss_bar(cv: &mut RgbaImage, game: &Haunt) {
    let Some(b) = &game.boss else {
        return;
    };
    let (x0, y0, w) = (92i64, 182i64, 168i64);
    text(cv, 62, y0, "REAPER", theme::BLOOD);
    let filled = i64::from(b.hp.max(0)) * w / i64::from(HP);
    for dx in -1..=w {
        for dy in -1..=5 {
            let edge = dx == -1 || dx == w || dy == -1 || dy == 5;
            let c = if edge {
                theme::BONE
            } else if dx < filled {
                theme::BLOOD
            } else {
                theme::DRIED
            };
            put(cv, x0 + dx, y0 + dy, px(c));
        }
    }
}

/// Écran titre, par-dessus le début du niveau.
fn title_screen(cv: &mut RgbaImage, game: &Haunt) {
    dim(cv, 0.5);
    title(cv, 30, "HAUNT", 6, (theme::BLOOD, theme::CANDLE));
    line(cv, 72, "LEVEL 1 - THE BONEYARD", theme::SAND);
    line(cv, 82, "VERY HARD. ONE SHROUD. THREE LIVES.", theme::ASH);
    line(cv, 110, "A D MOVE   W JUMP   S SINK", theme::BONE);
    line(cv, 120, "SPACE SEND TOKENS   P PAUSE", theme::BONE);
    if game.step / u64::from(10 * SUB) % 2 == 0 {
        title(cv, 142, "PRESS ENTER", 2, (theme::PUMPKIN, theme::CANDLE));
    }
}

/// Fin de partie ou victoire : titre, sous-titre, score, relance.
fn end_screen(cv: &mut RgbaImage, game: &Haunt, head: &str, sub: &str) {
    dim(cv, 0.6);
    let (scale, colours) = if game.phase == Phase::Clear {
        (2, (theme::ECTO, theme::BONE))
    } else {
        (4, (theme::BLOOD, theme::DRIED))
    };
    title(cv, 48, head, scale, colours);
    line(cv, 86, sub, theme::SAND);
    line(
        cv,
        104,
        &format!("SCORE {:06}   BEST {:06}", game.score, game.best),
        theme::BONE,
    );
    line(
        cv,
        114,
        &format!("TOKENS SENT {}", game.sent),
        theme::CANDLE,
    );
    if game.settle == 0 && game.step / u64::from(10 * SUB) % 2 == 0 {
        line(cv, 140, "ENTER TO RISE AGAIN", theme::PUMPKIN);
    }
}

/// Assombrit tout l'écran (sous un écran de texte).
fn dim(cv: &mut RgbaImage, k: f32) {
    for p in cv.pixels_mut() {
        for c in &mut p.0[..3] {
            *c = (f32::from(*c) * (1.0 - k)) as u8;
        }
    }
}

/// Titre centré à l'échelle `scale`, dégradé vertical, ombre d'encre.
fn title(cv: &mut RgbaImage, y: i64, text: &str, scale: i64, (from, to): (Color, Color)) {
    let x = (VIEW_W as i64 - font::width(text) * scale) / 2;
    let shadow = px(theme::INK);
    font::paint(
        cv,
        x + scale / 2 + 1,
        y + scale / 2 + 1,
        text,
        scale,
        |_| shadow,
    );
    font::paint(cv, x, y, text, scale, |row| {
        px(theme::lerp(from, to, row as f32 / 4.0))
    });
}

/// Ligne de texte centrée (échelle 1).
fn line(cv: &mut RgbaImage, y: i64, text: &str, colour: Color) {
    let x = (VIEW_W as i64 - font::width(text)) / 2;
    font::paint(cv, x + 1, y + 1, text, 1, |_| px(theme::INK));
    font::paint(cv, x, y, text, 1, |_| px(colour));
}

/// Texte à gauche (échelle 1).
fn text(cv: &mut RgbaImage, x: i64, y: i64, text: &str, colour: Color) {
    font::paint(cv, x, y, text, 1, |_| px(colour));
}

/// Petit fantôme (une vie).
fn ghost(cv: &mut RgbaImage, x: i64, y: i64, c: Rgba<u8>) {
    for (dy, row) in GHOST.iter().enumerate() {
        for (dx, ch) in row.chars().enumerate() {
            if ch == '#' && (y + dy as i64) < i64::from(VIEW_H) {
                put(cv, x + dx as i64, y + dy as i64, c);
            }
        }
    }
}
