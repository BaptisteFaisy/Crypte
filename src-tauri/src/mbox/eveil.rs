//! Éveil inotify du broadcast (lot `eveil`, vague 5) : VITESSE.
//!
//! Remplace le polling à vide par un éveil sur événement : à chaque `tick`
//! (10 Hz), lecture non bloquante (timeout 0) d'un fd inotify posé sur le
//! `BCAST_DIR` ; si une écriture est signalée, `sonde::tour` est appelée
//! (sa logique de relecture/offsets est réutilisée telle quelle, cf.
//! `sonde.rs`) ; sinon rien ne se passe (zéro tour à vide).
//!
//! # Spec de câblage exacte (budgets vérifiés, `feat/mailbox-agents` @92d4ab2)
//!
//! - `src/app.rs` L276 : SWAP 1 ligne, +0 ligne :
//!   `crate::mbox::sonde::tour(&self.events);`
//!   → `crate::mbox::eveil::tour(&self.events);`
//!   (`app.rs` 499L/22fn : budget +0, swap seul autorisé).
//! - `src/mbox/mod.rs` : +1 ligne `pub mod eveil;` entre `docteur` et
//!   `fantome` (ordre alphabétique, 444L/26fn → 445L/26fn).
//! - `src/mbox/sonde.rs` : RIEN (0 ligne, fichier interdit).
//! - Dépendances : `libc 0.2` directe déjà au lock (`Cargo.toml` L49,
//!   `Cargo.lock` 0.2.191) ; crate `notify` INTERDITE (hors lock, offline).
//!
//! # Choix documentés
//!
//! - État : `OnceLock<Mutex<Eveil>>` statique initialisé au 1er appel, même
//!   motif que `sonde::tour` et son `DERNIER` (`sonde.rs` L58) : aucun
//!   champ dans `App` (le swap d'1 ligne est préservé), pas d'état global
//!   mut hors ce verrou (empoisonné → fallback, jamais de panique).
//! - Dossiers surveillés, pas les fichiers : un watch inotify sur un
//!   fichier ne suit pas son remplacement (rotation = nouvel inode) ; les
//!   watches sur racine (inbox/ordres/acks), `bus/` et `.hb/` couvrent les
//!   `CREATE`/`MOVED_TO` des remplaçants.
//! - Pas de thread, pas de sleep : fd `IN_NONBLOCK`, lecture timeout 0.
//! - Le throttle 15 s de `sonde::tour` reste la cadence max (anti-rafale,
//!   `sonde.rs` L44) : l'éveil supprime les tours à vide, il ne change
//!   rien d'autre (zéro régression, fallback = `sonde::tour` throttlée).
//! - Jamais de secret : seuls les masques inotify sont inspectés, le
//!   contenu des fichiers n'est ni lu ni journalisé ici (émissions via
//!   `sonde`, inchangées).
//!
//! Portabilité (Tauri) : inotify est Linux-only. Hors Linux, `ouvrir`
//! rend un veilleur d'emblée hors service et `evenements` rend `None` :
//! `tour` replie sur `sonde::tour` (polling 15 s, comportement d'avant).

#[cfg(target_os = "linux")]
use std::ffi::CString;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "linux")]
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use super::sonde;
use super::BcastDir;
use crate::events::EventLog;

/// Masque surveillé : écriture terminée, arrivée (rotation/édition
/// atomique), création. Une suppression seule ne crée rien à relire.
#[cfg(target_os = "linux")]
const MASQUE: u32 = libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO | libc::IN_CREATE;
/// Tampon de lecture inotify (file vidée à chaque tour, timeout 0).
#[cfg(target_os = "linux")]
const TAMPON_LECTURE: usize = 4096;
/// Taille de l'en-tête `inotify_event` (`wd/mask/cookie/len`, 4×u32).
#[cfg(target_os = "linux")]
const TETE_EVENEMENT: usize = 16;
/// Erreurs de lecture consécutives avant hors-service définitif.
const SEUIL_ECHECS: u8 = 3;

/// Un tour éveillé : même signature d'appel que `sonde::tour` (swap 1 ligne
/// dans `tick`). Scrute via `sonde` si inotify signale, sinon rien ; repli
/// sur `sonde::tour` throttlée si inotify est indisponible (fd<0), si le
/// verrou est empoisonné ou après 3 erreurs de lecture consécutives.
pub fn tour(events: &EventLog) {
    static EVEIL: OnceLock<Mutex<Eveil>> = OnceLock::new();
    let dir = BcastDir::defaut();
    let mutex = EVEIL.get_or_init(|| Mutex::new(Eveil::ouvrir(&dir)));
    let Ok(mut eveil) = mutex.lock() else {
        sonde::tour(events);
        return;
    };
    eveil.ressaisir_si_deplacee(&dir);
    // `None` (erreur persistante) → fallback : comportement inchangé.
    if eveil.evenements().unwrap_or(true) {
        sonde::tour(events);
    }
}

/// Variante injectable (tests, rejeu) : même éveil mais sur `dir` explicite
/// et via `sonde::scrute` (non throttlée, dossier passé en paramètre).
#[cfg(test)]
pub(crate) fn scrute_avec(dir: &BcastDir, log: &EventLog, eveil: &mut Eveil) {
    eveil.ressaisir_si_deplacee(dir);
    if eveil.evenements().unwrap_or(true) {
        sonde::scrute(dir, log);
    }
}

/// Veilleur inotify : fd non bloquant + watches sur racine/`bus`/`.hb`.
/// Détenu par le `OnceLock` de `tour` (aucun état dans `App`).
/// Hors Linux les champs ne servent qu'au stub (`allow` documenté).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) struct Eveil {
    /// Descripteur inotify (`-1` = hors service → fallback permanent).
    fd: i32,
    /// Racine surveillée (déplacement = réouverture, cf. `BCAST_DIR`).
    racine: PathBuf,
    /// Faux tant qu'un dossier manque (reposé à sa réapparition, sans mkdir).
    complet: bool,
    /// Erreurs de lecture consécutives (hors `EAGAIN`/`EINTR`).
    echecs: u8,
}

impl Eveil {
    /// Ouvre inotify (`NONBLOCK`+`CLOEXEC`) et pose les watches best-effort
    /// (dossier manquant = réessayé plus tard, jamais de `mkdir`).
    #[cfg(target_os = "linux")]
    pub(crate) fn ouvrir(dir: &BcastDir) -> Self {
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        let mut eveil = Self {
            fd,
            racine: dir.racine().to_path_buf(),
            complet: false,
            echecs: 0,
        };
        eveil.termine();
        eveil
    }

    /// Hors Linux : d'emblée hors service (pas d'inotify).
    #[cfg(not(target_os = "linux"))]
    pub(crate) fn ouvrir(dir: &BcastDir) -> Self {
        Self {
            fd: -1,
            racine: dir.racine().to_path_buf(),
            complet: true,
            echecs: 0,
        }
    }

    /// Veilleur volontairement hors service (tests, fallback forcé).
    #[cfg(test)]
    pub(crate) fn hors_service() -> Self {
        Self {
            fd: -1,
            racine: PathBuf::new(),
            complet: true,
            echecs: SEUIL_ECHECS,
        }
    }

    /// Repose les watches manquantes (idempotent : ré-ajouter un chemin
    /// déjà surveillé rend le même descripteur de watch).
    #[cfg(target_os = "linux")]
    fn termine(&mut self) {
        if self.complet || self.fd < 0 {
            return;
        }
        let dossiers = [
            self.racine.clone(),
            self.racine.join("bus"),
            self.racine.join(".hb"),
        ];
        self.complet = dossiers.iter().all(|d| surveille(self.fd, d));
    }

    /// Vide la file inotify (timeout 0) : `Some(true)` = activité à relire,
    /// `Some(false)` = silence, `None` = erreur persistante → fallback.
    #[cfg(target_os = "linux")]
    fn evenements(&mut self) -> Option<bool> {
        if self.fd < 0 {
            return None;
        }
        self.termine();
        let mut tampon = [0u8; TAMPON_LECTURE];
        let lus = unsafe { libc::read(self.fd, tampon.as_mut_ptr().cast(), tampon.len()) };
        if lus > 0 {
            self.echecs = 0;
            let (activite, reterminer) = bouge(&tampon[..lus as usize]);
            if reterminer {
                self.complet = false;
            }
            return Some(activite);
        }
        if lus == 0 {
            return Some(false);
        }
        match errno() {
            e if e == libc::EAGAIN || e == libc::EWOULDBLOCK || e == libc::EINTR => {
                self.echecs = 0;
                Some(false)
            }
            e if e == libc::EBADF => {
                self.met_hors_service();
                None
            }
            _ => {
                self.echecs = self.echecs.saturating_add(1);
                if self.echecs >= SEUIL_ECHECS {
                    self.met_hors_service();
                    None
                } else {
                    Some(false)
                }
            }
        }
    }

    /// Hors Linux : `None` définitif → `tour` replie sur `sonde::tour`.
    #[cfg(not(target_os = "linux"))]
    fn evenements(&mut self) -> Option<bool> {
        None
    }

    /// Rouvre tout si la racine a changé (`BCAST_DIR` modifié entre 2 ticks).
    #[cfg(target_os = "linux")]
    fn ressaisir_si_deplacee(&mut self, dir: &BcastDir) {
        if dir.racine() == self.racine {
            return;
        }
        if self.fd >= 0 {
            unsafe { libc::close(self.fd) };
            // -1 AVANT réassignation : `*self = ...` droppe l'ancien
            // `self` en place, et `Drop` refermerait le fd (double
            // close, voire fermeture du nouveau fd si le noyau a
            // recyclé le numéro entre-temps).
            self.fd = -1;
        }
        *self = Self::ouvrir(dir);
    }

    /// Hors Linux : suit juste la racine (pas de fd à rouvrir).
    #[cfg(not(target_os = "linux"))]
    fn ressaisir_si_deplacee(&mut self, dir: &BcastDir) {
        self.racine = dir.racine().to_path_buf();
    }

    /// Ferme le fd et bascule en fallback permanent (`sonde::tour` seule).
    #[cfg(target_os = "linux")]
    fn met_hors_service(&mut self) {
        if self.fd >= 0 {
            unsafe { libc::close(self.fd) };
        }
        self.fd = -1;
        self.complet = true;
    }
}

#[cfg(target_os = "linux")]
impl Drop for Eveil {
    /// Ferme le fd inotify (les watches tombent avec lui).
    fn drop(&mut self) {
        if self.fd >= 0 {
            unsafe { libc::close(self.fd) };
            self.fd = -1;
        }
    }
}

/// Pose un watch sur `dossier` (faux = chemin à octet nul ou `add_watch`
/// refusé : dossier manquant, réessayé au tour suivant).
#[cfg(target_os = "linux")]
fn surveille(fd: i32, dossier: &Path) -> bool {
    let Ok(chemin) = CString::new(dossier.as_os_str().as_bytes()) else {
        return false;
    };
    unsafe { libc::inotify_add_watch(fd, chemin.as_ptr(), MASQUE) >= 0 }
}

/// Découpe les `inotify_event` bruts : `(activité, reterminer)`.
/// `Q_OVERFLOW` (file pleine, événements perdus) force la relecture ;
/// `IGNORED` (dossier parti) force la repose des watches.
#[cfg(target_os = "linux")]
fn bouge(brut: &[u8]) -> (bool, bool) {
    let (mut activite, mut reterminer) = (false, false);
    let mut i = 0;
    while i + TETE_EVENEMENT <= brut.len() {
        let masque = u32::from_ne_bytes(brut[i + 4..i + 8].try_into().unwrap_or([0; 4]));
        let nom = u32::from_ne_bytes(brut[i + 12..i + 16].try_into().unwrap_or([0; 4])) as usize;
        if masque & MASQUE != 0 || masque & libc::IN_Q_OVERFLOW != 0 {
            activite = true;
        }
        if masque & libc::IN_IGNORED != 0 {
            reterminer = true;
        }
        i += TETE_EVENEMENT + nom;
    }
    (activite, reterminer)
}

/// Dernier `errno` (lecture seule, sans `unsafe` direct).
#[cfg(target_os = "linux")]
fn errno() -> libc::c_int {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(target_os = "linux")]
    use std::time::{Duration, Instant};

    /// Bac jetable initialisé (`bus/`, `.hb/`, journaux vides).
    fn bac(tag: &str) -> (PathBuf, BcastDir) {
        let racine =
            std::env::temp_dir().join(format!("crypte-eveil-test-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&racine);
        let dir = BcastDir::depuis(racine.clone());
        dir.init().expect("init crée l'arborescence");
        (racine, dir)
    }

    /// Attend une activité inotify (borne 1 s, grain 10 ms).
    #[cfg(target_os = "linux")]
    fn attend(eveil: &mut Eveil) -> Option<bool> {
        let fin = Instant::now() + Duration::from_millis(1000);
        loop {
            match eveil.evenements() {
                Some(true) => return Some(true),
                None => return None,
                Some(false) if Instant::now() >= fin => return Some(false),
                Some(false) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn silence_sans_ecriture() {
        let (_racine, dir) = bac("silence");
        let mut eveil = Eveil::ouvrir(&dir);
        assert_eq!(eveil.evenements(), Some(false));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn ecriture_inbox_detectee_moins_d_une_seconde() {
        let (_racine, dir) = bac("inbox");
        let mut eveil = Eveil::ouvrir(&dir);
        assert_eq!(eveil.evenements(), Some(false));
        fs::write(dir.boite(), "- [2026-11-08 10:00] x [P1] : bonjour {#p}\n").unwrap();
        assert_eq!(attend(&mut eveil), Some(true));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn bus_et_hb_detectes() {
        let (_racine, dir) = bac("bushb");
        let mut eveil = Eveil::ouvrir(&dir);
        assert_eq!(eveil.evenements(), Some(false));
        fs::write(dir.bus("gpu"), "- [2026-11-08 10:00] g [P2] : cadre {#p}\n").unwrap();
        assert_eq!(attend(&mut eveil), Some(true));
        assert_eq!(eveil.evenements(), Some(false));
        fs::write(dir.hb("sonde-test"), "1 2026-11-08 10:00 vu\n").unwrap();
        assert_eq!(attend(&mut eveil), Some(true));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn sans_evenement_sonde_pas_appelee() {
        let (racine, dir) = bac("vide");
        let journal = racine.join("events.jsonl");
        let log = EventLog::new(Some(journal.clone()));
        sonde::scrute(&dir, &log); // amorce les offsets
        let mut eveil = Eveil::ouvrir(&dir);
        for _ in 0..100 {
            if eveil.evenements() != Some(true) {
                break;
            }
        }
        let avant = fs::read(&journal).unwrap_or_default();
        scrute_avec(&dir, &log, &mut eveil);
        let apres = fs::read(&journal).unwrap_or_default();
        assert_eq!(avant, apres, "tour à vide = zéro octet ajouté");
    }

    #[test]
    fn fallback_fd_ferme() {
        let (racine, dir) = bac("fallback");
        let journal = racine.join("events.jsonl");
        let log = EventLog::new(Some(journal.clone()));
        let mut eveil = Eveil::hors_service();
        // Même racine : pas de réouverture, le fd reste fermé (panne franche).
        eveil.racine = dir.racine().to_path_buf();
        assert_eq!(eveil.evenements(), None);
        // Fallback = `sonde::scrute` (amorce, marqueur `.alive`), sans panique.
        scrute_avec(&dir, &log, &mut eveil);
        assert!(racine.join(".mbox-sonde.alive").is_file());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn reouverture_si_racine_change() {
        let (_a, dir_a) = bac("racinea");
        let (_b, dir_b) = bac("racineb");
        let mut eveil = Eveil::ouvrir(&dir_a);
        assert_eq!(eveil.evenements(), Some(false));
        eveil.ressaisir_si_deplacee(&dir_b);
        assert_eq!(eveil.racine, dir_b.racine());
        assert!(eveil.fd >= 0);
    }
}
