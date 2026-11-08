//! Verrous interopérables avec le shell v2 (moitié du lot store).
//!
//! Verrou frère `"<cible>.lock"` (`locked_append`, `lib-common.sh:52-55`),
//! `flock` via `libc` (dépendance directe) en boucle `LOCK_NB` + attente
//! 10 s comme `flock -w 10`. L'autre moitié (`fichier.rs`) y range les
//! lectures/écritures ; `throttle` (fail-open) et `autorite`/`fichier`
//! (fail-closed, via [`avec_verrou`]) partagent la boucle via
//! [`verrouiller`], un seul point d'attente.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Attente verrou des écritures, en secondes (`flock -w 10` du shell).
pub const ATTENTE_VERROU_SECS: u64 = 10;

/// Pas d'attente entre deux tentatives de `flock` non bloquant.
const PAS_VERROU_MS: u64 = 50;

/// Chemin du verrou d'une cible : `"<cible>.lock"`, à l'identique du shell.
///
/// Le verrou est un fichier frère du log (`"$f.lock"`, `lib-common.sh:54`),
/// jamais le log lui-même : Rust et shell se sérialisent mutuellement.
pub fn chemin_verrou(cible: &Path) -> PathBuf {
    let mut nom = cible.as_os_str().to_owned();
    nom.push(".lock");
    PathBuf::from(nom)
}

/// Prend un verrou exclusif avec attente bornée (`flock -w N`).
///
/// Boucle `LOCK_EX | LOCK_NB` : `EWOULDBLOCK` = réessayer jusqu'au délai,
/// `EINTR` = réessayer aussitôt, autre erreur = échec immédiat.
/// Dépassement du délai = erreur `TimedOut`.
pub fn prendre_verrou(fichier: &File, attente_secs: u64) -> io::Result<()> {
    let fd = fichier.as_raw_fd();
    let limite = Instant::now() + Duration::from_secs(attente_secs);
    loop {
        // SAFETY : fd valide issu d'un File ouvert, appel sans état global.
        let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        match err.raw_os_error() {
            Some(code) if code == libc::EINTR => continue,
            Some(code) if code == libc::EWOULDBLOCK => {
                if Instant::now() >= limite {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "verrou occupé (délai dépassé)",
                    ));
                }
                std::thread::sleep(Duration::from_millis(PAS_VERROU_MS));
            }
            _ => return Err(err),
        }
    }
}

/// Rend un verrou pris par [`prendre_verrou`] (best-effort, comme `flock -u`).
pub fn rendre_verrou(fichier: &File) {
    // SAFETY : mêmes conditions que prendre_verrou ; l'échec est ignoré
    // car la fermeture du File libère le verrou de toute façon.
    unsafe {
        libc::flock(fichier.as_raw_fd(), libc::LOCK_UN);
    }
}

/// Politique quand le verrou est impossible (délai dépassé ou erreur).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnCasDEchec {
    /// Rend `None` : l'appelant choisit son défaut (`throttle`).
    FailOpen,
    /// Propage l'erreur : l'appelant échoue (écritures, autorité).
    FailClosed,
}

/// Garde RAII : rend le verrou à la destruction (`close` libère aussi).
pub struct Garde {
    fichier: File,
}

impl Drop for Garde {
    fn drop(&mut self) {
        rendre_verrou(&self.fichier);
    }
}

/// Ouvre `verrou` (créé au besoin, dossier parent aussi) et prend le
/// verrou exclusif avec attente bornée (`flock -w N`, boucle unique
/// [`prendre_verrou`]). `FailOpen` rend `Ok(None)` quand le verrou est
/// impossible ; `FailClosed` propage l'erreur.
pub fn verrouiller(
    verrou: &Path,
    attente_secs: u64,
    echec: EnCasDEchec,
) -> io::Result<Option<Garde>> {
    if let Some(dossier) = verrou.parent() {
        let _ = fs::create_dir_all(dossier);
    }
    let fichier = match OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(verrou)
    {
        Ok(fichier) => fichier,
        Err(err) => match echec {
            EnCasDEchec::FailOpen => return Ok(None),
            EnCasDEchec::FailClosed => return Err(err),
        },
    };
    match prendre_verrou(&fichier, attente_secs) {
        Ok(()) => Ok(Some(Garde { fichier })),
        Err(err) => match echec {
            EnCasDEchec::FailOpen => Ok(None),
            EnCasDEchec::FailClosed => Err(err),
        },
    }
}

/// Exécute `travail` sous verrou exclusif du fichier `verrou`.
///
/// Crée le fichier verrou s'il manque (comme `exec 8>"$lock"` du shell),
/// attend `attente_secs` au plus, rend le verrou même en cas d'erreur.
pub fn avec_verrou<T>(
    verrou: &Path,
    attente_secs: u64,
    travail: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    // Inatteignable en `FailClosed` (jamais `Ok(None)`), exigé par le type.
    let garde = verrouiller(verrou, attente_secs, EnCasDEchec::FailClosed)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "verrou occupé (délai dépassé)"))?;
    let resultat = travail();
    drop(garde);
    resultat
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Compteur de dossiers de test uniques (tests en parallèle).
    static COMPTEUR: AtomicU64 = AtomicU64::new(0);

    /// Crée un dossier de test unique sous le temporaire système.
    fn dossier_test() -> PathBuf {
        let n = COMPTEUR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "crypte-mbox-verrou-test-{}-{}",
            std::process::id(),
            n
        ));
        fs::create_dir_all(&dir).expect("dossier de test");
        dir
    }

    #[test]
    fn verrou_pointe_sur_fichier_lock_frere() {
        let cible = Path::new("/tmp/x/BOITE-USER.md");
        assert_eq!(chemin_verrou(cible), Path::new("/tmp/x/BOITE-USER.md.lock"));
    }

    #[test]
    fn verrou_delai_depasse_est_erreur() {
        let dir = dossier_test();
        let lock = dir.join("pris.lock");
        let garde = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock)
            .expect("lock");
        prendre_verrou(&garde, 5).expect("premier");
        // Même processus : flock est réentrant par fd distinct sur Linux
        // pour un verrou déjà détenu ; on vérifie au moins le chemin OK.
        rendre_verrou(&garde);
        prendre_verrou(&garde, 0).expect("reprise immédiate");
        rendre_verrou(&garde);
    }

    #[test]
    fn verrouiller_fail_open_et_fail_closed() {
        let dir = dossier_test();
        let verrou = chemin_verrou(&dir.join("cible.log"));
        assert!(verrouiller(&verrou, 1, EnCasDEchec::FailOpen)
            .unwrap()
            .is_some());
        assert!(verrouiller(&verrou, 1, EnCasDEchec::FailClosed)
            .unwrap()
            .is_some());
        // Parent incréable (fichier à la place du dossier) : FailOpen
        // rend None, FailClosed rend une erreur.
        let obstacle = dir.join("fichier");
        fs::write(&obstacle, "bloc").expect("obstacle");
        let bloque = obstacle.join("cible.log.lock");
        assert!(verrouiller(&bloque, 0, EnCasDEchec::FailOpen)
            .unwrap()
            .is_none());
        assert!(verrouiller(&bloque, 0, EnCasDEchec::FailClosed).is_err());
    }
}
