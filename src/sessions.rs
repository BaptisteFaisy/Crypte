//! Chats multiples + sous-agents.
//!
//! `Sessions` garde les chats ouverts et le courant ; le rendu de la
//! colonne gauche vit dans `sidebar` (`Sessions::draw_sidebar`).

mod chat;
mod subagents;

pub use chat::ChatSession;
pub use subagents::{AgentStatus, AskError, SubAgent};

use crate::tokens::TokenLedger;

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
    /// `history` = échange seul (système re-préfixé à chaque appel).
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

    /// Vrai si un agent porte cet id global (validation des liens).
    pub fn has_agent(&self, agent_id: usize) -> bool {
        self.chats
            .iter()
            .any(|c| c.agents.iter().any(|a| a.id == agent_id))
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

    /// Rend le chat `idx` courant (ignoré hors bornes).
    pub fn select(&mut self, idx: usize) {
        if idx < self.chats.len() {
            self.current = idx;
        }
    }

    /// Index du chat courant (0-based).
    pub fn index(&self) -> usize {
        self.current
    }

    /// Chats ouverts, dans l'ordre de la sidebar.
    pub fn iter(&self) -> impl Iterator<Item = &ChatSession> {
        self.chats.iter()
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

    /// Cumul global : tokens de tous les chats (agents et compacts
    /// inclus, déjà comptés dans chaque registre).
    pub fn grand_total(&self) -> u64 {
        self.iter()
            .map(|c| c.ledger.total())
            .fold(0, u64::saturating_add)
    }

    /// Cumul global des sous-agents (déjà inclus dans `grand_total`).
    pub fn agents_grand_total(&self) -> u64 {
        self.iter()
            .map(|c| c.agents_total())
            .fold(0, u64::saturating_add)
    }
}

impl Default for Sessions {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::subagents::user;
    use super::*;

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
    fn grand_total_covers_chats_and_agents_once() {
        let mut s = Sessions::new();
        s.current_mut().ledger.record(10, 20, 1.0);
        s.add_agent(0, "prompt test", user("u1")).unwrap();
        {
            let agent = s.find_agent_mut(1).unwrap().1;
            agent.prompt_tokens = 5;
            agent.completion_tokens = 7;
            agent.status = AgentStatus::Done;
        }
        s.current_mut().ledger.record(5, 7, 0.0);
        assert_eq!(s.get(0).unwrap().agents_total(), 12);
        assert_eq!(s.grand_total(), 42);
        assert_eq!(s.agents_grand_total(), 12);
        s.add();
        s.current_mut().ledger.record(100, 0, 1.0);
        assert_eq!(s.grand_total(), 142);
        assert_eq!(s.agents_grand_total(), 12);
    }
}
