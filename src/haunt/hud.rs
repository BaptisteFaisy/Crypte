//! HUD et écrans (vue 960×576) : bandeau (score, vies, sorte de tokens +
//! tokens envoyés, bougie), barre de vie de la Faucheuse, messages,
//! écrans titre (Kiro et la Faucheuse du bestiaire en grand), pause,
//! mort, fin de partie, victoire. Police 3×5 du hub agrandie par facteur
//! entier, titres en dégradé vertical avec ombre.

use image::{Rgba, RgbaImage};
use ratatui::style::Color;

use super::actors::{pill, token_colours};
use super::boss::HP;
use super::{Haunt, Phase, CANDLE, HZ, SUB, VIEW_H, VIEW_W};
use crate::canvas::{blit, put, px, Ink};
use crate::hub::font;
use crate::theme;

/// Petit fantôme des vies (5×6, dessiné ×2).
const GHOST: [&str; 6] = [".###.", "#####", "#.#.#", "#####", "#####", "#.#.#"];
/// Hauteur du bandeau (pixels).
const BAND: u32 = 26;
/// Taille du texte du HUD.
const K: i64 = 2;

/// HUD puis, selon la phase, l'écran par-dessus.
pub fn draw(cv: &mut RgbaImage, game: &Haunt) {
    band(cv, game);
    if game.boss.is_some() {
        boss_bar(cv, game);
    }
    if let Some((text, _)) = game.toast {
        title(cv, 70, text, 4, (theme::CANDLE, theme::PUMPKIN));
    }
    match game.phase {
        Phase::Title => title_screen(cv, game),
        Phase::Paused => {
            dim(cv, 0.55);
            title(cv, 220, "PAUSED", 10, (theme::BONE, theme::SAND));
            line(cv, 300, "P OR ENTER TO RESUME", theme::SAND);
        }
        Phase::Over => end_screen(cv, game, "GAME OVER", "THE CRYPT KEEPS YOU"),
        Phase::Clear => end_screen(cv, game, "THE REAPER FALLS", "THE CRYPT SLEEPS... FOR NOW"),
        Phase::Play if u32::from(game.kiro.dead) > 12 * SUB => {
            title(cv, 220, "YOU ARE DUST", 6, (theme::BLOOD, theme::DRIED));
            line(cv, 280, game.cause, theme::SAND);
        }
        Phase::Play => {}
    }
}

/// Bandeau du haut : score, vies, tokens, meilleur score, bougie.
fn band(cv: &mut RgbaImage, game: &Haunt) {
    let raw: &mut [u8] = cv;
    for px in raw[..(BAND * VIEW_W * 4) as usize].chunks_exact_mut(4) {
        for c in &mut px[..3] {
            *c = (u32::from(*c) * 90 / 256) as u8;
        }
    }
    text(cv, 16, 8, &format!("SCORE {:06}", game.score), theme::BONE);
    for i in 0..game.lives {
        let bare = i + 1 == game.lives && !game.kiro.shroud;
        let colour = if bare { theme::SPECTRE } else { theme::BONE };
        ghost(cv, 132 + i64::from(i) * 14, 7, px(colour));
    }
    pill(cv, (330, 6), "TOK", token_colours(game.kiro.weapon), K);
    text(cv, 368, 8, &format!("SENT {:04}", game.sent), theme::SAND);
    if game.best > 0 {
        text(cv, 560, 8, &format!("BEST {:06}", game.best), theme::ASH);
    }
    candle(cv, game);
}

/// Bougie qui raccourcit + temps restant (m:ss).
fn candle(cv: &mut RgbaImage, game: &Haunt) {
    let secs = game.candle / HZ;
    let beat = game.step / u64::from(SUB);
    let wax = i64::from(game.candle * 14 / CANDLE);
    for dy in 0..=wax {
        for dx in 0..4 {
            let c = if dx < 2 { theme::BONE } else { theme::SAND };
            put(cv, 862 + dx, 21 - dy, px(c));
        }
    }
    let flame = if beat / 3 % 2 == 0 {
        theme::CANDLE
    } else {
        theme::PUMPKIN
    };
    for dx in 1..3 {
        put(cv, 862 + dx, 19 - wax, px(flame));
        put(cv, 862 + dx, 18 - wax, px(flame));
    }
    let low = secs < 30 && beat / 8 % 2 == 0;
    let colour = if low { theme::BLOOD } else { theme::BONE };
    text(
        cv,
        874,
        8,
        &format!("{}:{:02}", secs / 60, secs % 60),
        colour,
    );
}

/// Barre de vie de la Faucheuse, en bas de l'écran.
fn boss_bar(cv: &mut RgbaImage, game: &Haunt) {
    let Some(b) = &game.boss else {
        return;
    };
    let (x0, y0, w) = (300i64, 548i64, 360i64);
    text(cv, 210, y0, "REAPER", theme::BLOOD);
    let filled = i64::from(b.hp.max(0)) * w / i64::from(HP);
    for dx in -2..w + 2 {
        for dy in -2..10 {
            let edge = dx < 0 || dx >= w || !(0..8).contains(&dy);
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
    let sp = &game.sprites;
    blit(cv, sp.hero.get(1.0), 110, 120, Ink::SOLID);
    blit(cv, sp.boss_art.get(-1.0), 754, 120, Ink::SOLID);
    title(cv, 110, "HAUNT", 16, (theme::BLOOD, theme::CANDLE));
    line(cv, 226, "LEVEL 1 - THE BONEYARD", theme::SAND);
    line(
        cv,
        250,
        "VERY HARD. ONE SHROUD. THREE LIVES. ALL PLATFORMS.",
        theme::ASH,
    );
    line(cv, 330, "A D MOVE   W JUMP   S SINK", theme::BONE);
    line(cv, 354, "SPACE SEND TOKENS   P PAUSE", theme::BONE);
    if game.step / u64::from(10 * SUB) % 2 == 0 {
        title(cv, 420, "PRESS ENTER", 5, (theme::PUMPKIN, theme::CANDLE));
    }
}

/// Fin de partie ou victoire : titre, sous-titre, score, relance.
fn end_screen(cv: &mut RgbaImage, game: &Haunt, head: &str, sub: &str) {
    dim(cv, 0.6);
    let (scale, colours) = if game.phase == Phase::Clear {
        (6, (theme::ECTO, theme::BONE))
    } else {
        (10, (theme::BLOOD, theme::DRIED))
    };
    title(cv, 150, head, scale, colours);
    line(cv, 260, sub, theme::SAND);
    let scores = format!("SCORE {:06}   BEST {:06}", game.score, game.best);
    line(cv, 300, &scores, theme::BONE);
    line(
        cv,
        326,
        &format!("TOKENS SENT {}", game.sent),
        theme::CANDLE,
    );
    if game.settle == 0 && game.step / u64::from(10 * SUB) % 2 == 0 {
        line(cv, 400, "ENTER TO RISE AGAIN", theme::PUMPKIN);
    }
}

/// Assombrit tout l'écran (sous un écran de texte).
fn dim(cv: &mut RgbaImage, k: f32) {
    let keep = ((1.0 - k) * 256.0) as u32;
    let raw: &mut [u8] = cv;
    for px in raw.chunks_exact_mut(4) {
        for c in &mut px[..3] {
            *c = ((u32::from(*c) * keep) >> 8) as u8;
        }
    }
}

/// Titre centré à l'échelle `scale`, dégradé vertical, ombre d'encre.
fn title(cv: &mut RgbaImage, y: i64, text: &str, scale: i64, (from, to): (Color, Color)) {
    let x = (VIEW_W as i64 - font::width(text) * scale) / 2;
    let shadow = px(theme::INK);
    let off = (scale / 2).max(1);
    font::paint(cv, x + off, y + off, text, scale, |_| shadow);
    font::paint(cv, x, y, text, scale, |row| {
        px(theme::lerp(from, to, row as f32 / 4.0))
    });
}

/// Ligne de texte centrée (×2, ombrée).
fn line(cv: &mut RgbaImage, y: i64, text: &str, colour: Color) {
    let x = (VIEW_W as i64 - font::width(text) * K) / 2;
    font::paint(cv, x + 2, y + 2, text, K, |_| px(theme::INK));
    font::paint(cv, x, y, text, K, |_| px(colour));
}

/// Texte à gauche (×2).
fn text(cv: &mut RgbaImage, x: i64, y: i64, text: &str, colour: Color) {
    font::paint(cv, x, y, text, K, |_| px(colour));
}

/// Petit fantôme (une vie), ×2.
fn ghost(cv: &mut RgbaImage, x: i64, y: i64, c: Rgba<u8>) {
    for (dy, row) in GHOST.iter().enumerate() {
        for (dx, ch) in row.chars().enumerate() {
            if ch != '#' {
                continue;
            }
            let (gx, gy) = (x + dx as i64 * 2, y + dy as i64 * 2);
            for p in 0..4 {
                if gy + p / 2 < i64::from(VIEW_H) {
                    put(cv, gx + p % 2, gy + p / 2, c);
                }
            }
        }
    }
}
