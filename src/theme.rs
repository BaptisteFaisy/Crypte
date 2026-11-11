//! Palette « crypte » : seule source des couleurs de l'UI.
//!
//! Base tirée du shot Dribbble « Halloween icons and illustrations »
//! (encre #020202, parchemin #E8D0A6, sable #BAA685, terre #4C4437) +
//! accents (citrouille, sang, ectoplasme, spectre, bougie). Les mêmes
//! valeurs colorent le pixel art de `assets/pixel/` et `ghostty/`.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Fond de la crypte (bords de la vignette).
pub const VOID: Color = Color::Rgb(0x0C, 0x09, 0x08);
/// Fond éclairé (centre de la vignette, à la bougie).
pub const CRYPT: Color = Color::Rgb(0x1E, 0x15, 0x12);
/// Os : texte principal, traits des illustrations.
pub const BONE: Color = Color::Rgb(0xEB, 0xDD, 0xC3);
/// Sable (Dribbble) : texte secondaire.
pub const SAND: Color = Color::Rgb(0xBA, 0xA6, 0x85);
/// Terre (Dribbble) : bordures au repos.
pub const UMBER: Color = Color::Rgb(0x4C, 0x44, 0x37);
/// Cendre : messages système, décor éteint.
pub const ASH: Color = Color::Rgb(0x6E, 0x68, 0x62);
/// Citrouille : accent principal (la crypte qui répond).
pub const PUMPKIN: Color = Color::Rgb(0xF2, 0x70, 0x1E);
/// Sang frais : alertes, gouttes, peur haute.
pub const BLOOD: Color = Color::Rgb(0xD7, 0x26, 0x3D);
/// Sang séché : traînées, fonds d'alerte.
pub const DRIED: Color = Color::Rgb(0x6E, 0x10, 0x1A);
/// Ectoplasme : effroi, sous-agents.
pub const ECTO: Color = Color::Rgb(0x7C, 0xF2, 0x9A);
/// Spectre : Kiro au repos, brouillard.
pub const SPECTRE: Color = Color::Rgb(0x9B, 0x8C, 0xFF);
/// Bougie : saisie, curseur, avertissements.
pub const CANDLE: Color = Color::Rgb(0xFF, 0xC4, 0x6B);
/// Encre : remplissage sombre des sprites (chat noir, cercueils…).
pub const INK: Color = Color::Rgb(0x12, 0x0D, 0x0B);
/// Ombres des accents dans le pixel art (citrouille, ectoplasme…).
pub const PUMPKIN_DARK: Color = Color::Rgb(0x96, 0x3E, 0x12);

// Palette « project space » (docs/design/project-space/DESIGN.md §2) :
// le fond 3D et son UI flottante. Distincte de la crypte ci-dessus.
/// Fond du monde 3D, brouillard, yeux des fantômes.
pub const SPACE_BG: Color = Color::Rgb(0x05, 0x05, 0x05);
/// Fantômes, lignes, texte du monde 3D.
pub const SPACE_FG: Color = Color::Rgb(0xF5, 0xF5, 0xF3);
/// Unique accent : ce qui est actif ou modifié.
pub const SPACE_ACCENT: Color = Color::Rgb(0xFF, 0x5B, 0x1F);
pub const ECTO_DARK: Color = Color::Rgb(0x2D, 0x4E, 0x34);
pub const SPECTRE_DARK: Color = Color::Rgb(0x42, 0x39, 0x6E);
pub const CANDLE_DARK: Color = Color::Rgb(0xAC, 0x78, 0x3C);
/// Toile de jute (poupées, tête de sac).
pub const BURLAP: Color = Color::Rgb(0x6E, 0x58, 0x3A);
/// Mousse : point vert, chat au repos (aucune réflexion en cours).
pub const MOSS: Color = Color::Rgb(0x58, 0xD6, 0x68);
/// Feu follet : point bleu, chat en réflexion (envoi en cours).
pub const WISP: Color = Color::Rgb(0x4A, 0xA8, 0xF0);

/// Composantes RGB (`None` pour une couleur non truecolor).
fn rgb(c: Color) -> Option<(f32, f32, f32)> {
    match c {
        Color::Rgb(r, g, b) => Some((f32::from(r), f32::from(g), f32::from(b))),
        _ => None,
    }
}

/// Interpolation linéaire `a → b` (`t` borné à 0..=1) ; rend `a` hors RGB.
pub fn lerp(a: Color, b: Color, t: f32) -> Color {
    let (Some(x), Some(y)) = (rgb(a), rgb(b)) else {
        return a;
    };
    let t = t.clamp(0.0, 1.0);
    let mix = |p: f32, q: f32| (p + (q - p) * t).round().clamp(0.0, 255.0) as u8;
    Color::Rgb(mix(x.0, y.0), mix(x.1, y.1), mix(x.2, y.2))
}

/// Texte en dégradé caractère par caractère (`from` → `to`).
pub fn gradient(text: &str, from: Color, to: Color, modifier: Modifier) -> Vec<Span<'static>> {
    let n = text.chars().count().saturating_sub(1).max(1) as f32;
    text.chars()
        .enumerate()
        .map(|(i, ch)| {
            let style = Style::new()
                .fg(lerp(from, to, i as f32 / n))
                .add_modifier(modifier);
            Span::styled(ch.to_string(), style)
        })
        .collect()
}

/// Titre de panneau : ` texte ` en dégradé gras, encadré d'espaces.
pub fn title(text: &str, from: Color, to: Color) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    spans.extend(gradient(text, from, to, Modifier::BOLD));
    spans.push(Span::raw(" "));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_hits_both_ends_and_clamps() {
        assert_eq!(lerp(VOID, BONE, 0.0), VOID);
        assert_eq!(lerp(VOID, BONE, 1.0), BONE);
        assert_eq!(lerp(VOID, BONE, 7.0), BONE);
        assert_eq!(lerp(Color::Red, BONE, 0.5), Color::Red);
    }

    #[test]
    fn gradient_keeps_every_char() {
        let spans = gradient("CRYPTE", BLOOD, CANDLE, Modifier::BOLD);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "CRYPTE");
        assert_eq!(spans.first().and_then(|s| s.style.fg), Some(BLOOD));
        assert_eq!(spans.last().and_then(|s| s.style.fg), Some(CANDLE));
    }
}
