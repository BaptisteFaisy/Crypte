//! Historique des messages + saisie éditable (thème crypte).
//!
//! `ChatState` garde les messages et le brouillon `input` ; `draw` rend
//! l'historique (pré-wrappé, cadré sur les derniers messages) au-dessus
//! d'une zone de saisie de 3 lignes.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Texte pâle (os) : messages du joueur.
const BONE: Color = Color::Rgb(232, 224, 200);
/// Sang frais : étiquette de l'assistant.
const FRESH_BLOOD: Color = Color::Rgb(220, 20, 60);
/// Ectoplasme vert spectral : corps des réponses.
const ECTOPLASM: Color = Color::Rgb(138, 255, 128);
/// Cendre grise : messages système.
const ASH: Color = Color::Rgb(107, 107, 117);
/// Bougie : bordure de saisie.
const CANDLE: Color = Color::Rgb(255, 179, 71);

/// Historique borné (les plus vieux sortent au-delà du cap).
const MAX_MESSAGES: usize = 200;
/// Hauteur fixe (bordures incluses) de la zone de saisie.
const INPUT_HEIGHT: u16 = 3;

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

/// Historique + brouillon de saisie.
#[derive(Debug, Default)]
pub struct ChatState {
    messages: Vec<Message>,
    pub input: String,
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

    /// Rend historique scrollé + saisie dans `area`.
    pub fn draw(&self, frame: &mut Frame, area: Rect) {
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
        self.draw_history(frame, hist);
        self.draw_input(frame, input);
    }

    /// Historique : lignes wrappées, cadrées sur les derniers messages.
    fn draw_history(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(FRESH_BLOOD))
            .title(" ☠ crypt-chat ");
        let inner_w = area.width.saturating_sub(2) as usize;
        let inner_h = area.height.saturating_sub(2) as usize;
        if inner_w == 0 || inner_h == 0 {
            frame.render_widget(block, area);
            return;
        }
        let lines = history_lines(&self.messages, inner_w);
        let skip = lines.len().saturating_sub(inner_h);
        let visible: Vec<Line<'static>> = lines.into_iter().skip(skip).collect();
        frame.render_widget(Paragraph::new(visible).block(block), area);
    }

    /// Saisie : brouillon cadré sur la fin + curseur bougie.
    fn draw_input(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(CANDLE))
            .title(" ▶ parler… (Entrée = envoyer) ");
        let inner_w = area.width.saturating_sub(2) as usize;
        let text = tail_chars(&self.input, inner_w.max(1));
        let line = Line::from(vec![
            Span::styled(text, Style::new().fg(BONE)),
            Span::styled("▌", Style::new().fg(CANDLE).add_modifier(Modifier::BOLD)),
        ]);
        frame.render_widget(Paragraph::new(line).block(block), area);
    }
}

/// Étiquette affichée en tête de chaque message.
fn role_tag(role: Role) -> &'static str {
    match role {
        Role::User => "▸ toi",
        Role::Assistant => "☠ crypte",
        Role::System => "— sys",
    }
}

/// Style du corps d'un message selon son rôle.
fn role_style(role: Role) -> Style {
    match role {
        Role::User => Style::new().fg(BONE),
        Role::Assistant => Style::new().fg(ECTOPLASM),
        Role::System => Style::new().fg(ASH).add_modifier(Modifier::DIM),
    }
}

/// Style de l'étiquette (l'assistant saigne, le reste suit le corps).
fn tag_style(role: Role) -> Style {
    match role {
        Role::Assistant => Style::new().fg(FRESH_BLOOD).add_modifier(Modifier::BOLD),
        _ => role_style(role).add_modifier(Modifier::BOLD),
    }
}

/// Découpe `text` en lignes de `width` caractères max (wrap simple, coupe dure).
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for raw in text.split('\n') {
        let chars: Vec<char> = raw.chars().collect();
        if chars.is_empty() {
            out.push(String::new());
            continue;
        }
        for chunk in chars.chunks(width) {
            out.push(chunk.iter().collect());
        }
    }
    out
}

/// Tous les messages en lignes wrappées, étiquette rôle en tête de message.
fn history_lines(messages: &[Message], width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for msg in messages {
        lines.push(Line::from(Span::styled(
            role_tag(msg.role).to_string(),
            tag_style(msg.role),
        )));
        let body = role_style(msg.role);
        for row in wrap_text(&msg.text, width) {
            lines.push(Line::from(Span::styled(row, body)));
        }
    }
    lines
}

/// Garde les `n` derniers caractères (cadrage saisie longue sur la fin).
fn tail_chars(s: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    let skip = chars.len().saturating_sub(n);
    chars.into_iter().skip(skip).collect()
}
