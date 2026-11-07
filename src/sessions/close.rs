//! Fermeture des chats (clic ×, Ctrl+W).
//!
//! `Sessions::close` retire un chat en gardant l'invariant (jamais vide,
//! index valide) ; refusé si dernier, inconnu, ou si une réponse indexée
//! est en vol sur lui ou après lui (les index glisseraient vers le
//! mauvais chat). `shift_bus_links` re-pointe les liens auto du bus.

use crate::bus::{Bus, LinkEnd};

use super::subagents::AgentStatus;
use super::Sessions;

/// Sort d'une fermeture : fermé, ou motif du refus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOutcome {
    Closed,
    LastChat,
    Busy,
    Unknown,
}

impl Sessions {
    /// Ferme le chat `idx` : refusé si dernier (`LastChat`), inconnu
    /// (`Unknown`), ou si une réponse est en vol sur lui ou après lui,
    /// ou si un agent tourne (`Busy`). Les livraisons d'asks en file
    /// suivent le décalage (jetées si elles visaient le fermé).
    pub fn close(&mut self, idx: usize) -> CloseOutcome {
        if self.get(idx).is_none() {
            return CloseOutcome::Unknown;
        }
        if self.count() == 1 {
            return CloseOutcome::LastChat;
        }
        if self.flight_at_or_after(idx) || self.any_agent_running() {
            return CloseOutcome::Busy;
        }
        self.remove_chat(idx);
        CloseOutcome::Closed
    }

    /// Vrai si un envoi (`sending`) ou une réponse annexe (`waiting` :
    /// `/compact`, `/web`) est en vol sur `idx` ou après.
    fn flight_at_or_after(&self, idx: usize) -> bool {
        (idx..self.count()).any(|i| self.get(i).is_some_and(|c| c.sending || c.waiting))
    }

    /// Vrai si un sous-agent tourne (livraison inconnue : prudence globale).
    fn any_agent_running(&self) -> bool {
        self.iter()
            .any(|c| c.agents.iter().any(|a| a.status == AgentStatus::Running))
    }

    /// Retire le chat `idx` (validé), recale les livraisons en file et
    /// le courant (voisin suivant, précédent si c'était le dernier).
    fn remove_chat(&mut self, idx: usize) {
        self.chats.remove(idx);
        for chat in &mut self.chats {
            for agent in &mut chat.agents {
                agent.inbox.retain(|ask| ask.deliver != idx);
                for ask in &mut agent.inbox {
                    if ask.deliver > idx {
                        ask.deliver -= 1;
                    }
                }
            }
        }
        if self.current > idx {
            self.current -= 1;
        } else if self.current >= self.chats.len() {
            self.current = self.chats.len() - 1;
        }
    }
}

/// Re-pointe les liens du bus après fermeture du chat `closed` : liens
/// touchant le fermé supprimés, index au-delà décalés (même glissement
/// que les réponses async, voir [`Sessions::close`]).
pub fn shift_bus_links(bus: &mut Bus, closed: usize) {
    for link in bus.links().to_vec() {
        match (shift_end(link.from, closed), shift_end(link.to, closed)) {
            (Some(from), Some(to)) => {
                if (from, to) != (link.from, link.to) {
                    bus.unlink(link.from, link.to);
                    let _ = bus.link(from, to);
                }
            }
            _ => {
                bus.unlink(link.from, link.to);
            }
        }
    }
}

/// Décale une extrémité de lien (`None` si elle visait le chat fermé).
fn shift_end(end: LinkEnd, closed: usize) -> Option<LinkEnd> {
    match end {
        LinkEnd::Chat(i) if i == closed => None,
        LinkEnd::Chat(i) if i > closed => Some(LinkEnd::Chat(i - 1)),
        other => Some(other),
    }
}

#[cfg(test)]
mod tests {
    use super::super::subagents::user;
    use super::*;
    use crate::bus::{Bus, Link, LinkEnd, QueuedAsk};
    use crate::sessions::{AgentStatus, Sessions};

    /// Trois chats nommés, courant au milieu (index 1).
    fn three() -> Sessions {
        let mut s = Sessions::new();
        s.add();
        s.add();
        s.select(1);
        s
    }

    #[test]
    fn close_milieu_garde_le_courant_sur_le_suivant() {
        let mut s = three();
        assert_eq!(s.close(1), CloseOutcome::Closed);
        assert_eq!(s.count(), 2);
        assert_eq!(s.index(), 1);
        assert_eq!(s.current().name, "Chat 3");
    }

    #[test]
    fn close_avant_le_courant_recule_le_courant() {
        let mut s = three();
        assert_eq!(s.close(0), CloseOutcome::Closed);
        assert_eq!(s.index(), 0);
        assert_eq!(s.current().name, "Chat 2");
    }

    #[test]
    fn close_courant_dernier_replie_sur_le_precedent() {
        let mut s = three();
        s.select(2);
        assert_eq!(s.close(2), CloseOutcome::Closed);
        assert_eq!(s.index(), 1);
        assert_eq!(s.current().name, "Chat 2");
    }

    #[test]
    fn close_refuse_le_dernier_et_l_inconnu() {
        let mut s = Sessions::new();
        assert_eq!(s.close(0), CloseOutcome::LastChat);
        assert_eq!(s.count(), 1);
        let mut s = three();
        assert_eq!(s.close(9), CloseOutcome::Unknown);
        assert_eq!(s.count(), 3);
    }

    #[test]
    fn close_refuse_le_vol_ici_ou_apres_pas_avant() {
        let mut s = three();
        s.get_mut(1).unwrap().sending = true;
        assert_eq!(s.close(0), CloseOutcome::Busy); // glisserait la réponse
        assert_eq!(s.count(), 3);
        assert_eq!(s.close(2), CloseOutcome::Closed); // avant : sans effet
        s.get_mut(1).unwrap().sending = false;
        s.get_mut(0).unwrap().waiting = true; // /compact ou /web en vol
        assert_eq!(s.close(0), CloseOutcome::Busy);
        assert_eq!(s.close(1), CloseOutcome::Closed);
    }

    #[test]
    fn close_refuse_tant_qu_un_agent_tourne() {
        let mut s = three();
        s.add_agent(0, "loin", user("u")).unwrap(); // Running
        assert_eq!(s.close(2), CloseOutcome::Busy); // livraison inconnue
        s.find_agent_mut(1).unwrap().1.status = AgentStatus::Done;
        assert_eq!(s.close(2), CloseOutcome::Closed);
    }

    #[test]
    fn close_recale_les_livraisons_d_asks_en_file() {
        let mut s = three();
        let id = s.add_agent(0, "loin", user("u")).unwrap();
        s.find_agent_mut(id).unwrap().1.status = AgentStatus::Done; // sinon Busy
        let (_, agent) = s.find_agent_mut(id).unwrap();
        agent.inbox.push_back(QueuedAsk {
            deliver: 2,
            text: String::from("a"),
            hops: 0,
        });
        agent.inbox.push_back(QueuedAsk {
            deliver: 1,
            text: String::from("b"),
            hops: 0,
        });
        assert_eq!(s.close(1), CloseOutcome::Closed);
        let (_, agent) = s.find_agent_mut(id).unwrap();
        let delivers: Vec<usize> = agent.inbox.iter().map(|q| q.deliver).collect();
        assert_eq!(delivers, vec![1]); // 2→1, visée du fermé jetée
    }

    #[test]
    fn shift_bus_links_jette_le_ferme_et_decale_apres() {
        let mut bus = Bus::new();
        bus.link(LinkEnd::Chat(0), LinkEnd::Chat(2)).unwrap();
        bus.link(LinkEnd::Chat(2), LinkEnd::Agent(7)).unwrap();
        bus.link(LinkEnd::Agent(7), LinkEnd::Chat(1)).unwrap();
        shift_bus_links(&mut bus, 1);
        assert_eq!(bus.links().len(), 2);
        assert!(bus.links().contains(&Link {
            from: LinkEnd::Chat(0),
            to: LinkEnd::Chat(1),
        }));
        assert!(bus.links().contains(&Link {
            from: LinkEnd::Chat(1),
            to: LinkEnd::Agent(7),
        }));
    }
}
