//! Bus de messages inter-chats/agents (faible latence).
//!
//! Adressage unifié ([`Target`]), files d'attente ([`QueuedTell`],
//! [`QueuedAsk`] : affichées aussitôt, déclenchées dès que la cible est
//! libre) et liens auto ([`Link`] : la réponse d'une source est
//! retransmise à sa cible, en éventail, avec un plafond de relais
//! [`MAX_AUTO_HOPS`] qui casse les boucles). Zéro délai humain : plus
//! aucun refus « occupé », tout s'enfile et part tout seul.

/// Relais automatiques max d'une chaîne (un lien = 1 relais ; au-delà,
/// la retransmission est comptée `capped` et abandonnée).
pub const MAX_AUTO_HOPS: u8 = 8;

/// Cible d'un message : chat (numéro 1-based, comme `/tell`) ou agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Chat(usize),
    Agent(usize),
}

/// `2`/`@2` → chat 2, `#3` → agent#3 (les agents exigent `#`, les
/// chiffres nus désignent un chat). `None` si pas une cible (`all`,
/// vide, `@x`, `0`…).
pub fn parse_target(token: &str) -> Option<Target> {
    let token = token.trim();
    if let Some(num) = token.strip_prefix('#') {
        return num
            .parse::<usize>()
            .ok()
            .filter(|n| *n >= 1)
            .map(Target::Agent);
    }
    let num = token.strip_prefix('@').unwrap_or(token);
    num.parse::<usize>()
        .ok()
        .filter(|n| *n >= 1)
        .map(Target::Chat)
}

/// Extrémité d'un lien : chat (index 0-based) ou agent (id global).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkEnd {
    Chat(usize),
    Agent(usize),
}

impl LinkEnd {
    /// Depuis un [`Target`] 1-based (chat n → index n-1).
    pub fn from_target(target: Target) -> Self {
        match target {
            Target::Chat(n) => LinkEnd::Chat(n - 1),
            Target::Agent(id) => LinkEnd::Agent(id),
        }
    }

    /// `Chat 2` / `agent#3` (listes, notes).
    pub fn label(self) -> String {
        match self {
            LinkEnd::Chat(idx) => format!("Chat {}", idx + 1),
            LinkEnd::Agent(id) => format!("agent#{id}"),
        }
    }
}

/// Règle auto : chaque réponse de `from` est retransmise à `to`
/// (déclenche sa réponse ; compte 1 relais, voir [`MAX_AUTO_HOPS`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    pub from: LinkEnd,
    pub to: LinkEnd,
}

/// Échec de création d'un lien : vers soi-même, ou déjà présent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    Same,
    Exists,
}

/// Table des liens + compteurs (retransmis, coupés par le plafond).
#[derive(Debug, Default)]
pub struct Bus {
    links: Vec<Link>,
    pub forwarded: u64,
    pub capped: u64,
}

impl Bus {
    /// Bus vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Ajoute un lien (refusé : vers soi-même, doublon).
    pub fn link(&mut self, from: LinkEnd, to: LinkEnd) -> Result<(), LinkError> {
        if from == to {
            return Err(LinkError::Same);
        }
        if self.links.iter().any(|l| l.from == from && l.to == to) {
            return Err(LinkError::Exists);
        }
        self.links.push(Link { from, to });
        Ok(())
    }

    /// Retire un lien (vrai s'il était présent).
    pub fn unlink(&mut self, from: LinkEnd, to: LinkEnd) -> bool {
        let before = self.links.len();
        self.links.retain(|l| l.from != from || l.to != to);
        self.links.len() != before
    }

    /// Cibles liées à `from` (éventail, dans l'ordre d'ajout).
    pub fn targets(&self, from: LinkEnd) -> Vec<LinkEnd> {
        self.links
            .iter()
            .filter(|l| l.from == from)
            .map(|l| l.to)
            .collect()
    }

    /// Tous les liens (affichage `/links`).
    #[allow(dead_code)]
    pub fn links(&self) -> &[Link] {
        &self.links
    }

    /// Texte `/links` : règles + compteurs (toujours les compteurs).
    pub fn list_text(&self) -> String {
        let mut out = if self.links.is_empty() {
            String::from("no links")
        } else {
            self.links
                .iter()
                .map(|l| format!("{} → {}", l.from.label(), l.to.label()))
                .collect::<Vec<_>>()
                .join("\n")
        };
        out.push_str(&format!(
            "\nforwarded: {} · capped (>{} hops): {}",
            self.forwarded, MAX_AUTO_HOPS, self.capped
        ));
        out
    }
}

/// Tell en file (affiché aussitôt dans le chat cible, versé à
/// l'historique + déclenché dès que le chat est libre).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedTell {
    pub envelope: String,
    pub hops: u8,
}

/// Ask en file (déclenché dès que l'agent est libre, livré à `deliver`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedAsk {
    pub deliver: usize,
    pub text: String,
    pub hops: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_forme_chats_agents_et_rejets() {
        assert_eq!(parse_target("2"), Some(Target::Chat(2)));
        assert_eq!(parse_target("@2"), Some(Target::Chat(2)));
        assert_eq!(parse_target("#3"), Some(Target::Agent(3)));
        assert_eq!(parse_target("  #3  "), Some(Target::Agent(3)));
        assert_eq!(parse_target("all"), None);
        assert_eq!(parse_target(""), None);
        assert_eq!(parse_target("@demain"), None);
        assert_eq!(parse_target("0"), None);
        assert_eq!(parse_target("#0"), None);
        assert_eq!(parse_target("#x"), None);
        assert_eq!(LinkEnd::from_target(Target::Chat(2)), LinkEnd::Chat(1));
        assert_eq!(LinkEnd::Chat(1).label(), "Chat 2");
        assert_eq!(LinkEnd::Agent(3).label(), "agent#3");
    }

    #[test]
    fn link_refuse_soi_meme_et_doublon_unlink_retire() {
        let mut bus = Bus::new();
        let a = LinkEnd::Agent(1);
        let c = LinkEnd::Chat(1);
        assert_eq!(bus.link(a, a), Err(LinkError::Same));
        assert!(bus.link(a, c).is_ok());
        assert_eq!(bus.link(a, c), Err(LinkError::Exists));
        assert!(bus.unlink(a, c));
        assert!(!bus.unlink(a, c));
        assert!(bus.links().is_empty());
    }

    #[test]
    fn targets_eventail_dans_l_ordre_d_ajout() {
        let mut bus = Bus::new();
        let src = LinkEnd::Chat(0);
        bus.link(src, LinkEnd::Agent(1)).unwrap();
        bus.link(src, LinkEnd::Chat(2)).unwrap();
        bus.link(LinkEnd::Agent(9), src).unwrap();
        assert_eq!(bus.targets(src), vec![LinkEnd::Agent(1), LinkEnd::Chat(2)]);
    }

    #[test]
    fn list_text_montre_regles_et_compteurs() {
        let mut bus = Bus::new();
        assert!(bus.list_text().starts_with("no links\nforwarded: 0"));
        bus.link(LinkEnd::Agent(1), LinkEnd::Chat(1)).unwrap();
        bus.forwarded = 5;
        bus.capped = 1;
        let text = bus.list_text();
        assert!(text.contains("agent#1 → Chat 2"), "{text}");
        assert!(text.contains("forwarded: 5"), "{text}");
        assert!(text.contains("capped (>8 hops): 1"), "{text}");
    }
}

/// Flux bus de bout en bout (via `App`) : files pompées aux tombées,
/// liens retransmis en éventail, plafond anti-boucles, diffusion.
/// Ici plutôt que dans `app.rs` (limite 500 lignes) : c'est la
/// sémantique du bus qui est vérifiée, pas le TUI.
#[cfg(test)]
mod flow_tests {
    use crate::app::App;
    use crate::provider::{Completion, ProviderConfig};
    use crate::sessions::AgentStatus;

    /// App de test (provider injoignable : les tâches spawnées échouent
    /// vite sur refus de connexion, sans réseau).
    fn test_app() -> App {
        let cfg = ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        App::from_config(cfg).unwrap()
    }

    /// Texte du dernier message affiché du chat `idx`.
    fn last_text(app: &App, idx: usize) -> String {
        app.sessions
            .get(idx)
            .unwrap()
            .chat
            .messages()
            .last()
            .map(|m| m.text.clone())
            .unwrap_or_default()
    }

    /// Réponse provider factice (tokens 1/1).
    fn done(text: &str) -> Completion {
        Completion {
            text: text.to_string(),
            prompt_tokens: 1,
            completion_tokens: 1,
        }
    }

    #[test]
    fn ask_enfile_si_agent_occupe() {
        let mut app = test_app();
        let history = vec![(String::from("user"), String::from("u"))];
        let id = app.sessions.add_agent(0, "prompt", history).unwrap();
        app.run_command("/ask #1 suite");
        assert_eq!(
            last_text(&app, 0),
            format!("agent#{id} busy — queued (position 1)")
        );
        let (_, agent) = app.sessions.find_agent_mut(id).unwrap();
        assert_eq!(agent.inbox.len(), 1);
        assert_eq!(agent.inbox[0].deliver, 0);
        assert_eq!(agent.inbox[0].text, "suite");
    }

    #[tokio::test]
    async fn drain_chat_pompe_la_file_en_groupe() {
        let mut app = test_app();
        app.add_chat(); // courant = Chat 2
        app.sessions.prev(); // courant = Chat 1
        app.sessions.get_mut(1).unwrap().sending = true;
        app.run_command("/tell 2 premier");
        app.run_command("/tell 2 second");
        assert_eq!(last_text(&app, 0), "Chat 2 busy — queued");
        app.drain_chat(1, Ok(done("ok")));
        let dest = app.sessions.get(1).unwrap();
        assert!(dest.sending); // réponse groupée relancée aussitôt
        assert!(dest.inbox.is_empty());
        let users = dest
            .history
            .iter()
            .filter(|(role, _)| role == "user")
            .count();
        assert_eq!(users, 2);
    }

    #[tokio::test]
    async fn drain_agent_pompe_sa_file() {
        let mut app = test_app();
        let history = vec![(String::from("user"), String::from("u"))];
        let id = app.sessions.add_agent(0, "prompt", history).unwrap();
        app.run_command("/ask #1 relance");
        app.drain_agent(0, id, 0, Ok(done("r1")));
        let (_, agent) = app.sessions.find_agent_mut(id).unwrap();
        assert_eq!(agent.status, AgentStatus::Running); // file pompée aussitôt
        assert!(agent.inbox.is_empty());
        assert_eq!(agent.history.last().unwrap().1, "relance");
    }

    #[tokio::test]
    async fn lien_chat_vers_chat_retransmet_et_declenche() {
        let mut app = test_app();
        app.add_chat();
        app.run_command("/link 1 2");
        assert_eq!(last_text(&app, 1), "linked: Chat 1 → Chat 2");
        app.drain_chat(0, Ok(done("salut")));
        let dest = app.sessions.get(1).unwrap();
        assert!(dest.sending);
        assert_eq!(last_text(&app, 1), "[auto Chat 1] salut");
        assert_eq!(app.bus.forwarded, 1);
    }

    #[tokio::test]
    async fn lien_agent_vers_chat_declenche_le_chat() {
        let mut app = test_app();
        app.add_chat();
        let history = vec![(String::from("user"), String::from("u"))];
        let id = app.sessions.add_agent(0, "prompt", history).unwrap();
        app.sessions.prev(); // courant = Chat 1 (clarté de la note)
        app.run_command("/link #1 2");
        assert_eq!(last_text(&app, 0), "linked: agent#1 → Chat 2");
        app.drain_agent(0, id, 0, Ok(done("trouvaille")));
        let dest = app.sessions.get(1).unwrap();
        assert!(dest.sending);
        assert_eq!(last_text(&app, 1), "[auto agent#1] trouvaille");
    }

    #[tokio::test]
    async fn lien_agent_vers_agent_libre_declenche() {
        let mut app = test_app();
        let history = vec![(String::from("user"), String::from("u"))];
        let a = app
            .sessions
            .add_agent(0, "premier", history.clone())
            .unwrap();
        let b = app.sessions.add_agent(0, "second", history).unwrap();
        app.drain_agent(0, b, 0, Ok(done("pret"))); // B libre pour recevoir
        app.run_command("/link #1 #2");
        assert_eq!(last_text(&app, 0), "linked: agent#1 → agent#2");
        app.drain_agent(0, a, 0, Ok(done("boulot")));
        let (_, agent) = app.sessions.find_agent_mut(b).unwrap();
        assert_eq!(agent.status, AgentStatus::Running);
        assert_eq!(agent.history.last().unwrap().1, "boulot");
        assert_eq!(agent.auto_hops, 1);
        assert_eq!(app.bus.forwarded, 1);
    }

    #[test]
    fn lien_agent_vers_agent_occupe_enfile() {
        let mut app = test_app();
        let history = vec![(String::from("user"), String::from("u"))];
        let a = app
            .sessions
            .add_agent(0, "premier", history.clone())
            .unwrap();
        let b = app.sessions.add_agent(0, "second", history).unwrap();
        app.run_command("/link #1 #2");
        app.drain_agent(0, a, 0, Ok(done("boulot")));
        let (_, agent) = app.sessions.find_agent_mut(b).unwrap();
        assert_eq!(agent.inbox.len(), 1); // B occupé : ask auto enfilé
        assert_eq!(agent.inbox[0].hops, 1);
        assert_eq!(agent.inbox[0].deliver, 0);
        assert_eq!(app.bus.forwarded, 1);
    }

    #[test]
    fn plafond_relais_coupe_les_boucles() {
        let mut app = test_app();
        app.add_chat();
        app.run_command("/link 1 2");
        app.run_command("/link 2 1");
        app.sessions.get_mut(0).unwrap().auto_hops = 8;
        app.drain_chat(0, Ok(done("x")));
        assert_eq!(app.bus.capped, 1);
        assert_eq!(app.bus.forwarded, 0);
        assert!(!app.sessions.get(1).unwrap().sending);
        assert!(last_text(&app, 0).starts_with("link cut"));
    }

    #[tokio::test]
    async fn msg_all_diffuse_aux_autres_chats() {
        let mut app = test_app();
        app.add_chat();
        app.add_chat(); // courant = Chat 3
        app.run_command("/msg all go");
        assert_eq!(
            last_text(&app, 2),
            "broadcast to 2 chats (2 triggered, 0 queued)"
        );
        assert!(app.sessions.get(0).unwrap().sending);
        assert!(app.sessions.get(1).unwrap().sending);
        assert_eq!(last_text(&app, 0), "[from Chat 3] go");
    }

    #[test]
    fn link_valide_ses_extremites() {
        let mut app = test_app();
        app.run_command("/link 1 9");
        assert_eq!(last_text(&app, 0), "chat 9 not found");
        app.run_command("/link #7 1");
        assert_eq!(last_text(&app, 0), "agent#7 unknown");
        app.run_command("/link 1 1");
        assert_eq!(last_text(&app, 0), "a link to itself is useless");
        app.run_command("/unlink 1 1");
        assert_eq!(last_text(&app, 0), "no link in that direction");
        assert!(app.bus.links().is_empty());
    }

    #[test]
    fn agents_all_montre_tous_les_chats() {
        let mut app = test_app();
        app.add_chat();
        let history = vec![(String::from("user"), String::from("u"))];
        app.sessions.add_agent(0, "loin", history.clone()).unwrap();
        app.sessions.add_agent(1, "ici", history).unwrap();
        app.run_command("/agents all");
        let text = last_text(&app, 1);
        assert!(text.contains("Chat 1 #1"), "{text}");
        assert!(text.contains("Chat 2 #2"), "{text}");
        app.run_command("/agents");
        assert!(last_text(&app, 1).starts_with("#2"));
    }
}
