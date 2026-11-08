//! MCP des powers : ajout idempotent au parc, survie au refresh.
//!
//! Quand les mots-clés d'un power matchent un envoi, ses serveurs
//! `mcp.json` rejoignent le parc pour la session (connexion paresseuse
//! au premier besoin outillé, comme la config). L'ajout est synchrone
//! (`try_lock`, jamais bloquant) : `Busy` = réessayé au prochain envoi
//! matchant. La config prime : un serveur du même nom est ignoré.

use std::collections::HashSet;

use super::{McpPool, McpState, Tracked};
use crate::kiro::powers::Power;

/// Serveurs ajoutés (frais) et ignorés (doublon de la config).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PowerAdded {
    pub fresh: Vec<String>,
    pub dupes: Vec<String>,
}

/// Issue d'une activation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PowerAdd {
    Added(PowerAdded),
    /// Power déjà activé (silence, rien à annoncer).
    Active,
    /// Verrou pris (réessayé au prochain envoi matchant).
    Busy,
}

impl McpState {
    /// Ajoute le MCP du power (idempotent, synchrone, jamais bloquant).
    pub(crate) fn try_add_power(&self, power: &Power) -> PowerAdd {
        let Ok(mut pool) = self.inner.try_lock() else {
            return PowerAdd::Busy;
        };
        if pool.powers.contains(&power.name) {
            return PowerAdd::Active;
        }
        PowerAdd::Added(pool.add_power(power))
    }
}

impl McpPool {
    /// Insère les serveurs du power (noms pris = doublons, tri final).
    fn add_power(&mut self, power: &Power) -> PowerAdded {
        self.powers.insert(power.name.clone());
        let mut added = PowerAdded::default();
        for (name, server) in &power.mcp_servers {
            if self.servers.iter().any(|tracked| &tracked.name == name) {
                added.dupes.push(name.clone());
            } else {
                added.fresh.push(name.clone());
                self.servers.push(Tracked {
                    name: name.clone(),
                    server: server.clone(),
                    client: None,
                    tools: Vec::new(),
                    error: None,
                    origin: Some(power.name.clone()),
                });
            }
        }
        self.servers.sort_by(|a, b| a.name.cmp(&b.name));
        added
    }

    /// État power extrait avant reconstruction (refresh, après `shutdown`
    /// : clients rendus, prêts à reconnecter).
    pub(crate) fn take_power_state(&mut self) -> (HashSet<String>, Vec<Tracked>) {
        let powers = std::mem::take(&mut self.powers);
        let servers = std::mem::take(&mut self.servers);
        let (kept, _): (Vec<Tracked>, Vec<Tracked>) = servers
            .into_iter()
            .partition(|tracked| tracked.origin.is_some());
        (powers, kept)
    }

    /// État power réinjecté après reconstruction (la config fraîche
    /// prime : un serveur power doublon est écarté, le power restant
    /// marqué actif — même nom = probablement même outil).
    pub(crate) fn restore_power_state(&mut self, kept: (HashSet<String>, Vec<Tracked>)) {
        let (powers, servers) = kept;
        self.powers = powers;
        for tracked in servers {
            if self.servers.iter().any(|known| known.name == tracked.name) {
                continue;
            }
            self.servers.push(tracked);
        }
        self.servers.sort_by(|a, b| a.name.cmp(&b.name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kiro::mcp::{Config, Server};
    use std::collections::HashMap;

    /// Power jouet (serveurs nommés, stdio factice).
    fn power(name: &str, servers: &[&str]) -> Power {
        Power {
            name: name.to_string(),
            version: String::new(),
            description: String::new(),
            keywords: Vec::new(),
            skills: Vec::new(),
            mcp_servers: servers
                .iter()
                .map(|server| {
                    (
                        server.to_string(),
                        Server {
                            command: Some(String::from("kh-nope-binaire")),
                            ..Default::default()
                        },
                    )
                })
                .collect(),
            has_mcp: true,
            legacy: false,
        }
    }

    /// Config jouet (un serveur `cfg`, stdio factice).
    fn cfg_one() -> Config {
        Config {
            mcp_servers: HashMap::from([(
                String::from("cfg"),
                Server {
                    command: Some(String::from("kh-nope-binaire")),
                    ..Default::default()
                },
            )]),
        }
    }

    #[test]
    fn ajout_frais_doublons_idempotent() {
        let state = McpState::for_config(cfg_one());
        let added = state.try_add_power(&power("db", &["pg", "cfg"]));
        assert_eq!(
            added,
            PowerAdd::Added(PowerAdded {
                fresh: vec![String::from("pg")],
                dupes: vec![String::from("cfg")],
            })
        );
        assert_eq!(state.try_add_power(&power("db", &["pg"])), PowerAdd::Active);
        let status = state.status();
        assert!(status.contains("2 servers"), "{status}");
        assert!(
            status.contains("pg (power db, stdio, not connected)"),
            "{status}"
        );
        assert!(status.contains("cfg (stdio, not connected)"), "{status}");
    }

    #[tokio::test]
    async fn refresh_garde_powers_et_reconnecte() {
        let state = McpState::for_config(cfg_one());
        assert!(matches!(
            state.try_add_power(&power("db", &["pg"])),
            PowerAdd::Added(_)
        ));
        let text = state.refresh_with(Config::default()).await;
        assert!(text.contains("1 server"), "{text}");
        assert!(text.contains("pg (power db, stdio, error: "), "{text}");
        assert!(state.status().contains("power db"));
    }
}
