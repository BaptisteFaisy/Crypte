//! Workspace disque des sous-agents (`/agent`).
//!
//! Chaque lancement déclenche la création de `<session>/agent-<id>/`
//! sous `$HOME/.config/kiro-horror-tui/workspaces`, en pur code (`std::fs`,
//! zéro appel LLM, zéro token). Les futurs outils y liront/écriront ;
//! en attendant, le dossier existe et son chemin est stocké sur le
//! [`SubAgent`](crate::sessions::SubAgent).
//!
//! Le workspace est un bonus, jamais bloquant : `HOME` absent ou échec
//! de création → pas de workspace, l'agent démarre quand même.

use std::path::{Path, PathBuf};

/// Dossier parent des workspaces, relatif à `$HOME`.
const WORKSPACES_DIR: &str = ".config/kiro-horror-tui/workspaces";

/// Racine des workspaces (`None` si `HOME` manquant).
pub fn base() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| Path::new(&home).join(WORKSPACES_DIR))
}

/// Id de session unique (`<unix_secs>-<pid>`), stable pour tout le run.
/// Les ids agents repartant à 1 à chaque démarrage, le namespace
/// session évite qu'un agent réutilise le workspace d'un run précédent.
pub fn session_id() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}-{}", std::process::id())
}

/// Chemin du workspace de l'agent (pur, ne touche pas le disque).
pub fn dir_for(base: &Path, session: &str, agent_id: usize) -> PathBuf {
    base.join(session).join(format!("agent-{agent_id}"))
}

/// Crée le workspace de l'agent (idempotent : existe déjà → `Ok`),
/// rend son chemin.
pub fn create(base: &Path, session: &str, agent_id: usize) -> std::io::Result<PathBuf> {
    let dir = dir_for(base, session, agent_id);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kh-ws-{}-{name}", std::process::id()))
    }

    #[test]
    fn dir_for_namespace_session_puis_agent() {
        assert_eq!(
            dir_for(Path::new("/x/workspaces"), "s1", 3),
            Path::new("/x/workspaces/s1/agent-3")
        );
    }

    #[test]
    fn session_id_forme_timestamp_pid() {
        let id = session_id();
        let (secs, pid) = id.rsplit_once('-').unwrap();
        assert!(secs.parse::<u64>().is_ok());
        assert_eq!(pid.parse::<u32>().unwrap(), std::process::id());
    }

    #[test]
    fn create_cree_et_reste_idempotent() {
        let base = tmp_dir("create");
        let dir = create(&base, "s1", 7).unwrap();
        assert_eq!(dir, base.join("s1").join("agent-7"));
        assert!(dir.is_dir());
        create(&base, "s1", 7).unwrap(); // déjà là → Ok quand même
        assert!(dir.is_dir());
        std::fs::remove_dir_all(&base).ok();
    }
}
