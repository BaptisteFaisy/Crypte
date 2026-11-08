//! Couche fichiers verrouillée, interopérable avec le shell v2.
//!
//! Moitié du lot store (l'autre est `verrou.rs`) : ajouts et réécritures
//! sous verrou frère, lectures fail-open. Patrons shell : `locked_append`
//! (`lib-common.sh:52-55`), `ordre_expire_pass` (`lib-ordres.sh:20-38`),
//! `rotate_one` (`bcast:144-168`), `tail -n` (`bcast:131-137`), `head -n1`
//! pour les hb (`bcast:209`), âge `file_age` (`lib-common.sh:80-86`).

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::time::UNIX_EPOCH;

use super::verrou::{avec_verrou, chemin_verrou, ATTENTE_VERROU_SECS};

/// Seuil de péremption d'un heartbeat, en secondes (`bcast:288`).
pub const SEUIL_HB_SECS: u64 = 900;

/// Ajoute des lignes à un journal sous verrou (interop `locked_append`).
///
/// Verrouille `"<cible>.lock"` pendant l'écriture (`lib-common.sh:52-55`) :
/// un `bcast` shell concurrent attend ou est attendu, jamais d'entrelacs.
/// Chaque ligne est terminée par `\n`, comme `printf '%s\n'`.
/// Échec (verrou, écriture) = erreur, jamais silencieux (`bcast:85`).
pub fn append_verrouille<S: AsRef<str>>(cible: &Path, lignes: &[S]) -> io::Result<()> {
    let verrou = chemin_verrou(cible);
    avec_verrou(&verrou, ATTENTE_VERROU_SECS, || {
        let mut f = OpenOptions::new().create(true).append(true).open(cible)?;
        for ligne in lignes {
            writeln!(f, "{}", ligne.as_ref())?;
        }
        f.flush()?;
        Ok(())
    })
}

/// Réécrit un fichier de façon atomique (`tmp` + `rename` sous verrou).
///
/// Patrons shell : `ordre_expire_pass` (`lib-ordres.sh:20-38`), `rotate_one`
/// (`bcast:144-168`). Le temporaire vit dans le même dossier (même
/// système de fichiers, donc `rename` atomique) et est nettoyé en cas
/// d'erreur. Le choix « réécrire seulement si changé » appartient à
/// l'appelant (pas de churn en lecture, `lib-ordres.sh:19`).
pub fn reecrire_atomique(cible: &Path, contenu: &str) -> io::Result<()> {
    let verrou = chemin_verrou(cible);
    avec_verrou(&verrou, ATTENTE_VERROU_SECS, || {
        let tmp = cible.with_extension(format!("tmp.{}", std::process::id()));
        let ecriture = (|| -> io::Result<()> {
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp)?;
            f.write_all(contenu.as_bytes())?;
            f.flush()?;
            Ok(())
        })();
        match ecriture {
            Ok(()) => fs::rename(&tmp, cible),
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                Err(e)
            }
        }
    })
}

/// Lit toutes les lignes d'un fichier (fail-open : absent = vide).
///
/// Ne verrouille pas : les lecteurs shell (`tail`, `grep`, `bcast:131-137`)
/// lisent sans verrou, une écriture concurrente ne casse qu'au pire la
/// dernière ligne lue.
pub fn lire_lignes(cible: &Path) -> Vec<String> {
    fs::read_to_string(cible)
        .map(|contenu| contenu.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default()
}

/// Lit les `n` dernières lignes d'un fichier (interop `tail -n`).
///
/// `n = 0` = vide ; `n` au-delà de la taille = tout le fichier.
pub fn tail_n(cible: &Path, n: usize) -> Vec<String> {
    let lignes = lire_lignes(cible);
    if n == 0 || lignes.is_empty() {
        return Vec::new();
    }
    let debut = lignes.len().saturating_sub(n);
    lignes[debut..].to_vec()
}

/// Lit un heartbeat : première ligne du fichier (interop `head -n1`).
///
/// `chemin` est le fichier `.hb/<sanitise>` complet, composé par
/// l'appelant (`BcastDir::hb()` + `sanitize()` du socle).
/// Absent, illisible ou vide = `None`.
pub fn lire_hb(chemin: &Path) -> Option<String> {
    let contenu = fs::read_to_string(chemin).ok()?;
    let premiere = contenu.lines().next()?;
    if premiere.is_empty() {
        return None;
    }
    Some(premiere.to_string())
}

/// Âge d'un fichier en secondes (`file_age`, `lib-common.sh:80-86`).
///
/// `-1` si illisible ou horodatage inutilisable, jamais de panique.
/// Un mtime dans le futur donne 0 (pas d'âge négatif hors `-1`).
fn age_fichier(chemin: &Path, maintenant_epoch: u64) -> i64 {
    let meta = match fs::metadata(chemin) {
        Ok(m) => m,
        Err(_) => return -1,
    };
    let modifie = match meta.modified() {
        Ok(t) => t,
        Err(_) => return -1,
    };
    let epoch = match modifie.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => return -1,
    };
    maintenant_epoch.saturating_sub(epoch) as i64
}

/// Dit si un heartbeat est périmé : âge (mtime) strictement > 900 s.
///
/// Interop `cmd_doctor` (`bcast:283-289`) : `file_age` puis comparaison
/// `-gt 900`. Fichier illisible = pas périmé (`-1 -gt 900` est faux).
pub fn hb_perime(chemin: &Path, maintenant_epoch: u64) -> bool {
    age_fichier(chemin, maintenant_epoch) > SEUIL_HB_SECS as i64
}

/// Époque de modification d'un fichier, `None` si illisible.
pub fn epoque_fichier(chemin: &Path) -> Option<u64> {
    let meta = fs::metadata(chemin).ok()?;
    meta.modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Compteur de dossiers de test uniques (tests en parallèle).
    static COMPTEUR: AtomicU64 = AtomicU64::new(0);

    /// Crée un dossier de test unique sous le temporaire système.
    fn dossier_test() -> PathBuf {
        let n = COMPTEUR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "crypte-mbox-fichier-test-{}-{}",
            std::process::id(),
            n
        ));
        fs::create_dir_all(&dir).expect("dossier de test");
        dir
    }

    #[test]
    fn append_cree_puis_concatene() {
        let dir = dossier_test();
        let cible = dir.join("journal.log");
        append_verrouille(&cible, &["ligne un"]).expect("append 1");
        append_verrouille(&cible, &["ligne deux", "ligne trois"]).expect("append 2");
        assert_eq!(
            lire_lignes(&cible),
            vec!["ligne un", "ligne deux", "ligne trois"]
        );
        // Le verrou frère existe, jamais un verrou sur le log lui-même.
        assert!(dir.join("journal.log.lock").is_file());
    }

    #[test]
    fn append_concurrent_serialise_sans_entrelacs() {
        use std::thread;
        let dir = dossier_test();
        let cible = dir.join("concurrent.log");
        let mut bras = Vec::new();
        for t in 0..4 {
            let c = cible.clone();
            bras.push(thread::spawn(move || {
                for i in 0..25 {
                    let ligne = format!("t{t}-l{i:02}");
                    append_verrouille(&c, &[ligne]).expect("append");
                }
            }));
        }
        for b in bras {
            b.join().expect("thread");
        }
        let lignes = lire_lignes(&cible);
        assert_eq!(lignes.len(), 100);
        for l in &lignes {
            assert!(l.starts_with('t'), "ligne entière : {l}");
        }
    }

    #[test]
    fn reecrire_remplace_et_nettoie() {
        let dir = dossier_test();
        let cible = dir.join("etat.txt");
        append_verrouille(&cible, &["ancien"]).expect("seed");
        reecrire_atomique(&cible, "neuf\n").expect("rewrite");
        assert_eq!(lire_lignes(&cible), vec!["neuf"]);
        // Aucun temporaire résiduel.
        let restes: Vec<_> = fs::read_dir(&dir)
            .expect("list")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .collect();
        assert!(!restes.iter().any(|n| n.to_string_lossy().contains("tmp.")));
    }

    #[test]
    fn tail_borne_correctement() {
        let dir = dossier_test();
        let cible = dir.join("gros.log");
        let lignes: Vec<String> = (1..=5).map(|i| format!("l{i}")).collect();
        append_verrouille(&cible, &lignes).expect("seed");
        assert_eq!(tail_n(&cible, 2), vec!["l4", "l5"]);
        assert_eq!(tail_n(&cible, 99).len(), 5);
        assert!(tail_n(&cible, 0).is_empty());
        assert!(tail_n(&dir.join("absent.log"), 3).is_empty());
    }

    #[test]
    fn lectures_absentes_sont_vides() {
        let dir = dossier_test();
        let absent = dir.join("rien.log");
        assert!(lire_lignes(&absent).is_empty());
        assert!(lire_hb(&absent).is_none());
        assert!(!hb_perime(&absent, crate::mbox::epoque()));
    }

    #[test]
    fn hb_lit_premiere_ligne_et_perime_apres_900s() {
        let dir = dossier_test();
        let hb = dir.join("sonde-abcdef12");
        fs::write(&hb, "1794147587 2026-11-08 15:19 vu mode=failover\n").expect("hb");
        let ligne = lire_hb(&hb).expect("lisible");
        assert!(
            ligne.starts_with("1794147587 "),
            "format bcast:216 : {ligne}"
        );
        let mtime = fs::metadata(&hb)
            .expect("meta")
            .modified()
            .expect("mtime")
            .duration_since(UNIX_EPOCH)
            .expect("epoch")
            .as_secs();
        assert!(!hb_perime(&hb, mtime + SEUIL_HB_SECS));
        assert!(!hb_perime(&hb, mtime));
        assert!(hb_perime(&hb, mtime + SEUIL_HB_SECS + 1));
    }
}
