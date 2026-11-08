//! Rendu de Kiro en balade : sprite 32×32 via [`ImageBank`] (Kitty dans
//! Ghostty, protégé des effets), bulle au-dessus (ou dessous) qui pointe
//! vers lui, étoiles qui tournent autour de la tête quand il est frappé
//! (cachées derrière lui), « BONK! » du côté du coup. Rien n'est jamais
//! écrit sur les cellules du sprite (placeholders Kitty).

use std::f32::consts::TAU;

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget},
    Frame,
};

use super::{lines, Mood, Phantom, Tone};
use crate::bestiary::SPRITE;
use crate::chat::wrap_text;
use crate::gfx::ImageBank;
use crate::theme::{self, ASH, BLOOD, BONE, CANDLE, INK, SPECTRE};

/// Sprite en demi-blocs (sans Kitty) : 16×8 cellules = 16×16 px.
pub(super) const HALFBLOCK_CELLS: (u16, u16) = (16, 8);
/// Hauteur visée du sprite en rangées de texte (Kitty).
const ROWS_TALL: u32 = 4;
/// Bulle : largeur de texte max, lignes max.
const BUBBLE_W: u16 = 36;
const BUBBLE_LINES: usize = 5;
/// Étoiles du coup (tournent autour de la tête).
const STARS: [&str; 3] = ["✦", "✧", "★"];
/// Rangées laissées aux étoiles entre la bulle et Kiro (coup, K.O.).
const STARS_GAP: u16 = 2;

impl Phantom {
    /// Nouvelle frame : oublie les zones cliquables (rien hors écran chat).
    pub fn begin_frame(&mut self) {
        self.body = None;
        self.bubble = None;
    }

    /// Dessine Kiro dans `arena` (sidebar + discussion) : sprite, bulle
    /// (protégée des effets), étoiles, « BONK! ». Mémorise les zones
    /// cliquables (`hit_test`). Éteint ou arène trop petite : rien.
    pub fn draw(&mut self, frame: &mut Frame, arena: Rect, bank: &mut ImageBank) {
        self.arena = Some(arena);
        if !self.on {
            return;
        }
        self.size = sprite_cells(bank.kitty_cell());
        let Some(body) = self.place(arena) else {
            return;
        };
        bank.draw(frame, body, self.creature());
        self.body = Some(body);
        let buf = frame.buffer_mut();
        let bubble = self.draw_bubble(buf, body, arena);
        if let Some(rect) = bubble {
            bank.protect(rect);
        }
        self.bubble = bubble;
        let keep = [Some(body), bubble];
        self.draw_stars(buf, body, arena, &keep);
        self.draw_bonk(buf, body, arena, &keep);
    }

    /// Rectangle du sprite (absolu), Kiro gardé dans l'arène ; apparaît
    /// au centre la première fois. `None` si l'arène est trop petite.
    fn place(&mut self, arena: Rect) -> Option<Rect> {
        let (mx, _, floor) = self.limits()?;
        if !self.spawned {
            self.spawn();
        }
        self.x = self.x.clamp(0.0, mx);
        self.y = self.y.clamp(0.0, floor);
        let x = (self.x.round() as i32 + self.shake()).clamp(0, mx as i32) as u16;
        let (w, h) = self.size;
        Some(Rect::new(
            arena.x + x,
            arena.y + self.y.round() as u16,
            w,
            h,
        ))
    }

    /// Texte et ton de la bulle : réplique en cours, sinon « hm... »
    /// animé pendant qu'une critique est en vol.
    fn bubble_text(&self) -> Option<(String, Tone)> {
        if let Some(s) = &self.speech {
            return Some((s.text.clone(), s.tone));
        }
        let dots = lines::THINKING[(self.tick / 3) as usize % lines::THINKING.len()];
        self.pending.then(|| (dots.to_string(), Tone::Mumble))
    }

    /// Bulle encadrée « Kiro », pointe vers lui ; rend son rectangle.
    fn draw_bubble(&self, buf: &mut Buffer, body: Rect, arena: Rect) -> Option<Rect> {
        let (text, tone) = self.bubble_text()?;
        let width = arena.width.saturating_sub(4).min(BUBBLE_W);
        if width < 8 {
            return None;
        }
        let rows = bubble_lines(&text, usize::from(width));
        let text_w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
        let (w, h) = (text_w.max(4) as u16 + 4, rows.len() as u16 + 2);
        let gap = match self.mood {
            Mood::Hurt(_) | Mood::Ko(_) => STARS_GAP,
            _ => 0,
        };
        let (rect, above) = bubble_rect(body, arena, (w, h), gap)?;
        let colour = tone_colour(tone);
        Clear.render(rect, buf);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(colour))
            .style(Style::new().bg(INK))
            .title(theme::title("Kiro", colour, BONE));
        let inner = block.inner(rect);
        block.render(rect, buf);
        let mut style = Style::new().fg(BONE).bg(INK);
        if tone == Tone::Critique {
            style = style.add_modifier(Modifier::ITALIC);
        }
        let padded = Rect {
            x: inner.x + 1,
            width: inner.width.saturating_sub(2),
            ..inner
        };
        let text: Vec<Line> = rows.into_iter().map(Line::from).collect();
        Paragraph::new(text).style(style).render(padded, buf);
        tail(buf, rect, body, above, colour);
        Some(rect)
    }

    /// Frappé ou K.O. : 3 étoiles tournent autour de la tête (au-dessus,
    /// ou en dessous tête en bas), cachées quand elles passent derrière.
    fn draw_stars(&self, buf: &mut Buffer, body: Rect, arena: Rect, keep: &[Option<Rect>]) {
        let head = match self.mood {
            Mood::Hurt(_) => f32::from(body.y) - 1.0,
            Mood::Ko(_) => f32::from(body.bottom()),
            _ => return,
        };
        let cx = f32::from(body.x) + f32::from(body.width) / 2.0;
        let rx = f32::from(body.width) / 2.0 + 1.0;
        let style = Style::new().fg(CANDLE).add_modifier(Modifier::BOLD);
        for (i, star) in STARS.iter().enumerate() {
            let a = self.tick as f32 * 0.6 + i as f32 * TAU / 3.0;
            let at = (
                (cx + rx * a.cos()).round() as i32,
                (head + a.sin()).round() as i32,
            );
            put(buf, at, star, style, arena, keep);
        }
    }

    /// « BONK! » à côté de Kiro, du côté du coup (l'autre s'il déborde).
    fn draw_bonk(&self, buf: &mut Buffer, body: Rect, arena: Rect, keep: &[Option<Rect>]) {
        let Some(b) = self.bonk else {
            return;
        };
        let len = b.word.chars().count() as i32;
        let y = i32::from(body.y + b.row.min(body.height.saturating_sub(1)));
        let left = i32::from(body.x) - len - 1;
        let right = i32::from(body.right()) + 1;
        let fits_right = right + len <= i32::from(arena.right());
        let fits_left = left >= i32::from(arena.x);
        let x = if (b.from_left && fits_left) || !fits_right {
            left
        } else {
            right
        };
        let style = Style::new().fg(BLOOD).add_modifier(Modifier::BOLD);
        put(buf, (x, y), b.word, style, arena, keep);
    }
}

/// Taille du sprite en cellules : Kitty = facteur entier qui lui donne
/// ~[`ROWS_TALL`] rangées (net : 1 pixel d'art = k×k pixels écran) ;
/// demi-blocs = [`HALFBLOCK_CELLS`].
fn sprite_cells(kitty: Option<(u16, u16)>) -> (u16, u16) {
    let Some((fw, fh)) = kitty else {
        return HALFBLOCK_CELLS;
    };
    let (fw, fh) = (u32::from(fw.max(1)), u32::from(fh.max(1)));
    let k = ((ROWS_TALL * fh + SPRITE / 2) / SPRITE).max(1);
    let px = SPRITE * k;
    (px.div_ceil(fw) as u16, px.div_ceil(fh) as u16)
}

/// Texte coupé à `width`, au plus [`BUBBLE_LINES`] lignes (« … » final).
fn bubble_lines(text: &str, width: usize) -> Vec<String> {
    let mut rows = wrap_text(text, width);
    if rows.len() > BUBBLE_LINES {
        rows.truncate(BUBBLE_LINES);
        if let Some(last) = rows.last_mut() {
            let kept = crate::provider::truncate(last, width.saturating_sub(1));
            *last = format!("{kept}…");
        }
    }
    rows
}

/// Bulle `w`×`h` centrée sur Kiro dans l'arène, à `gap` rangées de lui
/// (étoiles) : au-dessus s'il y a la place, sinon dessous ;
/// (rectangle, au-dessus ?) ou `None`.
fn bubble_rect(body: Rect, arena: Rect, (w, h): (u16, u16), gap: u16) -> Option<(Rect, bool)> {
    if w > arena.width {
        return None;
    }
    let centre = i32::from(body.x) + i32::from(body.width / 2) - i32::from(w / 2);
    let x = centre.clamp(i32::from(arena.x), i32::from(arena.right() - w)) as u16;
    let need = h + gap;
    if body.y >= arena.y + need {
        return Some((Rect::new(x, body.y - need, w, h), true));
    }
    (body.bottom() + need <= arena.bottom())
        .then(|| (Rect::new(x, body.bottom() + gap, w, h), false))
}

/// Pointe de la bulle (▼ ou ▲) sur le bord tourné vers Kiro.
fn tail(buf: &mut Buffer, rect: Rect, body: Rect, above: bool, colour: Color) {
    let x = (body.x + body.width / 2).clamp(rect.x + 1, rect.right().saturating_sub(2));
    let (y, glyph) = if above {
        (rect.bottom() - 1, "▼")
    } else {
        (rect.y, "▲")
    };
    if let Some(cell) = buf.cell_mut((x, y)) {
        cell.set_symbol(glyph).set_fg(colour);
    }
}

/// Écrit `text` en `at`, cellule par cellule, seulement dans `clip` et
/// hors des rectangles `keep` (sprite, bulle).
fn put(
    buf: &mut Buffer,
    at: (i32, i32),
    text: &str,
    style: Style,
    clip: Rect,
    keep: &[Option<Rect>],
) {
    for (i, ch) in text.chars().enumerate() {
        let (Ok(x), Ok(y)) = (u16::try_from(at.0 + i as i32), u16::try_from(at.1)) else {
            continue;
        };
        let pos = Position::new(x, y);
        if !clip.contains(pos) || keep.iter().flatten().any(|r| r.contains(pos)) {
            continue;
        }
        if let Some(cell) = buf.cell_mut(pos) {
            cell.set_symbol(ch.encode_utf8(&mut [0; 4]))
                .set_style(style);
        }
    }
}

/// Couleur de bulle : critique spectrale, cri sang, marmonnement cendre.
fn tone_colour(tone: Tone) -> Color {
    match tone {
        Tone::Critique => SPECTRE,
        Tone::Ouch => BLOOD,
        Tone::Mumble => ASH,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kitty_sprite_is_about_four_rows_tall_at_an_integer_scale() {
        assert_eq!(sprite_cells(None), HALFBLOCK_CELLS);
        // 10×20 px : ×3 = 96 px → 10×5 cellules (fit garde ×3).
        assert_eq!(sprite_cells(Some((10, 20))), (10, 5));
        // 9×19 px : ×2 = 64 px → 8×4.
        assert_eq!(sprite_cells(Some((9, 19))), (8, 4));
        // HiDPI 20×40 : ×5 = 160 px → 8×4, même taille à l'écran.
        assert_eq!(sprite_cells(Some((20, 40))), (8, 4));
        assert_eq!(sprite_cells(Some((0, 0))), (32, 32));
    }

    #[test]
    fn bubble_goes_above_else_below_and_stays_in_the_arena() {
        let arena = Rect::new(0, 1, 60, 20);
        let body = Rect::new(2, 12, 16, 8);
        let (rect, above) = bubble_rect(body, arena, (20, 4), 0).unwrap();
        assert!(above);
        assert_eq!((rect.x, rect.y, rect.bottom()), (0, 8, body.y));
        // Étoiles : deux rangées libres entre la bulle et Kiro.
        let (rect, _) = bubble_rect(body, arena, (20, 4), STARS_GAP).unwrap();
        assert_eq!(rect.bottom() + STARS_GAP, body.y);
        let high = Rect::new(50, 2, 10, 5);
        let (rect, above) = bubble_rect(high, arena, (20, 4), 0).unwrap();
        assert!(!above);
        assert_eq!((rect.right(), rect.y), (60, high.bottom()));
        assert!(bubble_rect(Rect::new(0, 1, 10, 19), arena, (20, 4), 0).is_none());
        assert!(bubble_rect(body, arena, (61, 4), 0).is_none());
    }

    #[test]
    fn long_text_is_cut_to_five_lines_with_an_ellipsis() {
        let rows = bubble_lines(&"word ".repeat(80), 20);
        assert_eq!(rows.len(), BUBBLE_LINES);
        assert!(rows.iter().all(|r| r.chars().count() <= 20));
        assert!(rows[BUBBLE_LINES - 1].ends_with('…'));
        assert_eq!(bubble_lines("short", 20), vec![String::from("short")]);
    }
}
