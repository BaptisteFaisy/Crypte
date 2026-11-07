//! Chats multiples + sidebar gauche.
//!
//! `Sessions` garde les chats ouverts et le courant ; `draw_sidebar` rend
//! la liste des chats (nom + tokens + `…` si envoi en cours + ` A{n}` si
//! sous-agents actifs) dans la colonne gauche, chat courant en vidéo inverse.

use std::time::Instant;

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::chat::ChatState;
use crate::tokens::TokenLedger;

/// Un chat : affichage, historique provider, usage tokens, envoi en cours.
pub struct ChatSession {
    pub name: String,
    pub chat: ChatState,
    pub history: Vec<(String, String)>,
    pub ledger: TokenLedger,
    pub sending: bool,
    pub sending_since: Option<Instant>,
    /// Modèle du chat (`None` = défaut provider).
    pub model: Option<String>,
    /// Sous-agents lancés depuis ce chat.
    pub agents: Vec<SubAgent>,
    /// Vrai si `history[0]` est le contexte agents.md pinné.
    pub pinned_system: bool,
}

impl ChatSession {
    /// Nouveau chat vide avec ce nom.
    pub fn new(name: String) -> Self {
        Self {
            name,
            chat: ChatState::new(),
            history: Vec::new(),
            ledger: TokenLedger::new(),
            sending: false,
            sending_since: None,
            model: None,
            agents: Vec::new(),
            pinned_system: false,
        }
    }

    /// Épingle le contexte agents.md en tête d'historique (chat neuf).
    pub fn pin_system(&mut self, content: &str) {
        self.history
            .push((String::from("system"), content.to_string()));
        self.pinned_system = true;
    }
}

/// Statut d'un sous-agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Running,
    Done,
    Failed,
}

impl AgentStatus {
    /// Étiquette courte française (liste `/agents`).
    pub fn label(self) -> &'static str {
        match self {
            AgentStatus::Running => "en cours",
            AgentStatus::Done => "terminé",
            AgentStatus::Failed => "échoué",
        }
    }
}

/// Un sous-agent : requête annexe dont la réponse arrive en async.
#[derive(Debug, Clone)]
pub struct SubAgent {
    pub id: usize,
    /// Extrait du prompt (tronqué à la création).
    pub prompt: String,
    pub status: AgentStatus,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

impl SubAgent {
    /// Nouveau sous-agent `Running`, prompt tronqué à 60 caractères.
    pub fn new(id: usize, prompt: &str) -> Self {
        Self {
            id,
            prompt: prompt.chars().take(AGENT_PROMPT_LEN).collect(),
            status: AgentStatus::Running,
            prompt_tokens: 0,
            completion_tokens: 0,
        }
    }
}

/// Longueur max de l'extrait de prompt d'un sous-agent.
const AGENT_PROMPT_LEN: usize = 60;

/// Chats ouverts + position du courant (jamais vide, index toujours valide).
pub struct Sessions {
    chats: Vec<ChatSession>,
    current: usize,
    next_agent_id: usize,
}

impl Sessions {
    /// Une session avec un seul chat « Chat 1 ».
    pub fn new() -> Self {
        Self {
            chats: vec![ChatSession::new(String::from("Chat 1"))],
            current: 0,
            next_agent_id: 1,
        }
    }

    /// Chat courant (lecture).
    pub fn current(&self) -> &ChatSession {
        &self.chats[self.current]
    }

    /// Chat courant (écriture).
    pub fn current_mut(&mut self) -> &mut ChatSession {
        &mut self.chats[self.current]
    }

    /// Chat par index, mutable (`None` si hors bornes, réponses async périmées).
    pub fn get_mut(&mut self, id: usize) -> Option<&mut ChatSession> {
        self.chats.get_mut(id)
    }

    /// Crée un sous-agent `Running` sur le chat `idx`, rend son id unique.
    /// `None` si hors bornes (l'id n'est pas consommé).
    pub fn add_agent(&mut self, idx: usize, prompt: &str) -> Option<usize> {
        let id = self.next_agent_id;
        let chat = self.chats.get_mut(idx)?;
        chat.agents.push(SubAgent::new(id, prompt));
        self.next_agent_id += 1;
        Some(id)
    }

    /// Nombre de sous-agents en cours sur le chat `idx` (0 si hors bornes).
    pub fn running_agents(&self, idx: usize) -> usize {
        self.chats
            .get(idx)
            .map(|c| {
                c.agents
                    .iter()
                    .filter(|a| a.status == AgentStatus::Running)
                    .count()
            })
            .unwrap_or(0)
    }

    /// Nouveau chat « Chat N » qui devient courant.
    pub fn add(&mut self) {
        let n = self.chats.len() + 1;
        self.chats.push(ChatSession::new(format!("Chat {n}")));
        self.current = self.chats.len() - 1;
    }

    /// Chat précédent (cyclique).
    pub fn prev(&mut self) {
        self.current = self.current.checked_sub(1).unwrap_or(self.chats.len() - 1);
    }

    /// Chat suivant (cyclique).
    pub fn next(&mut self) {
        self.current = (self.current + 1) % self.chats.len();
    }

    /// Index du chat courant (0-based).
    pub fn index(&self) -> usize {
        self.current
    }

    /// Nombre de chats ouverts.
    pub fn count(&self) -> usize {
        self.chats.len()
    }

    /// Registre cumulé = somme des registres des chats.
    pub fn total_ledger(&self) -> TokenLedger {
        let ledgers: Vec<TokenLedger> = self.chats.iter().map(|c| c.ledger.clone()).collect();
        TokenLedger::total_of(&ledgers)
    }

    /// Sidebar : une ligne par chat (nom + tokens + `…` si envoi + ` A{n}`).
    pub fn draw_sidebar(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default().borders(Borders::ALL).title("CHATS");
        let inner_w = area.width.saturating_sub(2) as usize;
        if inner_w == 0 {
            frame.render_widget(block, area);
            return;
        }
        let lines: Vec<Line<'static>> = self
            .chats
            .iter()
            .enumerate()
            .map(|(i, c)| {
                sidebar_line(
                    i == self.current,
                    &c.name,
                    c.ledger.total(),
                    c.sending,
                    self.running_agents(i),
                    inner_w,
                )
            })
            .collect();
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }
}

impl Default for Sessions {
    fn default() -> Self {
        Self::new()
    }
}

/// Une ligne de sidebar : nom tronqué + `123t` + `…` si envoi + ` A{n}`.
/// Le chat courant est en vidéo inverse ; le nom rétrécit pour faire place.
fn sidebar_line(
    current: bool,
    name: &str,
    total: u64,
    sending: bool,
    agents: usize,
    width: usize,
) -> Line<'static> {
    let mut right = format!(" {total}t");
    if agents > 0 {
        right.push_str(&format!(" A{agents}"));
    }
    let suffix = if sending { "…" } else { "" };
    let name_w = width.saturating_sub(right.chars().count() + suffix.chars().count());
    let name: String = name.chars().take(name_w).collect();
    let pad = name_w.saturating_sub(name.chars().count());
    let text = format!("{name}{:pad$}{right}{suffix}", "", pad = pad);
    let style = if current {
        Style::new().add_modifier(Modifier::REVERSED)
    } else {
        Style::new()
    };
    Line::styled(text, style)
}
