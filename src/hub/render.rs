//! Rendu du hub : crypte isométrique en pixel art (`iso.rs`), esprits qui
//! flottent au-dessus de leur ombre, triés par profondeur, fils
//! d'ectoplasme, orbes-messages, bulles, noms en police pixel.
//!
//! - Ghostty / Kitty : le canevas (1 pixel d'art = `scale`² pixels écran)
//!   est agrandi au plus proche voisin à la taille pixel exacte de la zone
//!   et renvoyé **à chaque frame** sous le même identifiant d'image (PNG) :
//!   les placeholders restent en place, seule l'image change. Net, fluide.
//! - Ailleurs : 1 pixel d'art = 1 demi-cellule (`▀`).

use image::{imageops, Rgba, RgbaImage};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

use super::iso::{put, px, Room};
use super::live::present_kitty;
use super::{font, Glyph, Hub, Key, Mood, Spectre};
use crate::bestiary::decode_grid;
use crate::sessions::Sessions;
use crate::theme;

/// Bulle 7×7 cerclée de terre ; glyphe dans le corps 5×4 (décalage +1).
const BUBBLE: [&str; 7] = [
    ".UUUUU.", "UWWWWWU", "UWWWWWU", "UWWWWWU", "UWWWWWU", ".UWUUU.", "..U....",
];
/// Hauteur de vol (pixels d'art) : agents, sous-agents.
const HOVER: (i64, i64) = (4, 3);

/// Sprites du hub : fantôme blanc (agent), petit fantôme noir (sous-agent).
/// Les variantes retournées (`*_left`) et les 5 bulles sont pré-rendues
/// au chargement : la frame emprunte, ne clone ni ne décode jamais.
pub struct Sprites {
    agent: [RgbaImage; 2],
    sub: [RgbaImage; 2],
    sleep: RgbaImage,
    dead: RgbaImage,
    agent_left: [RgbaImage; 2],
    sub_left: [RgbaImage; 2],
    sleep_left: RgbaImage,
    dead_left: RgbaImage,
    bubbles: [RgbaImage; 5],
}

impl Sprites {
    /// Grilles de `assets/hub/` (validées par les tests).
    pub fn load() -> Self {
        let g = |src: &str| decode_grid(src).expect("sprite du hub valide");
        let agent = [
            g(include_str!("../../assets/hub/agent-a.txt")),
            g(include_str!("../../assets/hub/agent-b.txt")),
        ];
        let sub = [
            g(include_str!("../../assets/hub/sous-agent-a.txt")),
            g(include_str!("../../assets/hub/sous-agent-b.txt")),
        ];
        let sleep = g(include_str!("../../assets/hub/sous-agent-dort.txt"));
        let dead = g(include_str!("../../assets/hub/sous-agent-mort.txt"));
        let flip = |img: &RgbaImage| imageops::flip_horizontal(img);
        Self {
            agent_left: [flip(&agent[0]), flip(&agent[1])],
            sub_left: [flip(&sub[0]), flip(&sub[1])],
            sleep_left: flip(&sleep),
            dead_left: flip(&dead),
            agent,
            sub,
            sleep,
            dead,
            bubbles: [
                bubble(Glyph::Talk),
                bubble(Glyph::Alert),
                bubble(Glyph::Ask),
                bubble(Glyph::Sleep),
                bubble(Glyph::Love),
            ],
        }
    }

    /// Bulle pré-rendue pour `glyph` (jamais décodée en frame).
    pub fn bubble(&self, glyph: Glyph) -> &RgbaImage {
        let i = match glyph {
            Glyph::Talk => 0,
            Glyph::Alert => 1,
            Glyph::Ask => 2,
            Glyph::Sleep => 3,
            Glyph::Love => 4,
        };
        &self.bubbles[i]
    }
}

/// Dessine le hub dans `area`. `cell` = taille de cellule en pixels si le
/// terminal parle Kitty (sinon demi-blocs). Fixe la taille de la crypte.
pub fn draw(
    hub: &mut Hub,
    sessions: &Sessions,
    area: Rect,
    buf: &mut Buffer,
    cell: Option<(u16, u16)>,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let (fw, fh, scale) = match cell {
        Some((fw, fh)) => (
            u32::from(fw),
            u32::from(fh),
            u32::from((fh / 6).clamp(2, 6)),
        ),
        None => (1, 2, 1),
    };
    let (bw, bh) = (u32::from(area.width) * fw, u32::from(area.height) * fh);
    let (cw, ch) = ((bw / scale).max(1), (bh / scale).max(1));
    let room = Room::fit(cw, ch);
    hub.set_room(room.n);
    let mut cv = RgbaImage::from_pixel(cw, ch, px(theme::VOID));
    room.draw(&mut cv, hub.tick);
    scene(&mut cv, hub, sessions, &room);
    if cell.is_some() {
        present_kitty(&mut hub.view, &cv, scale, (bw, bh), area, buf);
    } else {
        to_cells(&cv, area, buf);
    }
}

/// Fils, ombres, esprits (de l'arrière vers l'avant), orbes, bulles, noms.
fn scene(cv: &mut RgbaImage, hub: &Hub, sessions: &Sessions, room: &Room) {
    threads(cv, hub, room);
    let mut order: Vec<&Spectre> = Vec::with_capacity(hub.spectres.len());
    order.extend(hub.spectres.iter());
    order.sort_by(|a, b| (a.x + a.y).total_cmp(&(b.x + b.y)));
    for s in &order {
        spectre(cv, hub, room, s);
    }
    orbs(cv, hub, room);
    bubbles(cv, hub, room);
    for s in &order {
        name_tag(cv, hub, sessions, room, s);
    }
}

/// Sprite courant d'un esprit (frame animée, état, orientation),
/// emprunté aux pré-rendus (jamais cloné en frame).
fn sprite<'h>(hub: &'h Hub, s: &Spectre) -> &'h RgbaImage {
    let frame = ((hub.tick + seed(s)) / 4 % 2) as usize;
    let sp = &hub.sprites;
    let (right, left) = match (s.key, s.mood) {
        (Key::Agent(_), _) => (&sp.agent[frame], &sp.agent_left[frame]),
        (Key::Sub(_), Mood::Done) => (&sp.sleep, &sp.sleep_left),
        (Key::Sub(_), Mood::Failed) => (&sp.dead, &sp.dead_left),
        (Key::Sub(_), _) => (&sp.sub[frame], &sp.sub_left[frame]),
    };
    if s.left {
        left
    } else {
        right
    }
}

/// Décalage d'animation propre à chaque esprit.
fn seed(s: &Spectre) -> u64 {
    match s.key {
        Key::Agent(i) => i as u64 * 7,
        Key::Sub(i) => i as u64 * 5 + 3,
    }
}

/// Point d'ancrage au sol (pixels) et hauteur de vol de l'esprit.
fn anchor(hub: &Hub, room: &Room, s: &Spectre) -> (i64, i64, i64) {
    let (x, y) = room.project(s.x, s.y);
    let base = match s.key {
        Key::Agent(_) => HOVER.0,
        Key::Sub(_) => HOVER.1,
    };
    let bob = i64::from((hub.tick + seed(s)) / 6 % 2 == 0);
    (
        x.round() as i64,
        y.round() as i64,
        base + bob + i64::from(s.jump / 2),
    )
}

/// Ombre, corps, repère citrouille.
fn spectre(cv: &mut RgbaImage, hub: &Hub, room: &Room, s: &Spectre) {
    let img = sprite(hub, s);
    let (x, y, lift) = anchor(hub, room, s);
    let rx = i64::from(img.width() / 2);
    shadow(cv, x, y, rx, (rx / 3).max(1));
    let top = y - lift - i64::from(img.height());
    blend(cv, img, x - rx, top, f32::from(s.fade) / 8.0);
}

/// Ombre ovale au sol (assombrit ce qui est dessous).
fn shadow(cv: &mut RgbaImage, cx: i64, cy: i64, rx: i64, ry: i64) {
    for dy in -ry..=ry {
        for dx in -rx..=rx {
            let e = (dx * dx) as f32 / (rx * rx).max(1) as f32
                + (dy * dy) as f32 / (ry * ry).max(1) as f32;
            let (x, y) = (cx + dx, cy + dy);
            if e > 1.0 || x < 0 || y < 0 || x as u32 >= cv.width() || y as u32 >= cv.height() {
                continue;
            }
            let p = cv.get_pixel(x as u32, y as u32).0;
            let k = |c: u8| (f32::from(c) * 0.5) as u8;
            cv.put_pixel(x as u32, y as u32, Rgba([k(p[0]), k(p[1]), k(p[2]), 255]));
        }
    }
}

/// Colle `img` en (x0, y0) avec une opacité `alpha` (0..=1).
fn blend(cv: &mut RgbaImage, img: &RgbaImage, x0: i64, y0: i64, alpha: f32) {
    for (x, y, p) in img.enumerate_pixels() {
        let (cx, cy) = (x0 + i64::from(x), y0 + i64::from(y));
        if p.0[3] == 0 || cx < 0 || cy < 0 || cx as u32 >= cv.width() || cy as u32 >= cv.height() {
            continue;
        }
        let d = cv.get_pixel(cx as u32, cy as u32).0;
        let mix =
            |i: usize| (f32::from(d[i]) + (f32::from(p.0[i]) - f32::from(d[i])) * alpha) as u8;
        cv.put_pixel(cx as u32, cy as u32, Rgba([mix(0), mix(1), mix(2), 255]));
    }
}

/// Fils d'ectoplasme pointillés agent → sous-agents au travail.
fn threads(cv: &mut RgbaImage, hub: &Hub, room: &Room) {
    let ink = px(theme::lerp(theme::ECTO_DARK, theme::ECTO, 0.6));
    for s in hub
        .spectres
        .iter()
        .filter(|s| matches!(s.key, Key::Sub(_)) && s.mood == Mood::Running)
    {
        let Some(owner) = hub.spectres.iter().find(|o| o.key == Key::Agent(s.owner)) else {
            continue;
        };
        let (ax, ay, al) = anchor(hub, room, owner);
        let (bx, by, bl) = anchor(hub, room, s);
        let (ay, by) = (ay - al - 5, by - bl - 3);
        let len = ((bx - ax).abs() + (by - ay).abs()).max(1);
        for i in (0..len).filter(|i| (*i as u64 + hub.tick / 2) % 3 == 0) {
            let t = i as f32 / len as f32;
            put(
                cv,
                ax + ((bx - ax) as f32 * t) as i64,
                ay + ((by - ay) as f32 * t) as i64,
                ink,
            );
        }
    }
}

/// Orbes-messages : traînée ambrée, cœur bougie, arc au-dessus du sol.
fn orbs(cv: &mut RgbaImage, hub: &Hub, room: &Room) {
    let lift = room.tile_h() * 2.0;
    let at = |(x, y, h): (f32, f32, f32)| {
        let (sx, sy) = room.project(x, y);
        (sx.round() as i64, (sy - 6.0 - h * lift).round() as i64)
    };
    for orb in &hub.orbs {
        for &p in &orb.trail {
            let (x, y) = at(p);
            put(cv, x, y, px(theme::CANDLE_DARK));
        }
        let Some(p) = hub.orb_pos(orb) else { continue };
        let (x, y) = at(p);
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            put(cv, x + dx, y + dy, px(theme::PUMPKIN));
        }
        put(cv, x, y, px(theme::CANDLE));
    }
}

/// Bulles au-dessus à droite des têtes.
fn bubbles(cv: &mut RgbaImage, hub: &Hub, room: &Room) {
    for b in &hub.bubbles {
        let Some(s) = hub.spectres.iter().find(|s| s.key == b.key) else {
            continue;
        };
        let img = sprite(hub, s);
        let (x, y, lift) = anchor(hub, room, s);
        let top = y - lift - i64::from(img.height());
        blend(
            cv,
            hub.sprites.bubble(b.glyph),
            x + 1,
            (top - 7).max(0),
            1.0,
        );
    }
}

/// Bulle avec son glyphe : « … », « ! », « ? », « z », cœur.
fn bubble(glyph: Glyph) -> RgbaImage {
    let mut img = decode_grid(&BUBBLE.join("\n")).expect("bulle valide");
    let (colour, dots): (Color, &[(u32, u32)]) = match glyph {
        Glyph::Talk => (theme::INK, &[(0, 2), (2, 2), (4, 2)]),
        Glyph::Alert => (theme::BLOOD, &[(2, 0), (2, 1), (2, 3)]),
        Glyph::Ask => (theme::INK, &[(1, 0), (2, 0), (3, 1), (2, 2), (2, 3)]),
        Glyph::Sleep => (
            theme::SPECTRE_DARK,
            &[(1, 0), (2, 0), (3, 0), (2, 1), (1, 2), (2, 2), (3, 2)],
        ),
        Glyph::Love => (
            theme::BLOOD,
            &[
                (1, 0),
                (3, 0),
                (0, 1),
                (1, 1),
                (2, 1),
                (3, 1),
                (4, 1),
                (1, 2),
                (2, 2),
                (3, 2),
                (2, 3),
            ],
        ),
    };
    for &(x, y) in dots {
        img.put_pixel(x + 1, y + 1, px(colour));
    }
    img
}

/// Étiquettes : nom du chat sous l'agent (os, citrouille si choisi) ;
/// numéro du sous-agent au-dessus de sa tête (ectoplasme au travail,
/// cendre sinon) — jamais au même endroit, donc pas de chevauchement.
fn name_tag(cv: &mut RgbaImage, hub: &Hub, sessions: &Sessions, room: &Room, s: &Spectre) {
    let (x, y, lift) = anchor(hub, room, s);
    match s.key {
        Key::Agent(i) => {
            let name = sessions
                .get(i)
                .map(|c| {
                    let end = c
                        .name
                        .char_indices()
                        .map(|(idx, _)| idx)
                        .nth(10)
                        .unwrap_or(c.name.len());
                    &c.name[..end]
                })
                .unwrap_or("");
            font::label(cv, x, y + 3, name, px(theme::BONE), px(theme::VOID));
        }
        Key::Sub(id) => {
            let colour = if s.mood == Mood::Running {
                theme::ECTO
            } else {
                theme::ASH
            };
            let top = y - lift - i64::from(sprite(hub, s).height()) - 7;
            font::label(cv, x, top, &id.to_string(), px(colour), px(theme::VOID));
        }
    }
}

/// Demi-blocs : `▀` (haut = avant-plan, bas = fond), espace si égaux.
fn to_cells(cv: &RgbaImage, area: Rect, buf: &mut Buffer) {
    for cy in 0..area.height {
        for cx in 0..area.width {
            let (x, y) = (u32::from(cx), u32::from(cy) * 2);
            let (up, lo) = (
                cv.get_pixel(x, y).0,
                cv.get_pixel(x, (y + 1).min(cv.height() - 1)).0,
            );
            let (up, lo) = (
                Color::Rgb(up[0], up[1], up[2]),
                Color::Rgb(lo[0], lo[1], lo[2]),
            );
            if let Some(cell) = buf.cell_mut((area.x + cx, area.y + cy)) {
                cell.reset();
                if up == lo {
                    cell.set_symbol(" ").set_bg(lo);
                } else {
                    cell.set_symbol("▀").set_fg(up).set_bg(lo);
                }
            }
        }
    }
}
