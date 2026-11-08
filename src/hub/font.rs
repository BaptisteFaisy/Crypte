//! Police pixel 3×5 pour les noms du hub et l'écran Haunt (majuscules,
//! chiffres, quelques signes) ; les accents sont ramenés à la lettre de
//! base. Agrandissement entier seulement (`paint`, `scale`).

use image::{Rgba, RgbaImage};

use crate::canvas::put;

/// Glyphes 3×5, ligne par ligne (`1` = pixel allumé).
const GLYPHS: [(char, &str); 45] = [
    ('A', "010101111101101"),
    ('B', "110101110101110"),
    ('C', "011100100100011"),
    ('D', "110101101101110"),
    ('E', "111100110100111"),
    ('F', "111100110100100"),
    ('G', "011100101101011"),
    ('H', "101101111101101"),
    ('I', "111010010010111"),
    ('J', "001001001101010"),
    ('K', "101101110101101"),
    ('L', "100100100100111"),
    ('M', "101111111101101"),
    ('N', "110101101101101"),
    ('O', "010101101101010"),
    ('P', "110101110100100"),
    ('Q', "010101101111011"),
    ('R', "110101110101101"),
    ('S', "011100010001110"),
    ('T', "111010010010010"),
    ('U', "101101101101111"),
    ('V', "101101101101010"),
    ('W', "101101111111101"),
    ('X', "101101010101101"),
    ('Y', "101101010010010"),
    ('Z', "111001010100111"),
    ('0', "111101101101111"),
    ('1', "010110010010111"),
    ('2', "110001010100111"),
    ('3', "110001010001110"),
    ('4', "101101111001001"),
    ('5', "111100110001110"),
    ('6', "011100111101111"),
    ('7', "111001010010010"),
    ('8', "111101111101111"),
    ('9', "111101111001110"),
    ('#', "101111101111101"),
    ('-', "000000111000000"),
    ('.', "000000000000010"),
    ('?', "110001010000010"),
    (':', "000010000010000"),
    ('!', "010010010000010"),
    ('+', "000010111010000"),
    ('/', "001001010100100"),
    (' ', "000000000000000"),
];

/// Majuscule sans accent (les inconnus deviennent un espace).
fn fold(ch: char) -> char {
    let up = ch.to_uppercase().next().unwrap_or(ch);
    let base = match up {
        'À' | 'Â' | 'Ä' | 'Á' => 'A',
        'É' | 'È' | 'Ê' | 'Ë' => 'E',
        'Î' | 'Ï' | 'Í' => 'I',
        'Ô' | 'Ö' | 'Ó' | 'Œ' => 'O',
        'Ù' | 'Û' | 'Ü' | 'Ú' => 'U',
        'Ç' => 'C',
        other => other,
    };
    if GLYPHS.iter().any(|(c, _)| *c == base) {
        base
    } else {
        ' '
    }
}

/// Largeur en pixels de `text` (3 px par lettre + 1 px d'espace).
pub fn width(text: &str) -> i64 {
    (text.chars().count() as i64 * 4 - 1).max(0)
}

/// Écrit `text` centré sur `cx`, haut en `top`, sur une étiquette `bg`
/// (marge de 1 px) pour rester lisible sur n'importe quel fond.
pub fn label(cv: &mut RgbaImage, cx: i64, top: i64, text: &str, ink: Rgba<u8>, bg: Rgba<u8>) {
    let w = width(text);
    let x0 = cx - w / 2;
    for y in top - 1..top + 6 {
        for x in x0 - 1..x0 + w + 1 {
            put(cv, x, y, bg);
        }
    }
    paint(cv, x0, top, text, 1, |_| ink);
}

/// Écrit `text` à partir de (x0, top), chaque pixel de glyphe agrandi en
/// carré `scale`×`scale` ; `ink(ligne)` colore la ligne 0..5 du glyphe
/// (dégradés verticaux des titres).
pub fn paint(
    cv: &mut RgbaImage,
    x0: i64,
    top: i64,
    text: &str,
    scale: i64,
    ink: impl Fn(i64) -> Rgba<u8>,
) {
    let k = scale.max(1);
    for (n, ch) in text.chars().map(fold).enumerate() {
        let Some((_, bits)) = GLYPHS.iter().find(|(c, _)| *c == ch) else {
            continue;
        };
        for (i, bit) in bits.chars().enumerate() {
            if bit != '1' {
                continue;
            }
            let (dx, dy) = ((i % 3) as i64, (i / 3) as i64);
            let (x, y) = (x0 + (n as i64 * 4 + dx) * k, top + dy * k);
            for p in 0..k * k {
                put(cv, x + p % k, y + p / k, ink(dy));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_are_3x5_and_accents_fold() {
        assert!(GLYPHS.iter().all(|(_, b)| b.len() == 15));
        assert_eq!(fold('é'), 'E');
        assert_eq!(fold('ç'), 'C');
        assert_eq!(fold('@'), ' ');
        assert_eq!(width("#12"), 11);
    }

    #[test]
    fn label_paints_ink_on_its_tag() {
        let mut cv = RgbaImage::new(40, 12);
        let (ink, bg) = (Rgba([255, 0, 0, 255]), Rgba([1, 1, 1, 255]));
        label(&mut cv, 20, 3, "Crypte", ink, bg);
        let inked = cv.pixels().filter(|p| **p == ink).count();
        assert!(inked > 30, "texte presque vide ({inked} px)");
        assert_eq!(*cv.get_pixel(20, 2), bg);
    }
}
