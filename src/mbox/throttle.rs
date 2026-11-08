//! Lot throttle — dedup, quota glissant, état change-only et digest.
//!
//! Équivalents Rust de `lib-throttle.sh` : dedup inbox 30 min + quota 6/h,
//! dedup bus 5 min, compteurs pending, `digest_flush`. Toute normalisation
//! d'identité passe par le SOCLE, jamais ici.
//!
//! Signatures partagées du SOCLE utilisées (`super::format`, tranche
//! assembleur : `BcastDir`/`sanitize`/`sha1_hex` réexportés depuis le
//! socle, `Priorite`/`construire_ligne` définis dans `format`) :
//! ```text
//! BcastDir::depuis(PathBuf) -> BcastDir
//! BcastDir::etat(&self, emetteur, suffixe) -> PathBuf  // un fichier `.state`
//! BcastDir::boite(&self) -> PathBuf         // fichier `BOITE-USER.md`
//! sanitize(ident: &str) -> String           // `bcast_sanitize` : propre 40 + hash 8
//! sha1_hex(octets: &[u8]) -> String         // sha1 brut SANS newline (40 hex)
//! Priorite::P2 + construire_ligne(exp, prio, dest, msg) -> String  // dest "" = nu
//! ```
//! Shell -> Rust : `throttle_state_file` -> [`fichier_etat`], `pending_bump`
//! -> [`pending_incremente`], `pending_take` -> [`pending_prend`],
//! (`pending_reset` n'a aucun appelant shell : non porté, `pending_prend`
//! fait la RAZ atomique), `throttle_dedup` -> [`dedup`],
//! `throttle_rate` -> [`quota`], `throttle_inbox` -> [`garde_inbox`],
//! dedup bus 300 -> [`garde_bus`], `throttle_state` -> [`etat_change`],
//! `digest_flush` -> `super::digest` (même verrou, voir ce module).
//! Interop : formats v2 à l'identique ; lignes v1/illisibles = WARN +
//! ignorées, reconstruites au fil de l'eau ; `flock` 10 s, fail-open sur
//! dedup/quota/état, digest : expéditeur fautif sauté (comme le shell).

use super::format::{sanitize, sha1_hex, BcastDir};
use std::fs::{self, File, OpenOptions};
use std::io::Error as ErreurOs;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Fenêtre dedup inbox : 30 min (`throttle_inbox`).
pub const DEDUP_INBOX_S: i64 = 1800;
/// Quota inbox : 6 messages par heure glissante.
pub const QUOTA_INBOX_MAX: usize = 6;
/// Fenêtre du quota : heure glissante (horodatages, pas fenêtre fixe).
pub const QUOTA_FENETRE_S: i64 = 3600;
/// Fenêtre dedup bus : 5 min (`cmd_bus`).
pub const DEDUP_BUS_S: i64 = 300;
/// Historique dedup : 20 derniers hash (élagués à la fenêtre).
pub const DEDUP_HISTO_MAX: usize = 20;
/// Délai verrou : 10 s comme `flock -w 10` du shell.
const DELAI_VERROU_S: u64 = 10;

/// Motif de throttle. Verdicts CLI : `dup` / `rate` (`throttle_inbox`),
/// `same-state` (`cmd_send`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotifCompteur {
    /// Même corps déjà vu dans la fenêtre.
    Doublon,
    /// Quota horaire dépassé.
    Quota,
    /// Valeur d'état inchangée.
    MemeEtat,
}

impl MotifCompteur {
    /// Jeton CLI (`throttled:<jeton>` ou `same-state`).
    pub fn verdict(&self) -> &'static str {
        match self {
            Self::Doublon => "dup",
            Self::Quota => "rate",
            Self::MemeEtat => "same-state",
        }
    }
}

/// Compteurs pending `<san>.pending`, format shell `dup rate same last`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Compteurs {
    /// Écartés comme doublons.
    pub doublons: u64,
    /// Écartés par le quota.
    pub quotas: u64,
    /// Envois `--state` à valeur identique.
    pub memes: u64,
    /// Époque du dernier `pending_prend` (préservée par incrément).
    pub dernier: i64,
}

impl Compteurs {
    /// Parse tolérant : première ligne, champ non numérique = 0.
    fn depuis_ligne(texte: &str) -> Self {
        let mots: Vec<&str> = texte
            .lines()
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect();
        let nombre = |i: usize| -> u64 { mots.get(i).and_then(|m| m.parse().ok()).unwrap_or(0) };
        Self {
            doublons: nombre(0),
            quotas: nombre(1),
            memes: nombre(2),
            dernier: mots.get(3).and_then(|m| m.parse().ok()).unwrap_or(0),
        }
    }
    /// Sérialise au format shell avec newline finale.
    fn vers_ligne(&self) -> String {
        format!(
            "{} {} {} {}\n",
            self.doublons, self.quotas, self.memes, self.dernier
        )
    }
}

/// Garde RAII : `flock LOCK_UN` à la destruction (le `close` libère aussi).
struct GardeVerrou {
    fichier: File,
}

impl Drop for GardeVerrou {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.fichier.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

/// Époque UTC en secondes (`bcast_epoch` via l'horloge système).
fn epoque() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Avertissement non fatal sur stderr, préfixe identique au shell.
fn avertit(cause: &str) {
    eprintln!("bcast: WARN {cause}");
}

/// Chemin du verrou dédié : `<cible>.lock` (fichier séparé, comme shell).
fn chemin_verrou(cible: &Path) -> PathBuf {
    PathBuf::from(format!("{}.lock", cible.display()))
}

/// Verrou exclusif avec timeout (`flock -w` : boucle + LOCK_NB).
/// `None` = impossible (timeout ou erreur) ; l'appelant décide.
fn verrouille(lock: &Path, delai_s: u64) -> Option<GardeVerrou> {
    if let Some(parent) = lock.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let fichier = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(lock)
        .ok()?;
    let fd = fichier.as_raw_fd();
    let debut = Instant::now();
    loop {
        let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            return Some(GardeVerrou { fichier });
        }
        let bloque = ErreurOs::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK);
        if !bloque || debut.elapsed() >= Duration::from_secs(delai_s) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Parse une ligne dedup v2 (`<epoch> <sha1>`). Ligne v1 inverse ou
/// illisible = WARN + `None` (ignorée, reconstruite au fil de l'eau).
fn analyse_dedup(ligne: &str) -> Option<(i64, String)> {
    let mots: Vec<&str> = ligne.split_whitespace().collect();
    if mots.len() == 2 {
        if let Ok(moment) = mots[0].parse::<i64>() {
            return Some((moment, mots[1].to_string()));
        }
        if mots[1].parse::<i64>().is_ok() {
            avertit("dedup: ligne v1 inverse ignorée, reconstruite au fil de l'eau");
            return None;
        }
    }
    avertit("dedup: ligne illisible ignorée");
    None
}

/// Parse une ligne quota v2 (une époque seule). Ligne v1 (`<epoch>
/// <compte>`) ou illisible = WARN + `None`.
fn analyse_quota(ligne: &str) -> Option<i64> {
    match ligne.trim().parse::<i64>() {
        Ok(moment) => Some(moment),
        Err(_) => {
            avertit("quota: ligne v1/illisible ignorée, reconstruite au fil de l'eau");
            None
        }
    }
}

/// Fichier d'état `<etat>/<san(exp)>.<suffixe>` (`throttle_state_file`).
/// Crée `.state` au besoin (le shell suppose `bcast_init` déjà passé).
pub fn fichier_etat(dir: &BcastDir, expediteur: &str, suffixe: &str) -> PathBuf {
    let cible = dir.etat(expediteur, suffixe);
    if let Some(parent) = cible.parent() {
        let _ = fs::create_dir_all(parent);
    }
    cible
}

/// Incrémente un compteur pending sous verrou dédié. Faux = compteur
/// perdu (verrou/écriture), verdict déjà décidé sinon.
pub fn pending_incremente(dir: &BcastDir, expediteur: &str, motif: MotifCompteur) -> bool {
    let cible = fichier_etat(dir, expediteur, "pending");
    let Some(garde) = verrouille(&chemin_verrou(&cible), DELAI_VERROU_S) else {
        avertit("compteur digest perdu (verrou)");
        return false;
    };
    let mut compteurs = fs::read_to_string(&cible)
        .map(|t| Compteurs::depuis_ligne(&t))
        .unwrap_or_default();
    match motif {
        MotifCompteur::Doublon => compteurs.doublons += 1,
        MotifCompteur::Quota => compteurs.quotas += 1,
        MotifCompteur::MemeEtat => compteurs.memes += 1,
    }
    let ok = fs::write(&cible, compteurs.vers_ligne()).is_ok();
    drop(garde);
    ok
}

/// Prend les compteurs + RAZ atomique sous le même verrou.
/// `None` = verrou impossible, sauter l'expéditeur (comme le shell).
pub fn pending_prend(chemin: &Path) -> Option<Compteurs> {
    let garde = verrouille(&chemin_verrou(chemin), DELAI_VERROU_S)?;
    let compteurs = fs::read_to_string(chemin)
        .map(|t| Compteurs::depuis_ligne(&t))
        .unwrap_or_default();
    let raz = Compteurs {
        dernier: epoque(),
        ..Default::default()
    };
    let _ = fs::write(chemin, raz.vers_ligne());
    drop(garde);
    Some(compteurs)
}

/// Dedup : même corps dans la fenêtre ? Faux = doublon (compteur +1).
/// Verrou impossible = fail-open (WARN + vrai), comme le shell.
pub fn dedup(dir: &BcastDir, cle: &str, fenetre_s: i64, corps: &str) -> bool {
    let cible = fichier_etat(dir, cle, "dedup");
    let condense = sha1_hex(corps.as_bytes());
    let maintenant = epoque();
    let Some(garde) = verrouille(&chemin_verrou(&cible), DELAI_VERROU_S) else {
        avertit("dedup sans verrou (fail-open)");
        return true;
    };
    let mut frais: Vec<String> = Vec::new();
    if let Ok(contenu) = fs::read_to_string(&cible) {
        for ligne in contenu.lines() {
            let Some((moment, hash)) = analyse_dedup(ligne) else {
                continue;
            };
            if maintenant - moment >= fenetre_s {
                continue;
            }
            if hash == condense {
                drop(garde);
                let _ = pending_incremente(dir, cle, MotifCompteur::Doublon);
                return false;
            }
            frais.push(format!("{moment} {hash}"));
        }
    }
    frais.push(format!("{maintenant} {condense}"));
    let debut = frais.len().saturating_sub(DEDUP_HISTO_MAX);
    let _ = fs::write(&cible, frais[debut..].join("\n") + "\n");
    drop(garde);
    true
}

/// Quota : max messages par heure glissante. Faux = refus (compteur +1).
/// Verrou impossible = fail-open (WARN + vrai), comme le shell.
pub fn quota(dir: &BcastDir, expediteur: &str, max: usize) -> bool {
    let cible = fichier_etat(dir, expediteur, "ratel");
    let maintenant = epoque();
    let Some(garde) = verrouille(&chemin_verrou(&cible), DELAI_VERROU_S) else {
        avertit("quota sans verrou (fail-open)");
        return true;
    };
    let mut frais: Vec<i64> = Vec::new();
    if let Ok(contenu) = fs::read_to_string(&cible) {
        for ligne in contenu.lines() {
            let Some(moment) = analyse_quota(ligne) else {
                continue;
            };
            if maintenant - moment < QUOTA_FENETRE_S {
                frais.push(moment);
            }
        }
    }
    if frais.len() >= max {
        let lignes: Vec<String> = frais.iter().map(|t| t.to_string()).collect();
        let texte = if lignes.is_empty() {
            String::new()
        } else {
            lignes.join("\n") + "\n"
        };
        let _ = fs::write(&cible, texte);
        drop(garde);
        let _ = pending_incremente(dir, expediteur, MotifCompteur::Quota);
        return false;
    }
    frais.push(maintenant);
    let lignes: Vec<String> = frais.iter().map(|t| t.to_string()).collect();
    let _ = fs::write(&cible, lignes.join("\n") + "\n");
    drop(garde);
    true
}

/// Garde inbox : dedup 30 min puis quota 6/h (`throttle_inbox`).
/// `saute_dedup` = un `--state` qui change gagne sur le dedup, quota restant.
pub fn garde_inbox(
    dir: &BcastDir,
    expediteur: &str,
    corps: &str,
    saute_dedup: bool,
) -> Result<(), MotifCompteur> {
    debug_assert_eq!((DEDUP_INBOX_S, QUOTA_INBOX_MAX), (1800, 6));
    if !saute_dedup && !dedup(dir, expediteur, DEDUP_INBOX_S, corps) {
        return Err(MotifCompteur::Doublon);
    }
    if !quota(dir, expediteur, QUOTA_INBOX_MAX) {
        return Err(MotifCompteur::Quota);
    }
    Ok(())
}

/// Garde bus : dedup 5 min, pas de quota, clé `<from>:bus:<sujet>` (shell).
pub fn garde_bus(dir: &BcastDir, expediteur: &str, sujet: &str, corps: &str) -> bool {
    debug_assert_eq!(DEDUP_BUS_S, 300);
    dedup(
        dir,
        &format!("{expediteur}:bus:{sujet}"),
        DEDUP_BUS_S,
        corps,
    )
}

/// État change-only : n'écrit que si la valeur change (`throttle_state`).
/// Vrai = changé (passe), faux = identique (compteur `same` +1).
pub fn etat_change(dir: &BcastDir, expediteur: &str, cle: &str, valeur: &str) -> bool {
    let cible = fichier_etat(dir, expediteur, &format!("st-{}", sanitize(cle)));
    let Some(garde) = verrouille(&chemin_verrou(&cible), DELAI_VERROU_S) else {
        avertit("état sans verrou (fail-open)");
        return true;
    };
    let precedent = fs::read(&cible)
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o)
                .trim_end_matches(['\n', '\r'])
                .to_string()
        })
        .unwrap_or_default();
    if precedent == valeur {
        drop(garde);
        let _ = pending_incremente(dir, expediteur, MotifCompteur::MemeEtat);
        return false;
    }
    let _ = fs::write(&cible, valeur);
    drop(garde);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COMPTEUR: AtomicU64 = AtomicU64::new(0);

    /// Dossier broadcast temporaire unique (jamais le vrai `.scratch`).
    fn dir_temp() -> (BcastDir, PathBuf) {
        let numero = COMPTEUR.fetch_add(1, Ordering::SeqCst);
        let racine =
            std::env::temp_dir().join(format!("bcast-throttle-{}-{numero}", std::process::id()));
        let _ = fs::create_dir_all(&racine);
        (BcastDir::depuis(racine.clone()), racine)
    }

    #[test]
    fn dedup_nouveau_puis_doublon() {
        let (dir, racine) = dir_temp();
        assert!(dedup(&dir, "agent-a", 1800, "bonjour"));
        assert!(!dedup(&dir, "agent-a", 1800, "bonjour"));
        assert!(dedup(&dir, "agent-a", 1800, "autre corps"));
        let c = pending_prend(&fichier_etat(&dir, "agent-a", "pending")).expect("pending lisible");
        assert_eq!((c.doublons, c.quotas, c.memes), (1, 0, 0));
        let _ = fs::remove_dir_all(racine);
    }

    #[test]
    fn dedup_fenetre_expiree_repasse() {
        let (dir, racine) = dir_temp();
        assert!(dedup(&dir, "agent-b", 0, "même corps"));
        assert!(dedup(&dir, "agent-b", 0, "même corps"));
        let _ = fs::remove_dir_all(racine);
    }

    #[test]
    fn quota_six_par_heure_glissante() {
        let (dir, racine) = dir_temp();
        for _ in 0..6 {
            assert!(quota(&dir, "agent-c", 6));
        }
        assert!(!quota(&dir, "agent-c", 6));
        let c = pending_prend(&fichier_etat(&dir, "agent-c", "pending")).expect("pending lisible");
        assert_eq!(c.quotas, 1);
        let _ = fs::remove_dir_all(racine);
    }

    #[test]
    fn inbox_dedup_puis_skip_dedup() {
        let (dir, racine) = dir_temp();
        assert_eq!(garde_inbox(&dir, "agent-d", "corps", false), Ok(()));
        assert_eq!(
            garde_inbox(&dir, "agent-d", "corps", false),
            Err(MotifCompteur::Doublon)
        );
        assert_eq!(garde_inbox(&dir, "agent-d", "corps", true), Ok(()));
        assert_eq!(MotifCompteur::Doublon.verdict(), "dup");
        assert_eq!(MotifCompteur::Quota.verdict(), "rate");
        let _ = fs::remove_dir_all(racine);
    }

    #[test]
    fn bus_dedup_cinq_minutes_par_sujet() {
        let (dir, racine) = dir_temp();
        assert!(garde_bus(&dir, "agent-e", "gpu", "panne"));
        assert!(!garde_bus(&dir, "agent-e", "gpu", "panne"));
        assert!(garde_bus(&dir, "agent-e", "file", "panne"));
        let _ = fs::remove_dir_all(racine);
    }

    #[test]
    fn etat_change_only() {
        let (dir, racine) = dir_temp();
        assert!(etat_change(&dir, "agent-f", "mode", "normal"));
        assert!(!etat_change(&dir, "agent-f", "mode", "normal"));
        assert!(etat_change(&dir, "agent-f", "mode", "panne"));
        assert_eq!(MotifCompteur::MemeEtat.verdict(), "same-state");
        let c = pending_prend(&fichier_etat(&dir, "agent-f", "pending")).expect("pending lisible");
        assert_eq!(c.memes, 1);
        let _ = fs::remove_dir_all(racine);
    }

    #[test]
    fn legacy_v1_ignore_et_reconstruit() {
        let (dir, racine) = dir_temp();
        let vieux = epoque() - 100;
        fs::write(
            fichier_etat(&dir, "agent-h", "dedup"),
            format!("abcdef0123 {vieux}\n"),
        )
        .unwrap();
        fs::write(
            fichier_etat(&dir, "agent-h", "ratel"),
            format!("{vieux} 7\n"),
        )
        .unwrap();
        assert!(dedup(&dir, "agent-h", 1800, "corps neuf"));
        assert!(quota(&dir, "agent-h", 6));
        let relu = fs::read_to_string(fichier_etat(&dir, "agent-h", "dedup")).unwrap();
        assert!(!relu.contains("abcdef0123"), "v1 non purgé : {relu}");
        let _ = fs::remove_dir_all(racine);
    }
}
