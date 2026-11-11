//! Palette « crypte » : seule source des couleurs, version pure Tauri.
//!
//! Mêmes valeurs hex que `src/theme.rs` (TUI `crypte-mailbox`), mais en
//! [`Rgb`] au lieu de `ratatui::Color` (plus de dépendance TUI).
//! `gradient`/`title` (spans ratatui) ne sont pas portés : le frontend
//! Svelte fait son propre rendu à partir de `CRYPTE_THEME`.
//!
//! Les mêmes valeurs colorent le pixel art de `assets/pixel/`,
//! `assets/hub/` et `assets/haunt/`.

/// Triplet RVB (0..=255 par canal).
pub type Rgb = [u8; 3];

/// Fond de la crypte (bords de la vignette).
pub const VOID: Rgb = [0x0C, 0x09, 0x08];
/// Fond éclairé (centre de la vignette, à la bougie).
pub const CRYPT: Rgb = [0x1E, 0x15, 0x12];
/// Os : texte principal, traits des illustrations.
pub const BONE: Rgb = [0xEB, 0xDD, 0xC3];
/// Sable (Dribbble) : texte secondaire.
pub const SAND: Rgb = [0xBA, 0xA6, 0x85];
/// Terre (Dribbble) : bordures au repos.
pub const UMBER: Rgb = [0x4C, 0x44, 0x37];
/// Cendre : messages système, décor éteint.
pub const ASH: Rgb = [0x6E, 0x68, 0x62];
/// Citrouille : accent principal (la crypte qui répond).
pub const PUMPKIN: Rgb = [0xF2, 0x70, 0x1E];
/// Sang frais : alertes, gouttes, peur haute.
pub const BLOOD: Rgb = [0xD7, 0x26, 0x3D];
/// Sang séché : traînées, fonds d'alerte.
pub const DRIED: Rgb = [0x6E, 0x10, 0x1A];
/// Ectoplasme : effroi, sous-agents.
pub const ECTO: Rgb = [0x7C, 0xF2, 0x9A];
/// Spectre : Kiro au repos, brouillard.
pub const SPECTRE: Rgb = [0x9B, 0x8C, 0xFF];
/// Bougie : saisie, curseur, avertissements.
pub const CANDLE: Rgb = [0xFF, 0xC4, 0x6B];
/// Encre : remplissage sombre des sprites (chat noir, cercueils…).
pub const INK: Rgb = [0x12, 0x0D, 0x0B];
/// Ombres des accents dans le pixel art (citrouille, ectoplasme…).
pub const PUMPKIN_DARK: Rgb = [0x96, 0x3E, 0x12];
pub const ECTO_DARK: Rgb = [0x2D, 0x4E, 0x34];
pub const SPECTRE_DARK: Rgb = [0x42, 0x39, 0x6E];
pub const CANDLE_DARK: Rgb = [0xAC, 0x78, 0x3C];
/// Toile de jute (poupées, tête de sac).
pub const BURLAP: Rgb = [0x6E, 0x58, 0x3A];
/// Mousse : point vert, chat au repos (aucune réflexion en cours).
pub const MOSS: Rgb = [0x58, 0xD6, 0x68];
/// Feu follet : point bleu, chat en réflexion (envoi en cours).
pub const WISP: Rgb = [0x4A, 0xA8, 0xF0];

/// Interpolation linéaire `a → b` (`t` borné à 0..=1, comme le TUI).
pub fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let mix = |p: u8, q: u8| {
        (f32::from(p) + (f32::from(q) - f32::from(p)) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    [mix(a[0], b[0]), mix(a[1], b[1]), mix(a[2], b[2])]
}

/// Pixel opaque pour les sprites (`image`, sans passer par ratatui).
pub fn rgba(c: Rgb) -> image::Rgba<u8> {
    image::Rgba([c[0], c[1], c[2], 255])
}

/// Palette complète `(nom, hex)` pour le contrôle `--apercu`.
pub fn palette_hex() -> Vec<(&'static str, String)> {
    [
        ("void", VOID),
        ("crypt", CRYPT),
        ("bone", BONE),
        ("sand", SAND),
        ("umber", UMBER),
        ("ash", ASH),
        ("pumpkin", PUMPKIN),
        ("blood", BLOOD),
        ("dried", DRIED),
        ("ecto", ECTO),
        ("spectre", SPECTRE),
        ("candle", CANDLE),
        ("ink", INK),
        ("pumpkinDark", PUMPKIN_DARK),
        ("ectoDark", ECTO_DARK),
        ("spectreDark", SPECTRE_DARK),
        ("candleDark", CANDLE_DARK),
        ("burlap", BURLAP),
        ("moss", MOSS),
        ("wisp", WISP),
    ]
    .into_iter()
    .map(|(name, c)| (name, format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_atteint_les_deux_bouts_et_borne() {
        assert_eq!(lerp(VOID, BONE, 0.0), VOID);
        assert_eq!(lerp(VOID, BONE, 1.0), BONE);
        assert_eq!(lerp(VOID, BONE, 7.0), BONE);
        assert_eq!(lerp(VOID, BONE, -2.0), VOID);
    }

    #[test]
    fn palette_hex_couvre_le_theme_contracts() {
        let hex: Vec<(String, String)> = palette_hex()
            .into_iter()
            .map(|(n, h)| (n.to_string(), h))
            .collect();
        assert_eq!(hex.len(), 20);
        let get = |n: &str| hex.iter().find(|(k, _)| k == n).unwrap().1.clone();
        assert_eq!(get("void"), "#0C0908");
        assert_eq!(get("bone"), "#EBDDC3");
        assert_eq!(get("pumpkin"), "#F2701E");
        assert_eq!(get("blood"), "#D7263D");
        assert_eq!(get("ecto"), "#7CF29A");
    }
}
