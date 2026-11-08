//! Historique des messages + saisie éditable (thème crypte).
//!
//! `ChatState` garde les messages et le brouillon `input` ; `draw` rend
//! l'historique (gouttière colorée par rôle, retour à la ligne par mot,
//! cadré sur les derniers messages, **défilable**) au-dessus d'une
//! saisie de 3 lignes.

use std::borrow::Cow;
use std::cell::Cell;

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};
use serde::{Deserialize, Serialize};

use crate::scroll::Scroll;
use crate::theme::{self, ASH, BONE, CANDLE, PUMPKIN, SAND, UMBER};

/// Historique borné (les plus vieux sortent au-delà du cap).
const MAX_MESSAGES: usize = 200;
/// Hauteur fixe (bordures incluses) de la zone de saisie.
pub(crate) const INPUT_HEIGHT: u16 = 3;
/// Gouttière : barre colorée + espace devant chaque ligne.
pub(crate) const GUTTER: &str = "▍ ";
/// Lignes par cran de molette.
pub const SCROLL_STEP: isize = 3;
/// Lignes par page clavier (PgUp/PgDn).
pub const SCROLL_PAGE: isize = 10;

/// Émetteur d'un message.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

/// Un message : rôle + texte brut (wrappé seulement au rendu).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

/// Contexte de rendu : titre, horloge d'animation, attente, question IA.
#[derive(Clone, Copy, Debug)]
pub struct ChatView<'a> {
    pub title: &'a str,
    pub tick: u64,
    pub sending: bool,
    pub question_pending: bool,
}

/// Historique + brouillon de saisie (+ prompt custom éventuel).
#[derive(Debug, Default)]
pub struct ChatState {
    messages: Vec<Message>,
    pub input: String,
    prompt_label: Option<String>,
    mask_input: bool,
    input_preview: Option<String>,
    /// Décalage de l'historique (0 = collé aux derniers messages).
    pub scroll: Scroll,
    /// Rectangle de la colonne de discussion (mémorisé au rendu) :
    /// la molette ne défile que le chat survolé.
    pub area: Cell<Rect>,
    /// Version de `messages` (bump à chaque `push`, jamais remise à zéro).
    wrap_version: u64,
    /// Lignes wrappées : (version, largeur, lignes), `None` = à calculer.
    wrap_cache: Option<(u64, usize, Vec<Line<'static>>)>,
    /// Sélection souris en cours (copiée au relâchement, `None` sinon).
    pub(crate) select: Option<crate::clip::Selection>,
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
        self.wrap_version = self.wrap_version.wrapping_add(1);
    }

    /// Lecture seule de l'historique (pour le provider LLM).
    #[allow(dead_code)]
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Vide l'historique affiché, recolle le défilement en bas et
    /// invalide le cache de rendu (`/clear`).
    pub fn clear(&mut self) {
        self.messages.clear();
        self.scroll.to_bottom();
        self.wrap_version = self.wrap_version.wrapping_add(1);
    }

    /// Octets heap estimés (messages + brouillon + prompt + cache ; analytique).
    pub fn heap_bytes(&self) -> usize {
        let mut bytes = self.messages.capacity() * std::mem::size_of::<Message>()
            + self
                .messages
                .iter()
                .map(|message| message.text.capacity())
                .sum::<usize>();
        if let Some((_, _, lines)) = &self.wrap_cache {
            bytes += lines.capacity() * std::mem::size_of::<Line<'static>>();
            bytes += lines
                .iter()
                .flat_map(|line| line.spans.iter())
                .map(|span| span.content.len())
                .sum::<usize>();
        }
        bytes += self.input.capacity();
        bytes += self
            .prompt_label
            .as_ref()
            .map(|s| s.capacity())
            .unwrap_or(0);
        bytes += self
            .input_preview
            .as_ref()
            .map(|s| s.capacity())
            .unwrap_or(0);
        bytes
    }

    /// Tape un caractère dans le brouillon.
    pub fn input_push(&mut self, c: char) {
        self.input.push(c);
    }

    /// Efface le dernier caractère du brouillon.
    pub fn input_pop(&mut self) {
        self.input.pop();
    }

    /// Colle `s` (déjà assaini) à la fin du brouillon.
    pub fn input_insert(&mut self, s: &str) {
        self.input.push_str(s);
    }

    /// Lignes visibles en texte brut (gouttière retirée) pour la sélection.
    pub(crate) fn visible_plain(&mut self, sending: bool) -> Vec<String> {
        let inner = crate::clip::history_inner(self.area.get());
        if inner.width == 0 || inner.height == 0 {
            return Vec::new();
        }
        let width = inner.width as usize;
        let extra = usize::from(sending) * 2;
        let total = self.wrapped_lines(width).len().saturating_add(extra);
        let (skip, _) = self.scroll.viewport(total, inner.height as usize);
        self.wrapped_lines(width)
            .iter()
            .skip(skip)
            .take(inner.height as usize)
            .map(crate::clip::plain_text)
            .collect()
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

    /// Lignes wrappées de l'historique pour `width` : recalculées
    /// seulement si les messages ou la largeur ont changé (sinon la
    /// frappe paierait un re-wrap complet à chaque frame).
    fn wrapped_lines(&mut self, width: usize) -> &[Line<'static>] {
        let fresh = self
            .wrap_cache
            .as_ref()
            .is_some_and(|(v, w, _)| *v == self.wrap_version && *w == width);
        if !fresh {
            let lines = history_lines(&self.messages, width);
            self.wrap_cache = Some((self.wrap_version, width, lines));
        }
        &self.wrap_cache.as_ref().expect("cache rempli ci-dessus").2
    }

    /// Clé du cache de wrap (`None` = vide), pour les tests.
    #[cfg(test)]
    pub(crate) fn wrap_cache_key(&self) -> Option<(u64, usize)> {
        self.wrap_cache.as_ref().map(|(v, w, _)| (*v, *w))
    }

    /// Rend historique + saisie dans `area`.
    pub fn draw(&mut self, frame: &mut Frame, area: Rect, view: ChatView) {
        self.area.set(area);
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
        self.draw_input(frame, input, view.question_pending);
    }

    /// Historique : bloc arrondi, défilable (molette / PgUp / Shift+↑↓)
    /// avec badge des lignes remontées ; ne clone que la fenêtre visible.
    fn draw_history(&mut self, frame: &mut Frame, area: Rect, view: ChatView) {
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
        if inner.width < 4 || inner.height == 0 {
            frame.render_widget(block, area);
            return;
        }
        if self.messages.is_empty() && !view.sending {
            frame.render_widget(block, area);
            frame.render_widget(epitaph(inner.height), inner);
            return;
        }
        let width = inner.width as usize;
        let extra = usize::from(view.sending) * 2;
        let sel = self.select;
        let total = self.wrapped_lines(width).len().saturating_add(extra);
        let (skip, scrolled) = self.scroll.viewport(total, inner.height as usize);
        // Second appel = hit (calculé ci-dessus), pour libérer `scroll`.
        let base = self.wrapped_lines(width);
        let mut visible: Vec<Line<'static>> = base.iter().skip(skip).cloned().collect();
        if view.sending {
            let tail = [Line::raw(""), murmur_line(view.tick)];
            let drop = skip.saturating_sub(base.len()).min(2);
            visible.extend(tail.into_iter().skip(drop));
        }
        if let Some(sel) = &sel {
            let lens: Vec<usize> = base
                .iter()
                .skip(skip)
                .take(visible.len())
                .map(|l| crate::clip::plain_text(l).chars().count())
                .collect();
            crate::clip::highlight(&mut visible, &lens, inner, sel);
        }
        let block = if scrolled == 0 {
            block
        } else {
            block.title_bottom(scroll_badge(scrolled))
        };
        frame.render_widget(block, area);
        frame.render_widget(Paragraph::new(visible), inner);
    }

    /// Saisie : brouillon (ou aperçu) cadré sur la fin + curseur bougie fixe.
    /// Sans titre par défaut ; le label custom et le masquage (`•`)
    /// servent au flow `/connect` (seul cas avec un titre).
    /// Question IA en attente : l'invite suggère chiffre ou mots.
    fn draw_input(&self, frame: &mut Frame, area: Rect, question: bool) {
        let mut block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(CANDLE));
        if let Some(label) = &self.prompt_label {
            block = block.title(Line::from(label.clone()));
        }
        let inner_w = area.width.saturating_sub(3) as usize;
        let cursor = Span::styled("▌", Style::new().fg(CANDLE));
        let raw = self.input_preview.as_ref().unwrap_or(&self.input);
        let line = if raw.is_empty() {
            let hint = Style::new().fg(ASH).add_modifier(Modifier::ITALIC);
            let text = if question {
                "number or words to answer…"
            } else {
                "whisper to the crypt…"
            };
            Line::from(vec![cursor, Span::styled(text, hint)])
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

/// Badge de défilement : « ↕ 12 lines ↓ » sur le bord bas du cadre.
fn scroll_badge(scroll: usize) -> Line<'static> {
    let unit = if scroll == 1 { "line" } else { "lines" };
    Line::styled(
        format!(" \u{2195} {scroll} {unit} \u{2193} "),
        Style::new().fg(PUMPKIN).add_modifier(Modifier::BOLD),
    )
    .right_aligned()
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
mod tests;
