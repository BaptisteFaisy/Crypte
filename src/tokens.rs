//! Compteur de tokens : estimation locale (tiktoken) + livre de session.
//!
//! `estimate` utilise l'encodeur `o200k_base` (GPT-4o/o1) de tiktoken-rs,
//! avec repli heuristique (`len/4`) si l'encodeur est indisponible.
//! `TokenLedger` accumule l'usage réel retourné par l'API (seule source
//! fiable cross-provider) et affiche un tableau de bord session + dernier appel.

use std::sync::OnceLock;

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};
use tiktoken_rs::{o200k_base, CoreBPE};

use crate::theme;

/// Encodeur partagé (lourd, ~32 Mo), chargé une seule fois au premier
/// vrai besoin (`estimate`) puis gardé : le recharger après éviction
/// coûterait plus cher que le garder (cap = 1 entrée, pas de TTL).
/// `None` = construction impossible, `estimate` replie sur l'heuristique.
static ENCODER: OnceLock<Option<CoreBPE>> = OnceLock::new();

/// Heuristique : ~4 caractères par token.
/// Repli d'`estimate`, et estimation du chemin de démarrage (annonce
/// agents.md) pour ne pas charger l'encodeur avant le premier besoin.
pub(crate) fn heuristic(text: &str) -> usize {
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

/// Fenêtre de contexte max (tokens estimés) par envoi vers le provider.
pub const CONTEXT_LIMIT: u64 = 1_000_000;
/// Fallback context window (tokens) for unknown models.
const DEFAULT_CONTEXT_WINDOW: u64 = 128_000;
/// Env override for the context window (`> 0`, else ignored).
const CONTEXT_WINDOW_ENV: &str = "KH_CONTEXT_WINDOW";
/// Known windows by model-name fragment (lowercase, first match wins).
const CONTEXT_TABLE: &[(&str, u64)] = &[
    ("gpt-4.1", 1_000_000),
    ("gemini", 1_000_000),
    ("claude", 200_000),
    ("o1", 200_000),
    ("o3", 200_000),
    ("o4", 200_000),
    ("gpt-4o", 128_000),
    ("gpt-4-turbo", 128_000),
    ("gpt-4-32k", 32_768),
    ("gpt-4", 8_192),
    ("gpt-3.5-turbo-16k", 16_384),
    ("gpt-3.5", 16_384),
    ("mixtral", 32_768),
    ("mistral", 128_000),
    ("deepseek", 128_000),
    ("qwen", 128_000),
    ("llama", 128_000),
    ("muse-spark", 128_000),
    ("64k", 65_536),
    ("32k", 32_768),
    ("16k", 16_384),
    ("8k", 8_192),
];

/// Context window (tokens) of `model`: `KH_CONTEXT_WINDOW` wins when set
/// to an integer `> 0`, else the first [`CONTEXT_TABLE`] fragment found in
/// the lowercased name, else [`DEFAULT_CONTEXT_WINDOW`].
pub fn context_window(model: &str) -> u64 {
    if let Some(n) = context_env_override() {
        return n;
    }
    let name = model.to_lowercase();
    for (fragment, window) in CONTEXT_TABLE {
        if name.contains(fragment) {
            return *window;
        }
    }
    DEFAULT_CONTEXT_WINDOW
}

/// `KH_CONTEXT_WINDOW` parsed (`> 0`), else `None` (unset/invalid/zero).
fn context_env_override() -> Option<u64> {
    std::env::var(CONTEXT_WINDOW_ENV)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
}

/// Estime le contexte d'une liste de messages (somme des contenus).
pub fn context_estimate(messages: &[(String, String)]) -> u64 {
    messages
        .iter()
        .map(|(_, content)| estimate(content) as u64)
        .sum()
}

/// Texte de remplissage : `~used / limit (pct%)` (`limit` compact).
pub fn fmt_context(used: u64, limit: u64) -> String {
    let limit = limit.max(1);
    let pct = used as f64 / limit as f64 * 100.0;
    format!("~{used} / {} ({pct:.1}%)", fmt_limit(limit))
}

/// Limite compacte : `1M`, `128k`, sinon le nombre brut.
fn fmt_limit(limit: u64) -> String {
    if limit % 1_000_000 == 0 {
        format!("{}M", limit / 1_000_000)
    } else if limit % 1_000 == 0 {
        format!("{}k", limit / 1_000)
    } else {
        format!("{limit}")
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

    /// Dessine le grimoire : session, dernier appel, coût, débit, et la
    /// part prompt/réponse en jauge (citrouille = réponse).
    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        let label = Style::new().fg(theme::SAND);
        let num = Style::new().fg(theme::BONE).add_modifier(Modifier::BOLD);
        let row = |name: &'static str, value: String| {
            Line::from(vec![
                Span::styled(format!("  {name:<9}"), label),
                Span::styled(value, num),
            ])
        };
        let lines = vec![
            Line::raw(""),
            row(
                "session",
                format!(
                    "in={} out={} tot={}",
                    self.prompt,
                    self.completion,
                    self.total()
                ),
            ),
            row(
                "last",
                format!("in={} out={}", self.last_prompt, self.last_completion),
            ),
            row("cost", fmt_cost(self.cost())),
            row(
                "tps",
                format!(
                    "last={} avg={}",
                    fmt_tps(self.last_tps()),
                    fmt_tps(self.avg_tps())
                ),
            ),
            Line::raw(""),
            self.share_gauge(area.width.saturating_sub(6) as usize),
        ];
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(theme::UMBER))
            .title(theme::title(
                "\u{f068c} token grimoire",
                theme::BLOOD,
                theme::CANDLE,
            ));
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }

    /// Jauge prompt (os) / réponse (citrouille) sur `width` colonnes.
    fn share_gauge(&self, width: usize) -> Line<'static> {
        let total = self.total();
        if total == 0 || width == 0 {
            let dim = Style::new().fg(theme::ASH).add_modifier(Modifier::ITALIC);
            return Line::styled("  no souls consumed yet", dim);
        }
        let out = (self.completion as f64 / total as f64 * width as f64).round() as usize;
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "█".repeat(width - out.min(width)),
                Style::new().fg(theme::UMBER),
            ),
            Span::styled("█".repeat(out.min(width)), Style::new().fg(theme::PUMPKIN)),
        ])
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

    #[test]
    fn context_estimate_sums_all_messages() {
        let msgs = vec![
            (String::from("system"), String::from("abc")),
            (String::from("user"), String::from("Hello, world!")),
        ];
        let used = context_estimate(&msgs);
        assert!(used > 0);
        assert_eq!(used, (estimate("abc") + estimate("Hello, world!")) as u64);
        assert!(used < CONTEXT_LIMIT);
    }

    #[test]
    fn fmt_context_shows_limit_and_percent() {
        assert_eq!(fmt_context(500_000, 1_000_000), "~500000 / 1M (50.0%)");
        assert_eq!(fmt_context(0, 1_000_000), "~0 / 1M (0.0%)");
        assert_eq!(fmt_context(64_000, 128_000), "~64000 / 128k (50.0%)");
        assert_eq!(fmt_context(0, 200_000), "~0 / 200k (0.0%)");
    }

    #[test]
    fn context_window_matches_known_models_and_env_override() {
        let saved = std::env::var(CONTEXT_WINDOW_ENV).ok();
        std::env::remove_var(CONTEXT_WINDOW_ENV);
        assert_eq!(context_window("gpt-4.1"), 1_000_000);
        assert_eq!(context_window("openai/gpt-4o-mini"), 128_000);
        assert_eq!(context_window("claude-opus-4-5"), 200_000);
        assert_eq!(context_window("o3-mini"), 200_000);
        assert_eq!(context_window("gpt-4"), 8_192);
        assert_eq!(context_window("muse-spark-1.3-contributor"), 128_000);
        assert_eq!(context_window("some-unknown-model"), 128_000);
        std::env::set_var(CONTEXT_WINDOW_ENV, "64000");
        assert_eq!(context_window("gpt-4.1"), 64_000);
        std::env::set_var(CONTEXT_WINDOW_ENV, "0");
        assert_eq!(context_window("gpt-4.1"), 1_000_000);
        std::env::set_var(CONTEXT_WINDOW_ENV, "nope");
        assert_eq!(context_window("gpt-4.1"), 1_000_000);
        match saved {
            Some(v) => std::env::set_var(CONTEXT_WINDOW_ENV, v),
            None => std::env::remove_var(CONTEXT_WINDOW_ENV),
        }
    }
}
