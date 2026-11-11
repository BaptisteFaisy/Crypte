//! CLI `jeton` : émission/révocation/liste/purge (écrit par l'assembleur).
//!
//! Extension Rust sans équivalent shell (le shell n'a pas de jetons) : sans
//! elle, `emettre`/`revoquer`/`lister`/`purger` (lot auth) n'auraient aucun
//! appelant réel et `--token` serait inutilisable. L'autorité vit dans le
//! dossier des réglages (`Store::default_dir`), jamais dans le broadcast.
//! Le secret n'est montré qu'une fois, à l'émission, jamais ailleurs.

use std::path::Path;

use super::autorite::{dossier_defaut, emettre, lister, purger, revoquer, Statut};
use super::format::{analyser_ttl, BcastDir, Sortie};
use super::outils::{decoupage, opt, usg};

/// `jeton` : répartit vers new/revoque/list/purge. Le dossier broadcast
/// est ignoré : l'autorité des jetons vit dans les réglages.
pub(crate) fn cmd_jeton(_dir: &BcastDir, args: &[String]) -> Sortie {
    let dossier = match dossier_defaut() {
        Ok(d) => d,
        Err(_) => {
            eprintln!("bcast: autorité des jetons indisponible");
            return Sortie::ErreurExecution;
        }
    };
    match args.first().map(String::as_str) {
        Some("new") => jeton_nouveau_sur(&dossier, &args[1..]),
        Some("revoque") => jeton_revoque_sur(&dossier, &args[1..]),
        Some("list") => jeton_liste_sur(&dossier, &args[1..]),
        Some("purge") => jeton_purge_sur(&dossier),
        _ => usg("jeton : new|revoque|list|purge"),
    }
}

/// `jeton new <nom> [--ttl H]` : émet un jeton (heures, défaut 24,
/// 0 = permanent). Affiche `jeton=<nom> secret=<hex>` une seule fois ;
/// nom déjà actif = déjà-en-cours (code 3, comme le shell).
fn jeton_nouveau_sur(dossier: &Path, args: &[String]) -> Sortie {
    let Ok((opts, reste)) = decoupage(args, &[("ttl", true)]) else {
        return Sortie::Usage;
    };
    let [nom] = reste.as_slice() else {
        return usg("usage : bcast jeton new [--ttl H] <nom>");
    };
    if let Some(brut) = opt(&opts, "ttl") {
        if analyser_ttl(brut).is_err() {
            return usg("ttl invalide (heures, 0=permanent)");
        }
    }
    match emettre(dossier, nom, opt(&opts, "ttl")) {
        Ok(emis) => {
            println!("jeton={} secret={}", emis.nom, emis.secret);
            Sortie::Ok
        }
        Err(e) => {
            let texte = format!("{e:#}");
            eprintln!("bcast: {texte}");
            if texte.contains("existe déjà") {
                Sortie::DejaEnCours
            } else {
                Sortie::ErreurExecution
            }
        }
    }
}

/// `jeton revoque <nom>` : révoque le jeton actif d'un agent.
fn jeton_revoque_sur(dossier: &Path, args: &[String]) -> Sortie {
    let [nom] = args else {
        return usg("usage : bcast jeton revoque <nom>");
    };
    match revoquer(dossier, nom) {
        Ok(()) => {
            println!("revoque={nom}");
            Sortie::Ok
        }
        Err(e) => {
            eprintln!("bcast: {e:#}");
            Sortie::ErreurExecution
        }
    }
}

/// `jeton list [--all]` : fiches sans secret (`nom ttl statut`).
fn jeton_liste_sur(dossier: &Path, args: &[String]) -> Sortie {
    let tout = args.iter().any(|a| a == "--all");
    match lister(dossier, tout) {
        Ok(fiches) => {
            for fiche in fiches {
                let statut = match fiche.statut {
                    Statut::Actif => "actif",
                    Statut::Revoque => "revoque",
                    Statut::Expire => "expire",
                };
                let ttl = if fiche.ttl_h == 0 {
                    "permanent".to_string()
                } else {
                    format!("{}h", fiche.ttl_h)
                };
                println!("{} {ttl} {statut}", fiche.nom);
            }
            Sortie::Ok
        }
        Err(e) => {
            eprintln!("bcast: {e:#}");
            Sortie::ErreurExecution
        }
    }
}

/// `jeton purge` : marque expirés les jetons dépassés, rend le compte.
fn jeton_purge_sur(dossier: &Path) -> Sortie {
    match purger(dossier) {
        Ok(n) => {
            println!("purges={n}");
            Sortie::Ok
        }
        Err(e) => {
            eprintln!("bcast: {e:#}");
            Sortie::ErreurExecution
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Dossier d'autorité jetable (jamais le vrai dossier des réglages).
    fn bac(nom: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mbox-jeton-{}-{nom}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        dir
    }

    /// `argv` pratique.
    fn mots(m: &[&str]) -> Vec<String> {
        m.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn cycle_emission_double_revoque() {
        let dir = bac("cycle");
        assert_eq!(jeton_nouveau_sur(&dir, &mots(&["alice"])), Sortie::Ok);
        assert_eq!(
            jeton_nouveau_sur(&dir, &mots(&["alice"])),
            Sortie::DejaEnCours
        );
        assert_eq!(jeton_nouveau_sur(&dir, &mots(&[])), Sortie::Usage);
        assert_eq!(
            jeton_nouveau_sur(&dir, &mots(&["--ttl", "xx", "bob"])),
            Sortie::Usage
        );
        assert_eq!(
            jeton_nouveau_sur(&dir, &mots(&["zoe", "--ttl", "0"])),
            Sortie::Usage
        );
        assert_eq!(jeton_revoque_sur(&dir, &mots(&["alice"])), Sortie::Ok);
        assert_eq!(
            jeton_revoque_sur(&dir, &mots(&["alice"])),
            Sortie::ErreurExecution
        );
        assert_eq!(jeton_nouveau_sur(&dir, &mots(&["alice"])), Sortie::Ok);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn liste_et_purge() {
        let dir = bac("liste");
        assert_eq!(jeton_liste_sur(&dir, &[]), Sortie::Ok);
        assert_eq!(jeton_purge_sur(&dir), Sortie::Ok);
        assert_eq!(
            jeton_nouveau_sur(&dir, &mots(&["--ttl", "0", "roc"])),
            Sortie::Ok
        );
        assert_eq!(jeton_liste_sur(&dir, &mots(&["--all"])), Sortie::Ok);
        assert_eq!(jeton_purge_sur(&dir), Sortie::Ok);
        std::fs::remove_dir_all(&dir).ok();
    }
}
