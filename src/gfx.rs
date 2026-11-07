//! Images dans le terminal, au pixel près : protocole graphique Kitty
//! (Ghostty, kitty, WezTerm…) détecté au démarrage, demi-blocs ailleurs.
//!
//! Le sprite (32×32) est agrandi par un facteur **entier** au plus proche
//! voisin puis posé sur un canevas de exactement la taille pixel de la
//! zone : le terminal l'affiche sans redimensionner, donc sans flou.
//! - Kitty : 1 pixel image = 1 pixel écran (taille de cellule interrogée).
//! - Demi-blocs : 1 pixel image = 1 demi-cellule (`▀`, 2 pixels par cellule).
//!
//! Chaque (créature, taille) n'est encodée et transmise qu'une fois.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use image::{DynamicImage, Rgba};
use ratatui::{layout::Rect, Frame};
use ratatui_image::{
    picker::{Picker, ProtocolType},
    protocol::Protocol,
    Image, Resize,
};

use crate::bestiary::{self, Creature};

/// Taille de cellule supposée sans réponse du terminal (rapport ~1:2).
const FALLBACK_FONT: (u16, u16) = (10, 20);
/// Demi-blocs : une cellule = 1×2 pixels d'image.
const HALFBLOCK_FONT: (u16, u16) = (1, 2);
/// Au-delà, les entrées expirées puis la moitié la moins récente
/// sont évincées (redimensionnements successifs).
const CACHE_MAX: usize = 96;
/// Les tailles non redessinées depuis ce délai sont évincées.
const CACHE_TTL: Duration = Duration::from_secs(600);
/// Fond des pixels transparents en demi-blocs (= `theme::VOID`, comme le
/// fond de fenêtre que Ghostty montre sous les pixels transparents Kitty).
const VOID_RGBA: Rgba<u8> = Rgba([0x0C, 0x09, 0x08, 0xFF]);

/// Créatures déjà encodées pour le terminal, par (créature, taille de zone).
pub struct ImageBank {
    picker: Picker,
    cache: HashMap<CacheKey, CacheEntry>,
    drawn: Vec<Rect>,
}

/// (slug créature, largeur, hauteur de zone).
type CacheKey = (&'static str, u16, u16);

/// Protocole encodé + dernier dessin (ordre d'éviction LRU).
struct CacheEntry {
    proto: Protocol,
    last_used: Instant,
}

impl ImageBank {
    /// Interroge le terminal (protocole + taille de cellule). À appeler
    /// après `ratatui::init` et avant de lire les événements.
    pub fn detect() -> Self {
        let picker =
            Picker::from_query_stdio().unwrap_or_else(|_| Picker::from_fontsize(FALLBACK_FONT));
        Self::with_picker(picker)
    }

    /// Sans terminal (tests, `--smoke`) : demi-blocs.
    pub fn headless() -> Self {
        Self::with_picker(Picker::from_fontsize(FALLBACK_FONT))
    }

    fn with_picker(picker: Picker) -> Self {
        let picker = if picker.protocol_type() == ProtocolType::Halfblocks {
            let mut half = Picker::from_fontsize(HALFBLOCK_FONT);
            half.set_protocol_type(ProtocolType::Halfblocks);
            half.set_background_color(VOID_RGBA);
            half
        } else {
            picker
        };
        Self {
            picker,
            cache: HashMap::new(),
            drawn: Vec::new(),
        }
    }

    /// Nouvelle frame : oublie les zones d'images de la précédente.
    pub fn begin_frame(&mut self) {
        self.drawn.clear();
    }

    /// Zones occupées par des images dans la frame courante (les effets
    /// n'y touchent pas).
    /// Protège `area` des effets pour cette frame (scène pixel du hub).
    pub fn protect(&mut self, area: Rect) {
        self.drawn.push(area);
    }

    pub fn drawn(&self) -> &[Rect] {
        &self.drawn
    }

    /// Taille de cellule (pixels) si le terminal parle Kitty, sinon `None`.
    pub fn kitty_cell(&self) -> Option<(u16, u16)> {
        (self.picker.protocol_type() == ProtocolType::Kitty).then(|| self.picker.font_size())
    }

    /// Dessine `creature` centrée dans `area`, au pixel près.
    pub fn draw(&mut self, frame: &mut Frame, area: Rect, creature: &'static Creature) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let key = (creature.slug, area.width, area.height);
        if let Some(hit) = self.cache.get_mut(&key) {
            hit.last_used = Instant::now();
        } else {
            let Some(proto) = self.encode(area, creature) else {
                return;
            };
            self.evict_if_full();
            let hit = CacheEntry {
                proto,
                last_used: Instant::now(),
            };
            self.cache.insert(key, hit);
        }
        if let Some(hit) = self.cache.get(&key) {
            frame.render_widget(Image::new(&hit.proto), area);
            self.drawn.push(area);
        }
    }

    /// Fait de la place : expirés d'abord, sinon la moitié la moins
    /// récente (le cache reste tiède, sans pic de ré-encodage).
    fn evict_if_full(&mut self) {
        if self.cache.len() < CACHE_MAX {
            return;
        }
        self.cache.retain(|_, e| e.last_used.elapsed() < CACHE_TTL);
        if self.cache.len() < CACHE_MAX {
            return;
        }
        let mut oldest: Vec<(CacheKey, Instant)> =
            self.cache.iter().map(|(k, e)| (*k, e.last_used)).collect();
        oldest.sort_by_key(|(_, at)| *at);
        for (key, _) in oldest.into_iter().take(CACHE_MAX / 2) {
            self.cache.remove(&key);
        }
    }

    /// Canevas pixel de la taille exacte de `area` (rien à redimensionner).
    fn encode(&self, area: Rect, creature: &Creature) -> Option<Protocol> {
        let (fw, fh) = self.picker.font_size();
        let w = u32::from(area.width) * u32::from(fw);
        let h = u32::from(area.height) * u32::from(fh);
        let img = bestiary::fit(&creature.sprite().ok()?, w, h);
        self.picker
            .new_protocol(DynamicImage::ImageRgba8(img), area, Resize::Fit(None))
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn halfblocks_map_one_sprite_pixel_per_half_cell() {
        use ratatui::style::Color;
        let mut bank = ImageBank::headless();
        let mut term = Terminal::new(TestBackend::new(32, 16)).unwrap();
        let ghost = bestiary::get("fantome");
        for _ in 0..3 {
            bank.begin_frame();
            term.draw(|f| bank.draw(f, f.area(), ghost)).unwrap();
        }
        assert_eq!(bank.cache.len(), 1);
        assert_eq!(bank.drawn(), &[Rect::new(0, 0, 32, 16)]);
        let sprite = ghost.sprite().unwrap();
        let px = |x: u16, y: u16| {
            let [r, g, b, a] = sprite.get_pixel(u32::from(x), u32::from(y)).0;
            let [vr, vg, vb, _] = VOID_RGBA.0;
            if a == 255 {
                Color::Rgb(r, g, b)
            } else {
                Color::Rgb(vr, vg, vb)
            }
        };
        let buf = term.backend().buffer();
        for (x, y) in (0..32).flat_map(|x| (0..16).map(move |y| (x, y))) {
            let cell = &buf[(x, y)];
            let (up, low) = match cell.symbol() {
                "▀" => (cell.fg, cell.bg),
                "▄" => (cell.bg, cell.fg),
                _ => (cell.bg, cell.bg),
            };
            assert_eq!(
                (up, low),
                (px(x, 2 * y), px(x, 2 * y + 1)),
                "cellule ({x},{y})"
            );
        }
    }

    #[test]
    fn protocol_covers_exactly_the_area() {
        let bank = ImageBank::headless();
        let area = Rect::new(3, 2, 22, 9);
        let proto = bank.encode(area, bestiary::get("crane")).unwrap();
        assert_eq!(proto.area().width, area.width);
        assert_eq!(proto.area().height, area.height);
    }

    #[test]
    fn full_cache_evicts_oldest_half_not_all() {
        let mut bank = ImageBank::headless();
        let mut term = Terminal::new(TestBackend::new(128, 64)).unwrap();
        let ghost = bestiary::get("fantome");
        for w in 1..=100u16 {
            bank.begin_frame();
            term.draw(|f| bank.draw(f, Rect::new(0, 0, w, 1), ghost))
                .unwrap();
        }
        // 96 entrées puis éviction de moitié : 96 - 48 + 4 = 52.
        assert_eq!(bank.cache.len(), 52);
        assert!(bank.cache.contains_key(&("fantome", 100, 1)));
    }
}
