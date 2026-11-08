//! Bestiaire : 33 créatures en pixel art 32×32 dessinées pour la crypte
//! (d'après les 33 éléments du shot Dribbble « Halloween icons and
//! illustrations ») + Kiro effrayé.
//!
//! Chaque sprite est une grille texte de `assets/pixel/` (32 lignes de 32
//! caractères, une lettre = une couleur de [`theme`], `.` = transparent),
//! embarquée à la compilation et éditable à la main. Agrandissement
//! uniquement par facteur entier, au plus proche voisin : pixels nets.

use anyhow::{bail, Context, Result};
use image::{imageops, Rgba, RgbaImage};
use ratatui::style::Color;

use crate::theme;

/// Côté d'un sprite, en pixels.
pub const SPRITE: u32 = 32;

/// Une créature : identifiant de fichier, nom et légende (FR), sprite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Creature {
    pub slug: &'static str,
    pub name: &'static str,
    pub lore: &'static str,
    pixels: &'static str,
}

impl Creature {
    /// Sprite 32×32 décodé.
    pub fn sprite(&self) -> Result<RgbaImage> {
        decode(self.pixels).with_context(|| format!("sprite {}", self.slug))
    }
}

macro_rules! creature {
    ($slug:literal, $name:literal, $lore:literal) => {
        Creature {
            slug: $slug,
            name: $name,
            lore: $lore,
            pixels: include_str!(concat!("../assets/pixel/", $slug, ".txt")),
        }
    };
}

/// Les 33 créatures, dans l'ordre des 4 planches du shot.
pub const BESTIARY: [Creature; 33] = [
    creature!("loup-garou", "Werewolf", "It howls in your stead."),
    creature!("araignee", "Spider", "It weaves your sub-agents."),
    creature!("crane", "Grassy Skull", "Even dead, it keeps growing."),
    creature!(
        "chat-noir",
        "Black Cat",
        "It has already seen your next error."
    ),
    creature!("infirmiere", "Nurse", "The diagnosis is fatal."),
    creature!("fantome", "Kiro", "The keeper of the crypt."),
    creature!("couteau", "Knife", "It cuts answers short."),
    creature!("citrouille", "Pumpkin", "Its cross eyes never forgive."),
    creature!("chauve-souris", "Bat", "It sleeps upside down."),
    creature!("frankenstein", "Frankenstein", "Stitched from old prompts."),
    creature!(
        "tete-poignardee",
        "The Stabbed",
        "It read one diff too many."
    ),
    creature!("main-tombe", "Grave Hand", "Here lies a request."),
    creature!("cercueil-main", "Coffin", "History gets buried here."),
    creature!("vaudou", "Voodoo Doll", "Every pin is a bug."),
    creature!("chaudron", "Cauldron", "The answer is simmering."),
    creature!("sucette", "Lollipop", "The stick is a bone."),
    creature!(
        "citrouille-bougies",
        "Lantern",
        "Two candles keeping watch."
    ),
    creature!("zombie", "Zombie", "Brain out in the open."),
    creature!("dame-blanche", "White Lady", "Never look her in the face."),
    creature!("faucheuse", "The Reaper", "It counts your tokens."),
    creature!("cercueil-ouvert", "Open Coffin", "Somebody got out."),
    creature!("momie", "Mummy", "It shambles, arms outstretched."),
    creature!("ronde", "The Round", "Three demons dancing."),
    creature!("tete-de-sac", "Bag Head", "Trick or treat."),
    creature!("farceur", "The Prankster", "It rings at your door."),
    creature!("tele", "The TV", "It crawls out of the screen."),
    creature!("masque-bois", "Antler Mask", "A skull, antlers, a bone."),
    creature!("cri", "The Scream", "Its mask screams in silence."),
    creature!("arbre-mort", "Dead Tree", "Its branches bear bones."),
    creature!("bouquet", "The Bouquet", "Flowers for your grave."),
    creature!("poupee", "Pinned Heart", "It loves you to death."),
    creature!("pentacle", "Pentacle Cat", "The ritual has begun."),
    creature!("momie-course", "Hurried Mummy", "It runs faster than you."),
];

/// Kiro effrayé : hors bestiaire, planche seule.
pub const KIRO_SPOOKED: Creature = creature!("kiro-effroi", "Kiro", "Something scared it!");

/// Kiro en balade (voir [`crate::phantom`]) : grand fantôme blanc du
/// hub (corps os, ombre sable à droite, yeux d'encre). Au repos, en vol
/// (1 px plus haut, festons décalés), parle, juge (paupières lourdes),
/// frappé (écrasé, yeux en croix), K.O. (frappé tête en bas), apeuré.
pub const KIRO_IDLE: Creature = creature!("kiro-spectre", "Kiro", "It roams the crypt.");
pub const KIRO_FLOAT: Creature = creature!("kiro-flotte", "Kiro", "It drifts through the crypt.");
pub const KIRO_TALK: Creature = creature!("kiro-parle", "Kiro", "It has notes on your prompt.");
pub const KIRO_JUDGE: Creature = creature!("kiro-juge", "Kiro", "Unimpressed.");
pub const KIRO_HIT: Creature = creature!("kiro-frappe", "Kiro", "Ouch.");
pub const KIRO_KO: Creature = creature!("kiro-ko", "Kiro", "Down for the count.");
pub const KIRO_FRIGHT: Creature = creature!("kiro-peur", "Kiro", "Hands up, ghost.");

/// Kiro hors bestiaire (effroi du portrait + balade) : recherche, tests.
const KIRO_EXTRAS: [&Creature; 8] = [
    &KIRO_SPOOKED,
    &KIRO_IDLE,
    &KIRO_FLOAT,
    &KIRO_TALK,
    &KIRO_JUDGE,
    &KIRO_HIT,
    &KIRO_KO,
    &KIRO_FRIGHT,
];

/// Créature par identifiant (bestiaire puis Kiro hors bestiaire).
pub fn find(slug: &str) -> Option<&'static Creature> {
    BESTIARY.iter().chain(KIRO_EXTRAS).find(|c| c.slug == slug)
}

/// Créature par identifiant ; panique si absent (identifiants internes).
pub fn get(slug: &str) -> &'static Creature {
    find(slug).unwrap_or_else(|| panic!("unknown creature: {slug}"))
}

/// Couleur d'une lettre de grille (`.` = transparent, `None` = inconnue).
fn palette(ch: char) -> Option<Color> {
    Some(match ch {
        'W' => theme::BONE,
        'S' => theme::SAND,
        'U' => theme::UMBER,
        'K' => theme::INK,
        'O' => theme::PUMPKIN,
        'o' => theme::PUMPKIN_DARK,
        'R' => theme::BLOOD,
        'r' => theme::DRIED,
        'G' => theme::ECTO,
        'g' => theme::ECTO_DARK,
        'P' => theme::SPECTRE,
        'p' => theme::SPECTRE_DARK,
        'Y' => theme::CANDLE,
        'y' => theme::CANDLE_DARK,
        'N' => theme::BURLAP,
        '.' => Color::Reset,
        _ => return None,
    })
}

/// Grille texte 32×32 → image RGBA (transparent hors des pixels peints).
pub fn decode(grid: &str) -> Result<RgbaImage> {
    let img = decode_grid(grid)?;
    if img.dimensions() != (SPRITE, SPRITE) {
        bail!("sprite {:?} instead of {SPRITE}x{SPRITE}", img.dimensions());
    }
    Ok(img)
}

/// Grille texte rectangulaire (lignes de même longueur, lettres de la
/// palette, `.` transparent) → image RGBA.
pub fn decode_grid(grid: &str) -> Result<RgbaImage> {
    let rows: Vec<Vec<char>> = grid.lines().map(|r| r.chars().collect()).collect();
    let width = rows.first().map_or(0, Vec::len);
    if rows.is_empty() || width == 0 {
        bail!("empty grid");
    }
    let mut img = RgbaImage::new(width as u32, rows.len() as u32);
    for (y, row) in rows.iter().enumerate() {
        if row.len() != width {
            bail!("line {y}: {} columns instead of {width}", row.len());
        }
        for (x, &ch) in row.iter().enumerate() {
            let px = match palette(ch) {
                Some(Color::Rgb(r, g, b)) => Rgba([r, g, b, 255]),
                Some(_) => Rgba([0, 0, 0, 0]),
                None => bail!("line {y}: unknown color {ch:?}"),
            };
            img.put_pixel(x as u32, y as u32, px);
        }
    }
    Ok(img)
}

/// Sprite agrandi par le plus grand facteur **entier** qui tient dans
/// `w`×`h` px, centré sur un canevas transparent de exactement `w`×`h`.
/// Trop petit pour ×1 : réduction au plus proche voisin (repli).
pub fn fit(sprite: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let side = w.min(h);
    let size = match side / SPRITE {
        0 => side.max(1),
        k => k * SPRITE,
    };
    let scaled = imageops::resize(sprite, size, size, imageops::FilterType::Nearest);
    let mut canvas = RgbaImage::new(w.max(1), h.max(1));
    let (dx, dy) = ((w.saturating_sub(size)) / 2, (h.saturating_sub(size)) / 2);
    imageops::overlay(&mut canvas, &scaled, i64::from(dx), i64::from(dy));
    canvas
}

/// Planche PNG de tout le bestiaire (+ Kiro effrayé) agrandi ×6, 6 par
/// rangée, sur le fond de la crypte (`--planche`).
pub fn write_sheet(path: &str) -> Result<()> {
    const ZOOM: u32 = 6;
    const PAD: u32 = 24;
    const COLS: u32 = 6;
    let all: Vec<&Creature> = BESTIARY.iter().chain([&KIRO_SPOOKED]).collect();
    let cell = SPRITE * ZOOM;
    let step = cell + PAD;
    let rows = (all.len() as u32).div_ceil(COLS);
    let bg = Rgba([0x0C, 0x09, 0x08, 0xFF]);
    let mut sheet = RgbaImage::from_pixel(COLS * step + PAD, rows * step + PAD, bg);
    for (i, creature) in all.iter().enumerate() {
        let img = fit(&creature.sprite()?, cell, cell);
        let (x, y) = (
            PAD + (i as u32 % COLS) * step,
            PAD + (i as u32 / COLS) * step,
        );
        imageops::overlay(&mut sheet, &img, i64::from(x), i64::from(y));
    }
    sheet.save(path).with_context(|| format!("writing {path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sprite_decodes_with_visible_pixels() {
        for creature in BESTIARY.iter().chain(KIRO_EXTRAS) {
            let img = creature.sprite().expect(creature.slug);
            let inked = img.pixels().filter(|p| p.0[3] == 255).count();
            assert!(inked > 80, "{} almost empty ({inked} px)", creature.slug);
        }
    }

    #[test]
    fn decode_rejects_bad_grids() {
        assert!(decode("..\n..").is_err());
        assert!(decode_grid("..\n..").is_ok());
        assert!(decode_grid("..\n.").is_err());
        let bad = format!("{}\n", ".".repeat(31)).repeat(32);
        assert!(decode(&bad).is_err());
        let alien = format!("{}Z\n", ".".repeat(31)).repeat(32);
        assert!(decode(&alien).is_err());
    }

    #[test]
    fn fit_scales_by_integer_factor_only() {
        let sprite = get("citrouille").sprite().unwrap();
        let out = fit(&sprite, 70, 100);
        assert_eq!(out.dimensions(), (70, 100));
        let (dx, dy) = (3, 18);
        for (x, y) in [(0, 0), (15, 20), (31, 31), (8, 27)] {
            let src = sprite.get_pixel(x, y);
            for (ox, oy) in [(0, 0), (1, 1)] {
                let dst = out.get_pixel(dx + 2 * x + ox, dy + 2 * y + oy);
                assert_eq!(src, dst, "pixel ({x},{y}) distorted");
            }
        }
    }

    #[test]
    fn slugs_are_unique_and_findable() {
        let mut slugs: Vec<&str> = BESTIARY.iter().map(|c| c.slug).collect();
        slugs.extend(KIRO_EXTRAS.iter().map(|c| c.slug));
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), BESTIARY.len() + KIRO_EXTRAS.len());
        assert_eq!(get("kiro-ko"), &KIRO_KO);
        assert_eq!(get("fantome").name, "Kiro");
        assert_eq!(get("kiro-effroi"), &KIRO_SPOOKED);
        assert!(find("unicorn").is_none());
    }

    #[test]
    fn kiro_ko_is_the_hit_frame_upside_down() {
        let hit = KIRO_HIT.sprite().unwrap();
        assert_eq!(KIRO_KO.sprite().unwrap(), imageops::flip_vertical(&hit));
    }
}
