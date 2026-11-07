//! Compteur de tokens : estimation locale (tiktoken) + livre de session.
//!
//! `estimate` utilise l'encodeur `o200k_base` (GPT-4o/o1) de tiktoken-rs,
//! avec repli heuristique (`len/4`) si l'encodeur est indisponible.
//! `TokenLedger` accumule l'usage réel retourné par l'API (seule source
//! fiable cross-provider) et affiche un tableau de bord session + dernier appel.

use std::sync::OnceLock;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use tiktoken_rs::{o200k_base, CoreBPE};

/// Encodeur partagé (lourd, ~32 Mo) initialisé une seule fois.
/// `None` = construction impossible, `estimate` replie sur l'heuristique.
static ENCODER: OnceLock<Option<CoreBPE>> = OnceLock::new();

/// Heuristique de repli : ~4 caractères par token.
fn heuristic(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// Estime le nombre de tokens de `text` via `o200k_base`.
///
/// Repli sur `len/4` si l'encodeur ne peut pas être construit.
pub fn estimate(text: &str) -> usize {
    match ENCODER.get_or_init(|| o200k_base().ok()) {
        Some(bpe) => bpe.encode_ordinary(text).len(),
        None => heuristic(text),
    }
}

/// Registre d'usage tokens sur la session.
#[derive(Debug, Clone)]
pub struct TokenLedger {
    /// Tokens prompt cumulés (session).
    pub prompt: u64,
    /// Tokens completion cumulés (session).
    pub completion: u64,
    last_prompt: u64,
    last_completion: u64,
    /// Débit du dernier appel (tok/s), 0 si aucun appel ou durée nulle.
    last_tps: f64,
    /// Durées cumulées des appels chronométrés (secondes).
    secs: f64,
    /// Tokens completion cumulés des appels chronométrés.
    timed_completion: u64,
    /// Tarif $ / 1k tokens prompt (0 = non renseigné).
    price_in_per_1k: f64,
    /// Tarif $ / 1k tokens completion (0 = non renseigné).
    price_out_per_1k: f64,
}

impl TokenLedger {
    /// Nouveau registre vide, tarifs à 0 (coût estimé désactivé).
    pub fn new() -> Self {
        Self {
            prompt: 0,
            completion: 0,
            last_prompt: 0,
            last_completion: 0,
            last_tps: 0.0,
            secs: 0.0,
            timed_completion: 0,
            price_in_per_1k: 0.0,
            price_out_per_1k: 0.0,
        }
    }

    /// Fixe les tarifs ($ / 1k tokens) pour le coût estimé.
    #[allow(dead_code)]
    pub fn with_pricing(mut self, price_in_per_1k: f64, price_out_per_1k: f64) -> Self {
        self.price_in_per_1k = price_in_per_1k.max(0.0);
        self.price_out_per_1k = price_out_per_1k.max(0.0);
        self
    }

    /// Enregistre un appel : cumule la session, mémorise le dernier appel.
    pub fn add(&mut self, prompt: u64, completion: u64) {
        self.prompt = self.prompt.saturating_add(prompt);
        self.completion = self.completion.saturating_add(completion);
        self.last_prompt = prompt;
        self.last_completion = completion;
    }

    /// Total session (prompt + completion).
    pub fn total(&self) -> u64 {
        self.prompt.saturating_add(self.completion)
    }

    /// Dernier appel (prompt, completion).
    pub fn last(&self) -> (u64, u64) {
        (self.last_prompt, self.last_completion)
    }

    /// Coût estimé de la session en $ (0 si tarifs non renseignés).
    pub fn cost(&self) -> f64 {
        self.prompt as f64 / 1000.0 * self.price_in_per_1k
            + self.completion as f64 / 1000.0 * self.price_out_per_1k
    }

    /// Enregistre un appel chronométré : `add` + débit (completion / durée).
    ///
    /// `elapsed_secs <= 0` donne un débit nul et n'alimente pas la moyenne.
    pub fn record(&mut self, prompt: u64, completion: u64, elapsed_secs: f64) {
        self.add(prompt, completion);
        if elapsed_secs > 0.0 {
            self.last_tps = completion as f64 / elapsed_secs;
            self.secs += elapsed_secs;
            self.timed_completion = self.timed_completion.saturating_add(completion);
        } else {
            self.last_tps = 0.0;
        }
    }

    /// Débit du dernier appel en tok/s (0.0 si aucun appel).
    pub fn last_tps(&self) -> f64 {
        self.last_tps
    }

    /// Débit moyen en tok/s (completion / secondes, 0.0 si vide).
    pub fn avg_tps(&self) -> f64 {
        if self.secs > 0.0 {
            self.timed_completion as f64 / self.secs
        } else {
            0.0
        }
    }

    /// Agrège plusieurs registres : sommes in/out/durées, tarifs à 0.
    pub fn total_of(ledgers: &[TokenLedger]) -> TokenLedger {
        let mut total = TokenLedger::new();
        for ledger in ledgers {
            total.prompt = total.prompt.saturating_add(ledger.prompt);
            total.completion = total.completion.saturating_add(ledger.completion);
            total.secs += ledger.secs;
            total.timed_completion = total
                .timed_completion
                .saturating_add(ledger.timed_completion);
        }
        total
    }

    /// Dessine le tableau de bord : session + dernier appel.
    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        let title = Style::new()
            .fg(Color::Rgb(0xE8, 0xE0, 0xC8))
            .add_modifier(Modifier::BOLD);
        let dim = Style::new().fg(Color::Rgb(0x6B, 0x6B, 0x75));
        let num = Style::new().fg(Color::Rgb(0x39, 0xFF, 0x6A));
        let lines = vec![
            Line::from(vec![
                Span::styled("session  ", title),
                Span::styled(
                    format!(
                        "in={} out={} tot={}",
                        self.prompt,
                        self.completion,
                        self.total()
                    ),
                    num,
                ),
            ]),
            Line::from(vec![
                Span::styled("dernier  ", dim),
                Span::styled(
                    format!("in={} out={}", self.last_prompt, self.last_completion),
                    Style::new().fg(Color::Rgb(0xE8, 0xE0, 0xC8)),
                ),
            ]),
            Line::from(vec![
                Span::styled("coût     ", dim),
                Span::raw(fmt_cost(self.cost())),
            ]),
            Line::from(vec![
                Span::styled("tps      ", dim),
                Span::raw(format!(
                    "dernier={} moy={}",
                    fmt_tps(self.last_tps()),
                    fmt_tps(self.avg_tps())
                )),
            ]),
        ];
        let block = Block::new()
            .borders(Borders::ALL)
            .title("◆ tokens")
            .border_style(Style::new().fg(Color::Rgb(0x6B, 0x6B, 0x75)));
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }
}

impl Default for TokenLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// Texte du coût : montant en $ ou tiret si tarifs non renseignés.
pub fn fmt_cost(cost: f64) -> String {
    if cost > 0.0 {
        format!("${cost:.4}")
    } else {
        String::from("—")
    }
}

/// Texte d'un débit : `x.x tok/s` ou tiret si nul.
pub fn fmt_tps(tps: f64) -> String {
    if tps > 0.0 {
        format!("{tps:.1} tok/s")
    } else {
        String::from("—")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_non_empty_text_is_nonzero() {
        assert!(estimate("Hello, world!") > 0);
    }

    #[test]
    fn record_tracks_last_and_avg_tps() {
        let mut ledger = TokenLedger::new();
        assert_eq!(ledger.last_tps(), 0.0);
        assert_eq!(ledger.avg_tps(), 0.0);
        ledger.record(10, 100, 2.0);
        assert_eq!(ledger.last_tps(), 50.0);
        assert_eq!(ledger.avg_tps(), 50.0);
        assert_eq!(ledger.total(), 110);
        ledger.record(10, 200, 2.0);
        assert_eq!(ledger.last_tps(), 100.0);
        assert_eq!(ledger.avg_tps(), 75.0);
    }

    #[test]
    fn total_of_sums_two_ledgers() {
        let mut a = TokenLedger::new();
        a.record(10, 100, 2.0);
        let mut b = TokenLedger::new();
        b.record(30, 200, 2.0);
        let total = TokenLedger::total_of(&[a, b]);
        assert_eq!(total.prompt, 40);
        assert_eq!(total.completion, 300);
        assert_eq!(total.total(), 340);
        assert_eq!(total.last(), (0, 0));
        assert_eq!(total.cost(), 0.0);
        assert_eq!(total.avg_tps(), 75.0);
    }
}
