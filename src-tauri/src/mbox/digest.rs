//! Digest des compteurs throttle (extrait du lot throttle par l'assembleur).
//!
//! `throttle.rs` dépassait 500 lignes après formatage : `digest_flush` vit
//! ici, avec le même verrou (`.lock` frère, 10 s) via `fichier` au lieu
//! d'un append manuel — même fichier, même sémantique, zéro duplication.

use std::fs;

use super::fichier::append_verrouille;
use super::format::{construire_ligne, BcastDir, Priorite};
use super::throttle::pending_prend;

/// Émet les digest P2 en attente, un par expéditeur ; rend le nombre émis.
/// Prise atomique : deux flush concurrents ne doublonnent jamais.
pub fn digest_flush(dir: &BcastDir) -> usize {
    let mut emis = 0usize;
    let entrees = match fs::read_dir(dir.joindre(".state")) {
        Ok(entrees) => entrees,
        Err(_) => return 0,
    };
    for entree in entrees.flatten() {
        let chemin = entree.path();
        if chemin.extension().and_then(|e| e.to_str()) != Some("pending") || !chemin.is_file() {
            continue;
        }
        let Some(compteurs) = pending_prend(&chemin) else {
            continue;
        };
        let total = compteurs.doublons + compteurs.quotas + compteurs.memes;
        if total == 0 {
            continue;
        }
        let expediteur = chemin.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        let message = format!(
            "digest {expediteur} : {total} msg groupé(s) (dup={} quota={} état={})",
            compteurs.doublons, compteurs.quotas, compteurs.memes
        );
        let ligne = construire_ligne("bcast", Priorite::P2, "", &message);
        if append_verrouille(&dir.boite(), &[ligne]).is_ok() {
            emis += 1;
        }
    }
    emis
}

#[cfg(test)]
mod tests {
    use super::super::format::sanitize;
    use super::super::throttle::{pending_incremente, MotifCompteur};
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COMPTEUR: AtomicU64 = AtomicU64::new(0);

    /// Dossier broadcast temporaire unique (jamais le vrai `.scratch`).
    fn dir_temp() -> (BcastDir, PathBuf) {
        let numero = COMPTEUR.fetch_add(1, Ordering::SeqCst);
        let racine =
            std::env::temp_dir().join(format!("bcast-digest-{}-{numero}", std::process::id()));
        let _ = fs::create_dir_all(&racine);
        (BcastDir::depuis(racine.clone()), racine)
    }

    #[test]
    fn digest_groupe_puis_raz() {
        let (dir, racine) = dir_temp();
        assert!(pending_incremente(&dir, "agent-g", MotifCompteur::Doublon));
        assert!(pending_incremente(&dir, "agent-g", MotifCompteur::Quota));
        assert!(pending_incremente(&dir, "agent-g", MotifCompteur::MemeEtat));
        assert_eq!(digest_flush(&dir), 1);
        let boite = fs::read_to_string(dir.boite()).expect("boîte écrite");
        let attendu = format!(
            "digest {} : 3 msg groupé(s) (dup=1 quota=1 état=1)",
            sanitize("agent-g")
        );
        assert!(boite.contains(&attendu), "digest absent : {boite}");
        assert_eq!(digest_flush(&dir), 0);
        let _ = fs::remove_dir_all(racine);
    }
}
