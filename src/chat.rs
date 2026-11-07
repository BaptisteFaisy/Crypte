//! Historique des messages + saisie éditable (thème crypte).
//!
//! `ChatState` garde les messages et le brouillon `input` ; `draw` rend
//! l'historique (gouttière colorée par rôle, retour à la ligne par mot,
//! cadré sur les derniers messages) au-dessus d'une saisie de 3 lignes.

use std::borrow::Cow;

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::theme::{self, ASH, BONE, CANDLE, PUMPKIN, SAND, UMBER};

/// Historique borné (les plus vieux sortent au-delà du cap).
const MAX_MESSAGES: usize = 200;
/// Hauteur fixe (bordures incluses) de la zone de saisie.
const INPUT_HEIGHT: u16 = 3;
/// Gouttière : barre colorée + espace devant chaque ligne.
const GUTTER: &str = "▍ ";

/// Émetteur d'un message.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    User,
    Assistant,
    System,
}

/// Un message : rôle + texte brut (wrappé seulement au rendu).
#[derive(Clone, Debug)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

/// Contexte de rendu : titre du chat, horloge d'animation, attente.
#[derive(Clone, Copy, Debug)]
pub struct ChatView<'a> {
    pub title: &'a str,
    pub tick: u64,
    pub sending: bool,
}

/// Historique + brouillon de saisie (+ prompt custom éventuel).
#[derive(Debug, Default)]
pub struct ChatState {
    messages: Vec<Message>,
    pub input: String,
    prompt_label: Option<String>,
    mask_input: bool,
    input_preview: Option<String>,
}

impl ChatState {
    /// État vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Ajoute un message à l'historique.
    pub fn push(&mut self, role: Role, text: impl Into<String>) {
        self.messages.push(Message {
            role,
            text: text.into(),
        });
        let overflow = self.messages.len().saturating_sub(MAX_MESSAGES);
        if overflow > 0 {
            self.messages.drain(..overflow);
        }
    }

    /// Lecture seule de l'historique (pour le provider LLM).
    #[allow(dead_code)]
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Tape un caractère dans le brouillon.
    pub fn input_push(&mut self, c: char) {
        self.input.push(c);
    }

    /// Efface le dernier caractère du brouillon.
    pub fn input_pop(&mut self) {
        self.input.pop();
    }

    /// Prend le brouillon (le vide) pour l'envoyer.
    pub fn take_input(&mut self) -> String {
        std::mem::take(&mut self.input)
    }

    /// Change le label de saisie (`None` = défaut) + masquage (`•`).
    /// Sert au flow `/connect` (clé masquée, modèle explicite).
    pub fn set_prompt(&mut self, label: Option<String>, mask: bool) {
        self.prompt_label = label;
        self.mask_input = mask;
    }

    /// Texte affiché à la place du brouillon (`None` = brouillon).
    /// Le flow `/connect` y montre son buffer (brouillon préservé).
    pub fn set_input_preview(&mut self, preview: Option<String>) {
        self.input_preview = preview;
    }

    /// Rend historique + saisie dans `area`.
    pub fn draw(&self, frame: &mut Frame, area: Rect, view: ChatView) {
        let hist_h = area.height.saturating_sub(INPUT_HEIGHT);
        let hist = Rect {
            height: hist_h,
            ..area
        };
        let input = Rect {
            y: area.y.saturating_add(hist_h),
            height: area.height.saturating_sub(hist_h),
            ..area
        };
        self.draw_history(frame, hist, view);
        self.draw_input(frame, input, view.tick);
    }

    /// Historique : bloc arrondi, messages cadrés sur la fin.
    fn draw_history(&self, frame: &mut Frame, area: Rect, view: ChatView) {
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(UMBER))
            .title(theme::title(
                &format!("\u{f068c} {}", view.title),
                PUMPKIN,
                CANDLE,
            ));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width < 4 || inner.height == 0 {
            return;
        }
        if self.messages.is_empty() && !view.sending {
            frame.render_widget(epitaph(inner.height), inner);
            return;
        }
        let mut lines = history_lines(&self.messages, inner.width as usize);
        if view.sending {
            lines.push(Line::raw(""));
            lines.push(murmur_line(view.tick));
        }
        let skip = lines.len().saturating_sub(inner.height as usize);
        lines.drain(..skip);
        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// Saisie : brouillon (ou aperçu) cadré sur la fin + curseur bougie.
    /// Le label custom et le masquage (`•`) servent au flow `/connect`.
    fn draw_input(&self, frame: &mut Frame, area: Rect, tick: u64) {
        let title = match &self.prompt_label {
            Some(label) => Line::from(label.clone()),
            None => Line::styled(" ✒ speak ", Style::new().fg(CANDLE)),
        };
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(CANDLE))
            .title(title)
            .title_bottom(Line::styled(" Enter ", Style::new().fg(ASH)).right_aligned());
        let inner_w = area.width.saturating_sub(3) as usize;
        let cursor = if (tick / 5) % 2 == 0 { "▌" } else { " " };
        let cursor = Span::styled(cursor, Style::new().fg(CANDLE));
        let raw = self.input_preview.as_ref().unwrap_or(&self.input);
        let line = if raw.is_empty() {
            let hint = Style::new().fg(ASH).add_modifier(Modifier::ITALIC);
            Line::from(vec![cursor, Span::styled("whisper to the crypt…", hint)])
        } else {
            let shown = mask_text(raw, self.mask_input);
            let text = tail_chars(&shown, inner_w.max(1));
            Line::from(vec![Span::styled(text, Style::new().fg(BONE)), cursor])
        };
        frame.render_widget(Paragraph::new(line).block(block), area);
    }
}

/// Couleur de gouttière / d'étiquette par rôle.
fn role_colour(role: Role) -> Color {
    match role {
        Role::User => BONE,
        Role::Assistant => PUMPKIN,
        Role::System => ASH,
    }
}

/// Étiquette affichée en tête de chaque message.
fn role_tag(role: Role) -> &'static str {
    match role {
        Role::User => "you",
        Role::Assistant => "\u{f02a0} the crypt",
        Role::System => "whisper",
    }
}

/// Style du corps d'un message selon son rôle.
fn body_style(role: Role) -> Style {
    match role {
        Role::User => Style::new().fg(BONE),
        Role::Assistant => Style::new().fg(theme::lerp(BONE, CANDLE, 0.25)),
        Role::System => Style::new().fg(SAND).add_modifier(Modifier::ITALIC),
    }
}

/// Coupe `text` en lignes de `width` colonnes max, par mot ; un mot plus
/// long que la ligne est coupé net. Les sauts de ligne sont conservés.
/// Une seule passe (largeur suivie, découpe par index d'octets).
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::with_capacity(text.len() / width + 2);
    for raw in text.split('\n') {
        let mut line = String::new();
        let mut line_w = 0usize;
        for word in raw.split(' ') {
            let word_w = word.chars().count();
            if line_w > 0 && line_w + 1 + word_w > width {
                out.push(std::mem::take(&mut line));
                line_w = 0;
            }
            if line_w > 0 {
                line.push(' ');
                line_w += 1;
            }
            line.push_str(word);
            line_w += word_w;
            while line_w > width {
                let cut = line
                    .char_indices()
                    .map(|(i, _)| i)
                    .nth(width)
                    .unwrap_or(line.len());
                let rest = line.split_off(cut);
                out.push(std::mem::replace(&mut line, rest));
                line_w = line.chars().count();
            }
        }
        out.push(line);
    }
    out
}

/// Tous les messages : étiquette, corps à gouttière, ligne vide entre deux.
fn history_lines(messages: &[Message], width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::with_capacity(messages.len().saturating_mul(3).saturating_add(1));
    for (i, msg) in messages.iter().enumerate() {
        if i > 0 {
            lines.push(Line::raw(""));
        }
        let colour = role_colour(msg.role);
        let tag = Style::new().fg(colour).add_modifier(Modifier::BOLD);
        lines.push(Line::from(vec![
            Span::styled(GUTTER, Style::new().fg(colour)),
            Span::styled(role_tag(msg.role), tag),
        ]));
        let gutter = Style::new().fg(theme::lerp(colour, UMBER, 0.45));
        let body_w = width.saturating_sub(GUTTER.chars().count());
        for row in wrap_text(&msg.text, body_w) {
            lines.push(Line::from(vec![
                Span::styled(GUTTER, gutter),
                Span::styled(row, body_style(msg.role)),
            ]));
        }
    }
    lines
}

/// « la crypte murmure » + points animés pendant l'attente.
fn murmur_line(tick: u64) -> Line<'static> {
    let dots = ["·  ", "·· ", "···", " ··", "  ·", "   "][(tick / 3 % 6) as usize];
    Line::from(vec![
        Span::styled(GUTTER, Style::new().fg(PUMPKIN)),
        Span::styled(
            "the crypt murmurs ",
            Style::new().fg(SAND).add_modifier(Modifier::ITALIC),
        ),
        Span::styled(dots, Style::new().fg(CANDLE).add_modifier(Modifier::BOLD)),
    ])
}

/// Accueil d'un chat vide, centré verticalement.
fn epitaph(height: u16) -> Paragraph<'static> {
    let pad = height.saturating_sub(3) / 2;
    let mut lines = vec![Line::raw(""); pad as usize];
    lines.push(Line::from(theme::gradient(
        "The crypt is listening.",
        PUMPKIN,
        CANDLE,
        Modifier::BOLD,
    )));
    lines.push(Line::styled(
        "ask your question · /help for spells",
        Style::new().fg(ASH).add_modifier(Modifier::ITALIC),
    ));
    Paragraph::new(lines).alignment(Alignment::Center)
}

/// Masque `s` (un `•` par caractère) si `mask`, sinon emprunte.
fn mask_text(s: &str, mask: bool) -> Cow<'_, str> {
    if mask {
        Cow::Owned(s.chars().map(|_| '•').collect())
    } else {
        Cow::Borrowed(s)
    }
}

/// Garde les `n` derniers caractères (cadrage saisie longue sur la fin),
/// sans `Vec` transitoire (frontière d'octets trouvée par la fin).
fn tail_chars(s: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    match s.char_indices().rev().take(n).last() {
        Some((start, _)) => s[start..].to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_breaks_on_words_and_cuts_long_ones() {
        assert_eq!(
            wrap_text("le chat noir dort", 8),
            ["le chat", "noir", "dort"]
        );
        assert_eq!(wrap_text("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap_text("a\n\nb", 10), ["a", "", "b"]);
        for w in 1..12 {
            for row in wrap_text("les ossements craquent sous la lune rousse", w) {
                assert!(row.chars().count() <= w, "{row:?} > {w}");
            }
        }
    }

    #[test]
    fn history_has_one_tag_line_per_message() {
        let mut chat = ChatState::new();
        chat.push(Role::User, "bonsoir");
        chat.push(Role::Assistant, "bienvenue dans la crypte");
        let lines = history_lines(chat.messages(), 40);
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn prompt_masked_hides_key_length_only() {
        let mut chat = ChatState::new();
        chat.input.push_str("brouillon");
        chat.set_prompt(Some(String::from("Muse API key (masked)")), true);
        chat.set_input_preview(Some(String::from("sk-secret")));
        assert_eq!(chat.prompt_label.as_deref(), Some("Muse API key (masked)"));
        assert!(chat.mask_input);
        let shown = mask_text(
            chat.input_preview.as_ref().unwrap_or(&chat.input),
            chat.mask_input,
        );
        assert_eq!(shown, "•••••••••");
        assert!(!shown.contains("sk-secret"));
        assert_eq!(chat.input, "brouillon");
        chat.set_prompt(None, false);
        chat.set_input_preview(None);
        assert_eq!(chat.prompt_label, None);
        assert!(!chat.mask_input);
    }
}
