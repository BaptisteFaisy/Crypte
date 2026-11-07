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

/// Échec d'une suite de conversation `/ask` : id inconnu ou déjà en cours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskError {
    Unknown,
    Busy,
}

/// Un sous-agent : requête annexe dont la réponse arrive en async.
/// `history` garde l'échange complet (pour `/ask` : suite de conversation).
#[derive(Debug, Clone)]
pub struct SubAgent {
    pub id: usize,
    /// Extrait du prompt (tronqué à la création).
    pub prompt: String,
    pub status: AgentStatus,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub history: Vec<(String, String)>,
}

impl SubAgent {
    /// Nouveau sous-agent `Running`, prompt tronqué à 60 caractères.
    pub fn new(id: usize, prompt: &str, history: Vec<(String, String)>) -> Self {
        Self {
            id,
            prompt: prompt.chars().take(AGENT_PROMPT_LEN).collect(),
            status: AgentStatus::Running,
            prompt_tokens: 0,
            completion_tokens: 0,
            history,
        }
    }

    /// Ajoute un message user à l'échange (borné, tête system gardée).
    pub fn push_user(&mut self, text: String) {
        self.history.push((String::from("user"), text));
        trim_agent_history(&mut self.history);
    }

    /// Ajoute la réponse assistant à l'échange (borné, tête system gardée).
    pub fn push_assistant(&mut self, text: String) {
        self.history.push((String::from("assistant"), text));
        trim_agent_history(&mut self.history);
    }
}

/// Longueur max de l'extrait de prompt d'un sous-agent.
const AGENT_PROMPT_LEN: usize = 60;
/// Échanges max gardés par agent (au-delà, les plus vieux sortent).
const AGENT_HISTORY_MAX: usize = 20;

/// Borne l'historique agent : sort les plus vieux, garde la tête `system`.
fn trim_agent_history(history: &mut Vec<(String, String)>) {
    while history.len() > AGENT_HISTORY_MAX {
        let pinned = history
            .first()
            .map(|(role, _)| role == "system")
            .unwrap_or(false);
        history.remove(usize::from(pinned));
    }
}

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

    /// Chat par index, lecture (`None` si hors bornes).
    pub fn get(&self, id: usize) -> Option<&ChatSession> {
        self.chats.get(id)
    }

    /// Crée un sous-agent `Running` sur le chat `idx`, rend son id unique.
    /// `None` si hors bornes (l'id n'est pas consommé).
    pub fn add_agent(
        &mut self,
        idx: usize,
        prompt: &str,
        history: Vec<(String, String)>,
    ) -> Option<usize> {
        let id = self.next_agent_id;
        let chat = self.chats.get_mut(idx)?;
        chat.agents.push(SubAgent::new(id, prompt, history));
        self.next_agent_id += 1;
        Some(id)
    }

    /// Retrouve un agent par son id global : `(chat idx, agent)`.
    /// `None` si id inconnu.
    pub fn find_agent_mut(&mut self, agent_id: usize) -> Option<(usize, &mut SubAgent)> {
        self.chats.iter_mut().enumerate().find_map(|(i, c)| {
            c.agents
                .iter_mut()
                .find(|a| a.id == agent_id)
                .map(|a| (i, a))
        })
    }

    /// Prépare une suite de conversation `/ask` : pousse le texte user,
    /// passe l'agent en cours, rend `(chat propriétaire, échange complet)`.
    pub fn ask_agent(
        &mut self,
        agent_id: usize,
        text: String,
    ) -> Result<(usize, Vec<(String, String)>), AskError> {
        let Some((owner, agent)) = self.find_agent_mut(agent_id) else {
            return Err(AskError::Unknown);
        };
        if agent.status == AgentStatus::Running {
            return Err(AskError::Busy);
        }
        agent.push_user(text);
        agent.status = AgentStatus::Running;
        Ok((owner, agent.history.clone()))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Échange minimal : user seul.
    fn user(text: &str) -> Vec<(String, String)> {
        vec![(String::from("user"), text.to_string())]
    }

    #[test]
    fn add_agent_ids_uniques_et_historique_stocke() {
        let mut s = Sessions::new();
        s.add();
        let a = s.add_agent(0, "prompt un", user("u1")).unwrap();
        let b = s.add_agent(1, "prompt deux", user("u2")).unwrap();
        assert_eq!((a, b), (1, 2));
        assert_eq!(s.get(1).unwrap().agents[0].history, user("u2"));
        assert!(s.add_agent(9, "x", user("x")).is_none());
    }

    #[test]
    fn find_agent_mut_retrouve_le_bon_chat() {
        let mut s = Sessions::new();
        s.add();
        s.add_agent(1, "loin", user("u")).unwrap();
        let (idx, agent) = s.find_agent_mut(1).unwrap();
        assert_eq!(idx, 1);
        assert_eq!(agent.prompt, "loin");
        assert!(s.find_agent_mut(42).is_none());
    }

    #[test]
    fn agent_history_bornee_garde_la_tete_system() {
        let mut a = SubAgent::new(1, "p", vec![(String::from("system"), String::from("ctx"))]);
        for i in 0..25 {
            a.push_user(format!("u{i}"));
            a.push_assistant(format!("a{i}"));
        }
        assert_eq!(a.history.len(), AGENT_HISTORY_MAX);
        assert_eq!(a.history[0].0, "system");
        // 51 entrées → 31 plus vieilles sorties : reste [system, a15…a24].
        assert_eq!(
            a.history[1],
            (String::from("assistant"), String::from("a15"))
        );
        assert_eq!(
            a.history[AGENT_HISTORY_MAX - 1],
            (String::from("assistant"), String::from("a24"))
        );
    }

    #[test]
    fn ask_agent_pousse_repasse_en_cours_et_signale() {
        let mut s = Sessions::new();
        s.add();
        s.add_agent(1, "loin", user("u1")).unwrap();
        // Fraîchement créé = Running → occupé.
        assert_eq!(s.ask_agent(1, String::from("x")), Err(AskError::Busy));
        assert_eq!(s.ask_agent(42, String::from("x")), Err(AskError::Unknown));
        s.find_agent_mut(1).unwrap().1.status = AgentStatus::Done;
        let (owner, history) = s.ask_agent(1, String::from("suite")).unwrap();
        assert_eq!(owner, 1);
        assert_eq!(
            history.last(),
            Some(&(String::from("user"), String::from("suite")))
        );
        assert_eq!(s.find_agent_mut(1).unwrap().1.status, AgentStatus::Running);
    }

    #[test]
    fn agent_history_sans_system_sort_par_la_tete() {
        let mut a = SubAgent::new(1, "p", user("u0"));
        for i in 1..30 {
            a.push_user(format!("u{i}"));
        }
        assert_eq!(a.history.len(), AGENT_HISTORY_MAX);
        assert_eq!(a.history[0].1, "u10");
    }
}
