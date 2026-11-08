//! Sprites du jeu, dans le style du bestiaire (mêmes lettres de palette,
//! contours d'os, aplats d'encre) mais **minuscules** (6 à 18 px) pour
//! un grand niveau de plateformes :
//! - Kiro = le fantôme blanc du hub (`assets/hub/agent-*.txt`) sous son
//!   linceul ; nu (`kiro-a/b`) et effrayé (`kiro-effroi`) dans
//!   `assets/haunt/` ; les petits fantômes noirs du hub tels quels ;
//! - les créatures, objets, tuiles 6×6 et projectiles : `assets/haunt/` ;
//! - le bestiaire 32×32 ne sert qu'au décor lointain et à l'écran titre.
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

/// Grille d'`assets/haunt/`.
macro_rules! haunt {
    ($name:literal) => {
        grid(include_str!(concat!("../../assets/haunt/", $name)))
    };
}

/// Grille d'`assets/hub/`.
macro_rules! hub {
    ($name:literal) => {
        grid(include_str!(concat!("../../assets/hub/", $name)))
    };
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
    /// Faucheuse ×2 (boss).
    pub reaper: Pair,
    pub knife: Pair,
    pub lantern: Pair,
    pub candy: Pair,
    pub sheet: Pair,
    /// Terre (dessus), terre, dalle, briques, pieux, terre à l'os (6×6).
    pub tiles: [RgbaImage; 6],
    pub tombs: [Pair; 3],
    pub tree: Pair,
    pub skull: Pair,
    pub pumpkin: Pair,
    pub scythe: [RgbaImage; 4],
    pub wail: RgbaImage,
    pub raft: RgbaImage,
    pub flame: [RgbaImage; 2],
    /// Arbre mort du bestiaire (silhouettes du décor lointain).
    pub far_tree: Pair,
    /// Kiro et la Faucheuse du bestiaire ×3 (écran titre).
    pub hero: Pair,
    pub boss_art: Pair,
}

impl Sprites {
    /// Décode tout (grilles validées par les tests).
    pub fn load() -> Self {
        let tiles = haunt!("tuiles.txt");
        let tombs = haunt!("tombes.txt");
        Sprites {
            kiro: [
                Pair::new(haunt!("kiro-a.txt")),
                Pair::new(haunt!("kiro-b.txt")),
            ],
            shroud: [
                Pair::new(hub!("agent-a.txt")),
                Pair::new(hub!("agent-b.txt")),
            ],
            scared: Pair::new(haunt!("kiro-effroi.txt")),
            mummy: Pair::new(haunt!("momie.txt")),
            runner: Pair::new(haunt!("momie-course.txt")),
            bat: [
                Pair::new(haunt!("chauve-souris.txt")),
                Pair::new(haunt!("chauve-souris-b.txt")),
            ],
            hand: Pair::new(haunt!("main-tombe.txt")),
            wisp: [
                Pair::new(hub!("sous-agent-a.txt")),
                Pair::new(hub!("sous-agent-b.txt")),
            ],
            lady: Pair::new(haunt!("dame-blanche.txt")),
            head: Pair::new(haunt!("tete-zombie.txt")),
            coffin: Pair::new(haunt!("cercueil.txt")),
            coffin_open: Pair::new(haunt!("cercueil-ouvert.txt")),
            reaper: Pair::new(zoom(&haunt!("faucheuse.txt"), 2)),
            knife: Pair::new(haunt!("couteau.txt")),
            lantern: Pair::new(haunt!("lanterne.txt")),
            candy: Pair::new(haunt!("sucette.txt")),
            sheet: Pair::new(hub!("agent-a.txt")),
            tiles: std::array::from_fn(|i| crop(&tiles, i as u32 * 6, 6)),
            tombs: std::array::from_fn(|i| Pair::new(crop(&tombs, i as u32 * 7, 7))),
            tree: Pair::new(haunt!("arbre.txt")),
            skull: Pair::new(haunt!("crane.txt")),
            pumpkin: Pair::new(haunt!("citrouille.txt")),
            scythe: turns(haunt!("faux.txt")),
            wail: haunt!("plainte.txt"),
            raft: haunt!("radeau.txt"),
            flame: mirror(haunt!("flamme.txt")),
            far_tree: Pair::new(beast("arbre-mort")),
            hero: Pair::new(zoom(&beast("fantome"), 3)),
            boss_art: Pair::new(zoom(&beast("faucheuse"), 3)),
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

/// Les quatre quarts de tour d'un sprite (faux qui tourne).
fn turns(img: RgbaImage) -> [RgbaImage; 4] {
    let (a, b, c) = (
        imageops::rotate90(&img),
        imageops::rotate180(&img),
        imageops::rotate270(&img),
    );
    [img, a, b, c]
}

/// Un sprite et son miroir (flamme qui vacille).
fn mirror(img: RgbaImage) -> [RgbaImage; 2] {
    let flipped = imageops::flip_horizontal(&img);
    [img, flipped]
}

/// Agrandissement entier ×`k`, plus proche voisin (aucun lissage).
fn zoom(img: &RgbaImage, k: u32) -> RgbaImage {
    let (w, h) = img.dimensions();
    imageops::resize(img, w * k, h * k, imageops::FilterType::Nearest)
}
