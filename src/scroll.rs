//! Défilement d'un historique : écart en lignes depuis le bas.
//!
//! `Cell` : le rendu corrige la borne réelle (hauteur de la zone) alors
//! qu'il n'a qu'un `&self` ; [`Scroll::viewport`] en déduit la fenêtre
//! des lignes à afficher. Partagé par `chat` (rendu), `mouse` (molette)
//! et `app` (PgUp/PgDn, Shift+↑↓, Fin).

use std::cell::Cell;

/// Plafond du décalage brut (le rendu borne ensuite à l'historique réel).
const MAX: usize = 10_000;

/// Écart de défilement depuis le bas (0 = collé aux derniers messages).
#[derive(Debug, Default)]
pub struct Scroll {
    offset: Cell<usize>,
}

impl Scroll {
    /// Remonte (`delta` < 0) ou redescend l'historique ; jamais sous 0.
    /// La borne haute réelle est appliquée au rendu (hauteur de la zone).
    pub fn by(&self, delta: isize) -> usize {
        // i128 : `-isize::MIN` déborderait en `isize`.
        let want = (self.offset.get() as i128 - delta as i128).clamp(0, MAX as i128) as usize;
        self.offset.set(want);
        want
    }

    /// Recolle l'affichage à la fin (envoi d'un message, touche Fin).
    pub fn to_bottom(&self) {
        self.offset.set(0);
    }

    /// Lignes remontées depuis la fin (0 = vue collée au bas).
    pub fn offset(&self) -> usize {
        self.offset.get()
    }

    /// Fenêtre visible : `(première ligne affichée, décalage effectif)`,
    /// le décalage étant borné à l'historique réel puis mémorisé.
    pub fn viewport(&self, total: usize, height: usize) -> (usize, usize) {
        let max_skip = total.saturating_sub(height);
        let offset = self.offset().min(max_skip);
        self.offset.set(offset);
        (max_skip - offset, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_borne_le_scroll_et_recolle_au_bas() {
        let scroll = Scroll::default();
        // 100 lignes, 10 visibles : 90 de décalage maximum.
        scroll.by(-1000);
        assert_eq!(scroll.offset(), 1000);
        let (skip, offset) = scroll.viewport(100, 10);
        assert_eq!((skip, offset), (0, 90), "borné à l'historique réel");
        assert_eq!(scroll.offset(), 90, "borne mémorisée");
        scroll.to_bottom();
        assert_eq!(scroll.viewport(100, 10), (90, 0));
        scroll.by(-5);
        assert_eq!(scroll.viewport(100, 10), (85, 5));
        // Historique plus court que la zone : rien à défiler.
        assert_eq!(scroll.viewport(4, 10), (0, 0));
        assert_eq!(scroll.viewport(0, 10), (0, 0));
    }

    #[test]
    fn by_remonte_redescend_et_ne_passe_pas_sous_zero() {
        let scroll = Scroll::default();
        assert_eq!(scroll.offset(), 0);
        assert_eq!(scroll.by(-3), 3, "delta négatif = remonter");
        assert_eq!(scroll.by(1), 2);
        assert_eq!(scroll.by(100), 0, "jamais sous 0");
        assert_eq!(scroll.by(-50), 50);
        assert_eq!(scroll.by(-isize::MAX), MAX, "plafond brut");
        scroll.to_bottom();
        assert_eq!(scroll.offset(), 0);
    }
}
