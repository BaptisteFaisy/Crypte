//! Chats multiples + sous-agents.
//!
//! `Sessions` garde les chats ouverts et le courant ; le rendu de la
//! colonne gauche vit dans `sidebar` (`Sessions::draw_sidebar`).

mod chat;
mod close;
mod subagents;

pub use chat::ChatSession;
pub use close::{shift_bus_links, CloseOutcome};
pub use subagents::{AgentStatus, AskError, SpecTag, SubAgent};

use std::path::Path;

use crate::tokens::TokenLedger;

/// Chats ouverts + position du courant (vide si le dernier est fermé,
/// `[+]` / Ctrl+N recrée un chat ; `current` exige un chat, `index`
/// est neutre à vide).
/// Accès direct par index (`Vec`), sans table secondaire : quelques chats,
/// `select`/`get` en O(1) sans surcoût mémoire d'index redondant.
pub struct Sessions {
    chats: Vec<ChatSession>,
    current: usize,
    next_agent_id: usize,
    /// Namespace des workspaces agents (`<unix_secs>-<pid>`, un par run).
    session: String,
}

impl Sessions {
    /// Une session avec un seul chat « Chat 1 ».
    pub fn new() -> Self {
        Self {
            chats: vec![ChatSession::new(String::from("Chat 1"))],
            current: 0,
            next_agent_id: 1,
            session: crate::workspace::session_id(),
        }
    }

    /// Chat courant (lecture). Panique si aucun chat (vérifier `count()`).
    pub fn current(&self) -> &ChatSession {
        &self.chats[self.current]
    }

    /// Chat courant (écriture). Panique si aucun chat (vérifier `count()`).
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

    /// Attache le workspace disque de l'agent (`<session>/agent-<id>`,
    /// créé en pur code, zéro appel LLM). Silencieux si pas de base,
    /// id inconnu ou échec disque — l'agent tourne quand même. Base
    /// `None` = pas de workspace (tests).
    pub fn attach_workspace(&mut self, agent_id: usize, base: Option<&Path>) {
        let session = self.session.clone();
        let (Some(dir), Some((_, agent))) = (base, self.find_agent_mut(agent_id)) else {
            return;
        };
        agent.workspace = crate::workspace::create(dir, &session, agent_id).ok();
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

    /// Reconstruit depuis des chats sauvegardés (jamais vide, courant borné).
    pub fn from_chats(chats: Vec<ChatSession>, current: usize) -> Self {
        if chats.is_empty() {
            return Self::new();
        }
        let current = current.min(chats.len() - 1);
        Self {
            chats,
            current,
            next_agent_id: 1,
            session: crate::workspace::session_id(),
        }
    }

    /// Nouveau chat « Chat N » qui devient courant.
    pub fn add(&mut self) {
        let n = self.chats.len() + 1;
        self.chats.push(ChatSession::new(format!("Chat {n}")));
        self.current = self.chats.len() - 1;
    }

    /// Chat précédent (cyclique) — utilisé par les tests (plus de raccourci clavier).
    #[cfg(test)]
    pub fn prev(&mut self) {
        self.current = self.current.checked_sub(1).unwrap_or(self.chats.len() - 1);
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

    /// Registre cumulé = somme des registres des chats, sans copie.
    pub fn total_ledger(&self) -> TokenLedger {
        TokenLedger::total_of_iter(self.chats.iter().map(|c| &c.ledger))
    }

    /// Cumul global : tokens de tous les chats (agents et compacts
    /// inclus, déjà comptés dans chaque registre).
    pub fn grand_total(&self) -> u64 {
        self.iter()
            .map(|c| c.ledger.total())
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
    fn from_chats_borne_et_neuf_si_vide() {
        let chats = vec![
            ChatSession::new(String::from("a")),
            ChatSession::new(String::from("b")),
        ];
        let s = Sessions::from_chats(chats, 9);
        assert_eq!(s.count(), 2);
        assert_eq!(s.index(), 1);
        let fresh = Sessions::from_chats(vec![], 0);
        assert_eq!(fresh.count(), 1);
        assert_eq!(fresh.index(), 0);
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
    fn grand_total_sums_all_chats() {
        let mut s = Sessions::new();
        s.current_mut().ledger.record(10, 20, 1.0);
        s.current_mut().ledger.record(5, 7, 0.0);
        assert_eq!(s.grand_total(), 42);
        s.add();
        s.current_mut().ledger.record(100, 0, 1.0);
        assert_eq!(s.grand_total(), 142);
    }

    #[test]
    fn attach_workspace_pose_le_dossier_ou_rien() {
        let mut s = Sessions::new();
        s.add_agent(0, "loin", user("u")).unwrap();
        // Id inconnu : rien ne se passe.
        s.attach_workspace(42, None);
        // Sans base : pas de workspace, agent intact.
        s.attach_workspace(1, None);
        assert!(s.find_agent_mut(1).unwrap().1.workspace.is_none());
        // Avec base : dossier créé sous la session, chemin stocké.
        let session = s.session.clone();
        let base = std::env::temp_dir().join(format!("kh-ws-{}-attach", std::process::id()));
        s.attach_workspace(1, Some(&base));
        let ws = s.find_agent_mut(1).unwrap().1.workspace.clone().unwrap();
        assert_eq!(ws, base.join(&session).join("agent-1"));
        assert!(ws.is_dir());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn from_chats_restaure_avec_namespace_frais_et_courant_borne() {
        let chats = vec![
            ChatSession::new(String::from("a")),
            ChatSession::new(String::from("b")),
        ];
        let s = Sessions::from_chats(chats, 9);
        assert!(!s.session.is_empty());
        assert_eq!(s.current().name, "b");
        let vide = Sessions::from_chats(Vec::new(), 0);
        assert_eq!(vide.current().name, "Chat 1");
    }
}
