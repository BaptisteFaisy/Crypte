//! Sprites du jeu, tous dans le style du bestiaire : créatures 32×32 de
//! `assets/pixel/` (momies, chauve-souris, main de tombe, Dame blanche,
//! tête de zombie, cercueils, Faucheuse ×2, couteau, lanterne, sucette),
//! fantômes du hub (`assets/hub/`, ×2) et grilles du jeu
//! (`assets/haunt/` : Kiro sous son linceul, battement d'ailes, tuiles,
//! tombes, faux, plainte, cercueil flottant, flamme).
//!
//! Décodés une fois, orientés à l'avance (`Pair`), agrandis par facteur
//! **entier** au plus proche voisin seulement : la frame ne décode rien.

use image::{imageops, RgbaImage};

use crate::bestiary::{self, decode_grid};

/// Sprite et son miroir, + lignes transparentes sous les pieds (pour
/// poser le dessin sur le sol).
pub struct Pair {
    right: RgbaImage,
    left: RgbaImage,
    pub pad: u32,
}

impl Pair {
    fn new(img: RgbaImage) -> Pair {
        let pad = (0..img.height())
            .rev()
            .take_while(|&y| (0..img.width()).all(|x| img.get_pixel(x, y).0[3] == 0))
            .count() as u32;
        Pair {
            left: imageops::flip_horizontal(&img),
            right: img,
            pad,
        }
    }

    /// Dessin tourné vers `facing` (≥ 0 : droite, sens d'origine).
    pub fn get(&self, facing: f32) -> &RgbaImage {
        if facing < 0.0 {
            &self.left
        } else {
            &self.right
        }
    }
}

pub struct Sprites {
    pub kiro: [Pair; 2],
    pub shroud: [Pair; 2],
    pub scared: Pair,
    pub mummy: Pair,
    pub runner: Pair,
    pub bat: [Pair; 2],
    pub hand: Pair,
    pub wisp: [Pair; 2],
    pub lady: Pair,
    pub head: Pair,
    pub coffin: Pair,
    pub coffin_open: Pair,
    pub reaper: Pair,
    pub knife: Pair,
    pub lantern: Pair,
    pub candy: Pair,
    pub sheet: Pair,
    /// Terre (dessus), terre, dalle, briques, pieux, terre à l'os.
    pub tiles: [RgbaImage; 6],
    pub tombs: [Pair; 3],
    pub tree: Pair,
    /// Arbre mort ×2 (silhouettes du décor lointain).
    pub tree_big: Pair,
    pub skull: Pair,
    pub pumpkin: Pair,
    pub scythe: [RgbaImage; 4],
    pub wail: RgbaImage,
    pub raft: RgbaImage,
    pub flame: [RgbaImage; 2],
}

impl Sprites {
    /// Décode tout (grilles validées par les tests).
    pub fn load() -> Self {
        let kiro = beast("fantome");
        let shroud = grid(include_str!("../../assets/haunt/kiro-linceul.txt"));
        let tiles = grid(include_str!("../../assets/haunt/tuiles.txt"));
        let tombs = grid(include_str!("../../assets/haunt/tombes.txt"));
        let scythe = grid(include_str!("../../assets/haunt/faux.txt"));
        let flame = grid(include_str!("../../assets/haunt/flamme.txt"));
        let wisp = |src: &str| Pair::new(zoom(&grid(src), 2));
        Sprites {
            kiro: [Pair::new(sway(&kiro, 0)), Pair::new(sway(&kiro, 1))],
            shroud: [Pair::new(sway(&shroud, 0)), Pair::new(sway(&shroud, 1))],
            scared: Pair::new(beast("kiro-effroi")),
            mummy: Pair::new(beast("momie")),
            runner: Pair::new(beast("momie-course")),
            bat: [
                Pair::new(beast("chauve-souris")),
                Pair::new(grid(include_str!("../../assets/haunt/chauve-souris-b.txt"))),
            ],
            hand: Pair::new(beast("main-tombe")),
            wisp: [
                wisp(include_str!("../../assets/hub/sous-agent-a.txt")),
                wisp(include_str!("../../assets/hub/sous-agent-b.txt")),
            ],
            lady: Pair::new(beast("dame-blanche")),
            head: Pair::new(beast("zombie")),
            coffin: Pair::new(beast("cercueil-main")),
            coffin_open: Pair::new(beast("cercueil-ouvert")),
            reaper: Pair::new(zoom(&beast("faucheuse"), 2)),
            knife: Pair::new(beast("couteau")),
            lantern: Pair::new(beast("citrouille-bougies")),
            candy: Pair::new(beast("sucette")),
            sheet: Pair::new(zoom(&grid(include_str!("../../assets/hub/agent-a.txt")), 2)),
            tiles: std::array::from_fn(|i| crop(&tiles, i as u32 * 16, 16)),
            tombs: std::array::from_fn(|i| Pair::new(crop(&tombs, i as u32 * 16, 16))),
            tree: Pair::new(beast("arbre-mort")),
            tree_big: Pair::new(zoom(&beast("arbre-mort"), 2)),
            skull: Pair::new(beast("crane")),
            pumpkin: Pair::new(beast("citrouille")),
            scythe: [
                scythe.clone(),
                imageops::rotate90(&scythe),
                imageops::rotate180(&scythe),
                imageops::rotate270(&scythe),
            ],
            wail: grid(include_str!("../../assets/haunt/plainte.txt")),
            raft: grid(include_str!("../../assets/haunt/radeau.txt")),
            flame: [flame.clone(), imageops::flip_horizontal(&flame)],
        }
    }
}

/// Sprite du bestiaire (32×32).
fn beast(slug: &str) -> RgbaImage {
    bestiary::get(slug)
        .sprite()
        .expect("sprite du bestiaire valide")
}

/// Grille de `assets/haunt/` ou `assets/hub/`.
fn grid(src: &str) -> RgbaImage {
    decode_grid(src).expect("grille du jeu valide")
}

/// Colonne `x..x + w` d'une planche (toute sa hauteur).
fn crop(sheet: &RgbaImage, x: u32, w: u32) -> RgbaImage {
    imageops::crop_imm(sheet, x, 0, w, sheet.height()).to_image()
}

/// Agrandissement entier ×`k`, plus proche voisin (aucun lissage).
fn zoom(img: &RgbaImage, k: u32) -> RgbaImage {
    let (w, h) = img.dimensions();
    imageops::resize(img, w * k, h * k, imageops::FilterType::Nearest)
}

/// Fantôme : frame `n` = franges du bas décalées de `n` pixel (ondulation).
fn sway(img: &RgbaImage, n: u32) -> RgbaImage {
    let mut out = img.clone();
    let (w, h) = img.dimensions();
    for y in h.saturating_sub(5)..h {
        for x in 0..w {
            let src = x.checked_sub(n).map(|sx| *img.get_pixel(sx, y));
            out.put_pixel(x, y, src.unwrap_or(image::Rgba([0, 0, 0, 0])));
        }
    }
    out
}
