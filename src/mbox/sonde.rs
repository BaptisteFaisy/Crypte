//! Sonde boîte/bus/ordres/hb (lot `watch`, vague 2).
//!
//! Nom final assembleur : `src/mbox/sonde.rs` (`mod.rs` du lot socle).
//! Appelée depuis `tick()` par une seule ligne, cadence interne 15 s :
//! aucun état dans `App`, aucune variante `Incoming`, aucun spawn
//! (tranchage du delta : `app.rs` 498L/22fn ne tient que +1 ligne).
//!
//! # Signatures partagées utilisées (socle réel `socle/mod.rs`, vérifié)
//!
//! - `super::{BcastDir, SUJETS_BUS, epoque}` (`mod.rs` du socle) :
//!   `defaut()`, `depuis(PathBuf)`, `racine()->&Path`,
//!   `joindre(&str)->PathBuf`, `boite/ordres/acks/tailles()->PathBuf`,
//!   `bus(&str)->PathBuf` (fichier `bus/<sujet>.log`), `epoque()->u64`.
//!   Dossier `.hb` via `joindre(".hb")` (`hb(nom)` = un fichier).
//! - `super::format::{est_message, est_suite, priorite_ligne, Priorite}`
//!   (`format.rs` à créer par l'assembleur, voir `CABLAGE.md` : n'existe
//!   dans aucun lot ; logique = `lib-common.sh` L57-70).
//! - `crate::events::{Event, EventLog}` + patch `CABLAGE.md` :
//!   `Event::mailbox(canal, priorite, texte)` (seul ajout dans `events.rs`).
//!
//! # Miroirs shell (refs : `.scratch/broadcast`, lecture seule)
//!
//! `lit_nouveau`=`read_new` (`notifier2.sh` L31-78), `traite_inbox`=
//! `process_inbox` (L98-124), `traite_ordres`/`traite_acks`=`process_ordres`
//! (L126-138), `verifie_hb`=`check_hb` (`agents-watch2.sh` L129-156).
//!
//! # Écarts assumés (jamais de replay sauvage)
//!
//! Offsets propres (pas de course avec `notifier2`), illisible → amorçage,
//! 64 émissions max/fichier/tour, pas de `mkdir`, alertes via `EventLog`.

use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use super::format::{est_message, est_suite, priorite_ligne, Priorite};
use super::{epoque, BcastDir, SUJETS_BUS};
use crate::events::{Event, EventLog};

/// Cadence max d'un tour (s) : `NOTIFIER_INTERVAL` (`notifier2.sh`).
const INTERVALLE: u64 = 15;
/// Âge mtime d'un heartbeat muet (s, `agents-watch2.sh` L139).
const SEUIL_MUET: u64 = 900;
/// Délai de ré-alerte par sonde (s, `agents-watch2.sh` L141).
const REARMEMENT: u64 = 3600;
/// Émissions max par fichier et par tour (replay borné).
const LIMITE_EMISSION: usize = 64;
/// Texte max par événement (caractères, `${l:0:300}` du shell).
const TEXTE_MAX: usize = 300;
/// Lignes de `.sizes` inspectées (`tail -n 20`, `notifier2.sh` L46).
const QUEUE_TAILLES: usize = 20;

/// Un tour throtté : l'unique appel depuis `tick()` (`app.rs`, +1 ligne).
pub(crate) fn tour(log: &EventLog) {
    static DERNIER: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    let verrou = DERNIER.get_or_init(|| Mutex::new(None));
    let mut garde = verrou.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(dernier) = *garde {
        if dernier.elapsed().as_secs() < INTERVALLE {
            return;
        }
    }
    *garde = Some(Instant::now());
    scrute(&BcastDir::defaut(), log);
}

/// Un tour complet sans throttle (tests, rejeu assembleur).
pub(crate) fn scrute(dir: &BcastDir, log: &EventLog) {
    let _ = fs::write(dir.racine().join(".mbox-sonde.alive"), b"");
    let lignes = lit_nouveau(dir, log, &dir.boite(), "inbox");
    traite_inbox(&lignes, "inbox", log);
    for sujet in SUJETS_BUS {
        let tag = format!("bus-{sujet}");
        let canal = format!("bus/{sujet}");
        traite_inbox(&lit_nouveau(dir, log, &dir.bus(sujet), &tag), &canal, log);
    }
    let lignes = lit_nouveau(dir, log, &dir.ordres(), "ordres");
    traite_ordres(&lignes, log);
    let lignes = lit_nouveau(dir, log, &dir.acks(), "ack");
    traite_acks(&lignes, log);
    verifie_hb(dir, log);
}

/// Octets nouveaux d'un fichier (lignes complètes seules), offset à jour.
/// Miroir de `read_new` : amorçage, rotation, troncation, ligne incomplète.
fn lit_nouveau(dir: &BcastDir, log: &EventLog, chemin: &Path, tag: &str) -> Vec<String> {
    let Ok(meta) = fs::metadata(chemin) else {
        return Vec::new();
    };
    let (dev, ino, taille) = (meta.dev(), meta.ino(), meta.len());
    let offset = dir.racine().join(format!(".mbox-sonde.{tag}.offset"));
    let base = chemin.file_name().and_then(|b| b.to_str()).unwrap_or(tag);
    let pos = match lit_offset(&offset) {
        None => {
            ecrit_offset(&offset, dev, ino, taille);
            return Vec::new();
        }
        Some((odev, oino, _)) if (odev, oino) != (dev, ino) => {
            if rotation_recente(&dir.tailles(), base) {
                ecrit_offset(&offset, dev, ino, taille);
                return Vec::new();
            }
            0
        }
        Some((_, _, p)) if taille < p => {
            signale_troncation(dir, base, p, taille);
            ecrit_offset(&offset, dev, ino, taille);
            let texte = format!("{tag} tronqué hors rotation ({p} -> {taille})");
            emet(log, "alerte", "P0", &texte);
            return Vec::new();
        }
        Some((_, _, p)) => p,
    };
    if taille <= pos {
        ecrit_offset(&offset, dev, ino, pos);
        return Vec::new();
    }
    let Ok(mut fichier) = fs::File::open(chemin) else {
        return Vec::new();
    };
    let mut brut = Vec::new();
    if fichier.seek(SeekFrom::Start(pos)).is_err() || fichier.read_to_end(&mut brut).is_err() {
        return Vec::new();
    }
    let (lignes, avance) = decoupe(&brut, brut.last() == Some(&b'\n'));
    ecrit_offset(&offset, dev, ino, pos + avance as u64);
    lignes
}

/// Lit `dev:ino:pos` (`None` = absent/corrompu → amorçage, pas replay).
fn lit_offset(chemin: &Path) -> Option<(u64, u64, u64)> {
    let contenu = fs::read_to_string(chemin).ok()?;
    let mut p = contenu.trim().split(':');
    let (a, b, c) = (p.next()?, p.next()?, p.next()?);
    Some((a.parse().ok()?, b.parse().ok()?, c.parse().ok()?))
}

/// Écrit `dev:ino:pos` sans newline final (comme le shell).
fn ecrit_offset(chemin: &Path, dev: u64, ino: u64, pos: u64) {
    let _ = fs::write(chemin, format!("{dev}:{ino}:{pos}"));
}

/// Vrai si `.sizes` mentionne une rotation récente de `base`.
fn rotation_recente(tailles: &Path, base: &str) -> bool {
    let Ok(contenu) = fs::read_to_string(tailles) else {
        return false;
    };
    let aiguille = format!("rotate {base}");
    contenu
        .lines()
        .rev()
        .take(QUEUE_TAILLES)
        .any(|l| l.contains(&aiguille))
}

/// Append best-effort `shrink-unexpected` dans `.sizes` (miroir L59).
fn signale_troncation(dir: &BcastDir, base: &str, ancien: u64, nouveau: u64) {
    let ligne = format!(
        "{} shrink-unexpected {base} old={ancien} new={nouveau}\n",
        epoque()
    );
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.tailles())
    {
        let _ = f.write_all(ligne.as_bytes());
    }
}

/// Lignes complètes + avance en octets ; dernière ligne retenue si le
/// fichier ne finit pas par `\n` (zéro émission si c'est la seule).
fn decoupe(brut: &[u8], termine_nl: bool) -> (Vec<String>, usize) {
    if brut.is_empty() {
        return (Vec::new(), 0);
    }
    let fin = if termine_nl {
        brut.len()
    } else {
        let Some(i) = brut.iter().rposition(|b| *b == b'\n') else {
            return (Vec::new(), 0);
        };
        i + 1
    };
    let mut morceaux: Vec<&[u8]> = brut[..fin].split(|b| *b == b'\n').collect();
    morceaux.pop();
    let lignes = morceaux
        .iter()
        .map(|m| String::from_utf8_lossy(m).into_owned())
        .collect();
    (lignes, fin)
}

/// Priorités + continuations rattachées (max 2), émission bornée.
fn traite_inbox(lignes: &[String], canal: &str, log: &EventLog) {
    let mut derniere: Option<&str> = None;
    let mut suite = 0_usize;
    let (mut vues, mut emises) = (0_usize, 0_usize);
    for ligne in lignes {
        if ligne.is_empty() {
            continue;
        }
        vues += 1;
        if est_suite(ligne) {
            if suite < 2 {
                if let Some(p) = derniere.filter(|_| emises < LIMITE_EMISSION) {
                    emet(log, canal, p, ligne);
                    emises += 1;
                }
                suite += 1;
            }
            continue;
        }
        if !est_message(ligne) {
            derniere = None;
            continue;
        }
        let p = etiquette(priorite_ligne(ligne));
        if emises < LIMITE_EMISSION {
            emet(log, canal, p, ligne);
            emises += 1;
        }
        derniere = Some(p);
        suite = 0;
    }
    annonce_suite(log, canal, vues.saturating_sub(emises));
}

/// Ordres actifs : `id : texte` (champ 8, 200 octets), priorité P0.
fn traite_ordres(lignes: &[String], log: &EventLog) {
    let (mut vues, mut emises) = (0_usize, 0_usize);
    for ligne in lignes {
        if ligne.is_empty() || !ligne.contains("|actif|") {
            continue;
        }
        vues += 1;
        if emises < LIMITE_EMISSION {
            let id = ligne.split('|').next().unwrap_or("?");
            emet(log, "ordres", "P0", &format!("{id} : {}", champ8(ligne)));
            emises += 1;
        }
    }
    annonce_suite(log, "ordres", vues.saturating_sub(emises));
}

/// Un seul événement récapitulatif P2 (`grep -c .` du shell).
fn traite_acks(lignes: &[String], log: &EventLog) {
    let n = lignes.iter().filter(|l| !l.is_empty()).count();
    if n > 0 {
        emet(log, "acks", "P2", &format!("{n} ACK(s) enregistré(s)"));
    }
}

/// Heartbeats muets : alerte P0 1×/h/sonde (marqueur partagé shell).
fn verifie_hb(dir: &BcastDir, log: &EventLog) {
    let maintenant = epoque();
    if maintenant == 0 {
        return;
    }
    let Ok(entrees) = fs::read_dir(dir.joindre(".hb")) else {
        return;
    };
    for entree in entrees.flatten() {
        let chemin = entree.path();
        if !chemin.is_file() {
            continue;
        }
        let Some(nom) = chemin.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if nom.starts_with("pid-") {
            continue;
        }
        if fs::read_to_string(&chemin).is_ok_and(|c| c.contains("stop-propre")) {
            continue;
        }
        let Some(age) = age_fichier(&chemin, maintenant) else {
            continue;
        };
        if age <= SEUIL_MUET {
            continue;
        }
        let marqueur = dir.racine().join(format!(".hb-alerted-{nom}"));
        if age_fichier(&marqueur, maintenant).is_some_and(|a| a < REARMEMENT) {
            continue;
        }
        let _ = fs::write(&marqueur, b"");
        emet(log, "hb", "P0", &format!("sonde muette : {nom} ({age}s)"));
    }
}

/// Âge mtime en secondes (`None` = illisible, `file_age` → -1).
fn age_fichier(chemin: &Path, maintenant: u64) -> Option<u64> {
    let mtime = fs::metadata(chemin).ok()?.modified().ok()?;
    let epoque = mtime.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(maintenant.saturating_sub(epoque))
}

/// Un fait vers le journal temps réel (texte tronqué à 300 caractères).
fn emet(log: &EventLog, canal: &str, priorite: &str, texte: &str) {
    let court: String = texte.chars().take(TEXTE_MAX).collect();
    log.emit(&Event::mailbox(canal, priorite, &court));
}

/// Étiquette shell d'une priorité (`[P0]`/`[P1]`/`[P2]`).
fn etiquette(p: Priorite) -> &'static str {
    match p {
        Priorite::P0 => "P0",
        Priorite::P1 => "P1",
        Priorite::P2 => "P2",
    }
}

/// Champ 8 coupé à 200 octets (`cut -d'|' -f8 | head -c 200`).
fn champ8(ligne: &str) -> String {
    let champ = ligne.split('|').nth(7).unwrap_or("");
    let octets = champ.as_bytes();
    let coupe = if octets.len() > 200 {
        &octets[..200]
    } else {
        octets
    };
    String::from_utf8_lossy(coupe).into_owned()
}

/// Fin de rafale bornée (`(+n autres, voir bcast tail)`).
fn annonce_suite(log: &EventLog, canal: &str, reste: usize) {
    if reste > 0 {
        emet(
            log,
            canal,
            "P2",
            &format!("(+{reste} autres, voir bcast tail)"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Bac à sable v2 (jamais commité, convention `events.rs`).
    fn bac(tag: &str) -> (PathBuf, BcastDir) {
        let racine =
            std::env::temp_dir().join(format!("crypte-sonde-test-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&racine);
        fs::create_dir_all(racine.join("bus")).unwrap();
        fs::create_dir_all(racine.join(".hb")).unwrap();
        (racine.clone(), BcastDir::depuis(racine.clone()))
    }

    fn compte(path: &Path, aiguille: &str) -> usize {
        fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains(aiguille))
            .count()
    }

    #[test]
    fn amorcage_puis_livraison() {
        let (racine, dir) = bac("amorce");
        fs::write(dir.boite(), "- [2026-11-08 10:00] x [P1] : un {#p}\n").unwrap();
        let journal = racine.join("events.jsonl");
        let log = EventLog::new(Some(journal.clone()));
        scrute(&dir, &log);
        assert!(racine.join(".mbox-sonde.inbox.offset").is_file());
        assert_eq!(compte(&journal, "mailbox"), 0);
        fs::write(
            dir.boite(),
            "- [2026-11-08 10:00] x [P1] : un {#p}\n- [2026-11-08 10:01] y : deux {#p}\n",
        )
        .unwrap();
        scrute(&dir, &log);
        assert_eq!(compte(&journal, "deux"), 1);
        assert_eq!(compte(&journal, "P1"), 1); // legacy sans tag = P1
    }

    #[test]
    fn incomplet_retenu_puis_livre() {
        let (racine, dir) = bac("incomplet");
        fs::write(dir.boite(), "- [2026-11-08 10:00] x [P1] : amorce {#p}\n").unwrap();
        let journal = racine.join("events.jsonl");
        let log = EventLog::new(Some(journal.clone()));
        scrute(&dir, &log);
        fs::write(
            dir.boite(),
            "- [2026-11-08 10:00] x [P1] : amorce {#p}\n- [2026-11-08 10:01] y [P0] : moiti",
        )
        .unwrap();
        scrute(&dir, &log);
        assert_eq!(compte(&journal, "moiti"), 0);
        fs::write(dir.boite(), "- [2026-11-08 10:00] x [P1] : amorce {#p}\n- [2026-11-08 10:01] y [P0] : moitie {#p}\n")
            .unwrap();
        scrute(&dir, &log);
        assert_eq!(compte(&journal, "moitie"), 1);
    }

    #[test]
    fn remplacement_inode() {
        let (racine, dir) = bac("inode");
        let mut contenu = String::new();
        for i in 0..70 {
            contenu.push_str(&format!("- [2026-11-08 10:00] x [P1] : ligne {i} {{#p}}\n"));
        }
        fs::write(dir.boite(), &contenu).unwrap();
        let journal = racine.join("events.jsonl");
        let log = EventLog::new(Some(journal.clone()));
        scrute(&dir, &log);
        fs::write(racine.join(".mbox-sonde.inbox.offset"), "1:2:0").unwrap();
        fs::write(dir.tailles(), "1 rotate BOITE-USER.md moved=9 kept=70\n").unwrap();
        scrute(&dir, &log);
        assert_eq!(compte(&journal, "ligne"), 0); // rotation : reprise en fin
        fs::write(racine.join(".mbox-sonde.inbox.offset"), "1:2:0").unwrap();
        fs::write(dir.tailles(), "1 doctor ok\n").unwrap();
        scrute(&dir, &log);
        let n = compte(&journal, "ligne");
        assert!(n <= 64, "replay borné, vu {n}"); // hors rotation : borné
        assert!(compte(&journal, "voir bcast tail") >= 1);
    }

    #[test]
    fn troncation_signalee() {
        let (racine, dir) = bac("troncation");
        fs::write(dir.boite(), "0123456789abcdef0123456789abcdef0123456789\n").unwrap();
        let journal = racine.join("events.jsonl");
        let log = EventLog::new(Some(journal.clone()));
        scrute(&dir, &log);
        fs::write(dir.boite(), "court\n").unwrap();
        scrute(&dir, &log);
        let tailles = fs::read_to_string(dir.tailles()).unwrap();
        assert!(
            tailles.contains("shrink-unexpected BOITE-USER.md old=43 new=6"),
            "{tailles}"
        );
        assert_eq!(compte(&journal, "tronqué hors rotation"), 1);
    }

    #[test]
    fn ordres_actifs_seuls() {
        let (racine, dir) = bac("ordres");
        fs::write(dir.ordres(), "O-1|1|d|ALL|24|actif|s|vieux\n").unwrap();
        let journal = racine.join("events.jsonl");
        let log = EventLog::new(Some(journal.clone()));
        scrute(&dir, &log);
        fs::write(
            dir.ordres(),
            "O-1|1|d|ALL|24|actif|s|vieux\nO-2|2|d|ALL|24|actif|s|neuf\nO-3|3|d|ALL|24|revoque|s|mort\n",
        )
        .unwrap();
        scrute(&dir, &log);
        assert_eq!(compte(&journal, "O-2 : neuf"), 1);
        assert_eq!(compte(&journal, "mort"), 0);
    }

    #[test]
    fn hb_muet_marqueur() {
        use std::ffi::CString;

        let (racine, dir) = bac("hb");
        let hbd = dir.joindre(".hb");
        let vieux = hbd.join("sonde-x");
        fs::write(&vieux, "1 2026-11-01 00:00 vu\n").unwrap();
        let passe = epoque().saturating_sub(1000) as i64;
        let chemin = CString::new(vieux.to_str().unwrap()).unwrap();
        let t = [libc::timespec {
            tv_sec: passe,
            tv_nsec: 0,
        }; 2];
        unsafe {
            libc::utimensat(libc::AT_FDCWD, chemin.as_ptr(), t.as_ptr(), 0);
        }
        fs::write(hbd.join("pid-123"), "1 d vu\n").unwrap();
        fs::write(hbd.join("propre"), "1 d stop-propre\n").unwrap();
        fs::write(hbd.join("frais"), "1 d vu\n").unwrap();
        let journal = racine.join("events.jsonl");
        let log = EventLog::new(Some(journal.clone()));
        scrute(&dir, &log);
        assert_eq!(compte(&journal, "sonde muette"), 1);
        assert_eq!(compte(&journal, "pid-123"), 0);
        assert!(racine.join(".hb-alerted-sonde-x").is_file());
        scrute(&dir, &log);
        assert_eq!(compte(&journal, "sonde muette"), 1); // marqueur : silence
    }
}
