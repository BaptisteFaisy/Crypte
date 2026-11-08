//! Utilitaires partagés des commandes CLI (extrait de `cli.rs`).
//!
//! `cli.rs` dépassait 500 lignes après formatage : le découpage d'options
//! (`decoupage`, `opt`), l'identité (`expediteur`) et la porte à jeton
//! (`porte_jeton`, `usg`) vivent ici, utilisés par `cli`, `cli_gestion`
//! et `jeton`.

use std::path::Path;

use super::autorite::{dossier_defaut, porte_ecriture};
use super::format::{normalise_emetteur, Sortie};

/// Erreur d'usage : message sur stderr + code 2 (`die`, bcast:19).
pub(crate) fn usg(message: &str) -> Sortie {
    eprintln!("bcast: {message}");
    Sortie::Usage
}

/// Découpe options et messages : `specs` = `(nom sans tirets, prend_valeur)`.
/// `--` ou premier positionnel : tout le reste est message (`*) msg=("$@")`).
pub(crate) fn decoupage(
    args: &[String],
    specs: &[(&str, bool)],
) -> Result<(Vec<(String, String)>, Vec<String>), Sortie> {
    let (mut opts, mut msg) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < args.len() {
        let mot = args[i].as_str();
        if mot == "--" {
            msg.extend(args[i + 1..].iter().cloned());
            break;
        }
        let nom = mot
            .strip_prefix("--")
            .or_else(|| mot.strip_prefix('-').filter(|_| mot.len() == 2));
        let Some(nom) = nom.filter(|_| mot.starts_with('-')) else {
            msg.extend(args[i..].iter().cloned());
            break;
        };
        let Some((_, veut_valeur)) = specs.iter().find(|(n, _)| *n == nom) else {
            return Err(usg(&format!("option inconnue : {mot}")));
        };
        if *veut_valeur {
            let Some(val) = args.get(i + 1) else {
                return Err(usg(&format!("option {mot} attend une valeur")));
            };
            opts.push((nom.to_string(), val.clone()));
            i += 2;
        } else {
            opts.push((nom.to_string(), String::new()));
            i += 1;
        }
    }
    Ok((opts, msg))
}

/// Dernière valeur d'une option répétable (`--dest`).
pub(crate) fn opt<'a>(opts: &'a [(String, String)], nom: &str) -> Option<&'a str> {
    opts.iter()
        .rev()
        .find(|(n, _)| n == nom)
        .map(|(_, v)| v.as_str())
}

/// Identité d'expéditeur : `--from` sinon `BCAST_SENDER` (`need_from`, bcast:21).
pub(crate) fn expediteur(brut: Option<&str>) -> Option<String> {
    let candidat = brut
        .map(str::to_string)
        .or_else(|| std::env::var("BCAST_SENDER").ok())?;
    match normalise_emetteur(&candidat) {
        Ok(nom) => Some(nom),
        Err(e) => {
            eprintln!("bcast: {e:#}");
            None
        }
    }
}

/// Porte des écritures sensibles : `--token "nom:secret"` (découpe au
/// dernier `:`) vérifié par l'autorité + garde anti-secret sur le texte.
/// Tout échec refuse (1), sans jamais montrer le jeton.
pub(crate) fn porte_jeton(texte: &str, brut: Option<&str>) -> bool {
    let Some(brut) = brut else {
        eprintln!("bcast: jeton requis (--token \"nom:secret\")");
        return false;
    };
    match dossier_defaut() {
        Ok(d) => porte_dossier(&d, texte, brut),
        Err(_) => {
            eprintln!("bcast: autorité des jetons indisponible");
            false
        }
    }
}

/// Cœur testable de la porte : dossier injectable, autorité réelle.
fn porte_dossier(dossier: &Path, texte: &str, brut: &str) -> bool {
    let (nom, secret) = match brut.rsplit_once(':') {
        Some((n, s)) if !n.is_empty() && !s.is_empty() => (n, s),
        _ => {
            eprintln!("bcast: jeton invalide (forme \"nom:secret\" attendue)");
            return false;
        }
    };
    match porte_ecriture(dossier, nom, secret, texte) {
        Ok(_) => true,
        Err(e) => {
            eprintln!("bcast: {e:#}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::autorite::emettre;
    use super::*;
    use std::path::PathBuf;

    /// Dossier jetable unique (jamais le vrai dossier des réglages).
    fn bac(nom: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mbox-outils-{}-{nom}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        dir
    }

    #[test]
    fn porte_jeton() {
        let racine = bac("porte");
        let jetons = racine.join("jetons");
        std::fs::create_dir_all(&jetons).expect("dossier jetons");
        let emis = emettre(&jetons, "cli-t", None).expect("émission");
        let brut = format!("{}:{}", emis.nom, emis.secret);
        assert!(porte_dossier(&jetons, "texte ordre", &brut));
        assert!(!porte_dossier(&jetons, "texte ordre", "cli-t:mauvais"));
        assert!(!porte_dossier(&jetons, "texte ordre", "sans-separateur"));
        assert!(!porte_dossier(&jetons, "tiens sk-fuite", &brut)); // garde anti-secret
        let _ = std::fs::remove_dir_all(&racine);
    }
}
