//! CLI `doctor` : santé inbox + bus + ordres + hb (extrait de `cli_gestion`).
//!
//! `cli_gestion.rs` dépassait 500 lignes après formatage : `cmd_doctor` et
//! ses trois contrôles vivent ici. Miroir de `cmd_doctor` (bcast:268-302) :
//! 2 si erreur, 1 si alerte, 0 sinon.

use std::path::PathBuf;

use super::fichier::{append_verrouille, epoque_fichier, hb_perime, lire_lignes, tail_n};
use super::format::{epoque, est_message, est_suite, BcastDir, Sortie, SUJETS_BUS};

/// Alerte quand l'inbox dépasse ce nombre de lignes (bcast:278).
const INBOX_ALERTE_LIGNES: usize = 2000;
/// Lignes récentes contrôlées par `doctor` (bcast:280).
const DOCTOR_FORMAT_N: usize = 200;

/// `doctor` : santé inbox + bus + ordres + hb (`cmd_doctor`, bcast:268-302).
/// 2 si erreur, 1 si alerte, 0 sinon. Les contrôles bus/ordres/hb-frais
/// étendent le shell (inbox + hb périmés + systemd seuls).
pub(crate) fn cmd_doctor(dir: &BcastDir, _args: &[String]) -> Sortie {
    if !dir.racine().is_dir() {
        println!("ERR dossier absent : {}", dir.racine().display());
        return Sortie::Usage; // `return 2` du shell (bcast:272).
    }
    // Best-effort de `[ -w ]` (root écrit malgré tout, accepté).
    let mut erreurs =
        u32::from(std::fs::metadata(dir.racine()).is_ok_and(|m| m.permissions().readonly()));
    if erreurs > 0 {
        println!("ERR dossier non inscriptible");
    }
    let alertes = verif_inbox(dir) + verif_canaux(dir) + verif_systemd(dir);
    match append_verrouille(&dir.tailles(), &[format!("{} doctor ok", epoque())]) {
        Ok(()) => println!("OK écriture verrouillée"),
        Err(_) => {
            println!("ERR écriture verrouillée impossible");
            erreurs += 1;
        }
    }
    if erreurs > 0 {
        Sortie::Usage // `err` du shell vaut 2 (bcast:299).
    } else if alertes > 0 {
        Sortie::ErreurExecution
    } else {
        Sortie::Ok
    }
}

/// Contrôle inbox : volume + format des lignes récentes. Rend les alertes.
fn verif_inbox(dir: &BcastDir) -> u32 {
    let lignes = lire_lignes(&dir.boite());
    let octets = std::fs::metadata(dir.boite()).map(|m| m.len()).unwrap_or(0);
    println!("OK inbox {} lignes {octets} octets", lignes.len());
    let mut alertes = u32::from(lignes.len() > INBOX_ALERTE_LIGNES);
    if alertes > 0 {
        println!("WARN inbox > 2000 lignes sur 7j : chasser les spammeurs (bcast tail, throttle/heartbeat)");
    }
    // Ligne valide : message avec contenu autour de ` : `, continuation,
    // ou vide stricte (regex du `grep -cvE`, bcast:280).
    let valide = |l: &String| {
        l.is_empty()
            || est_suite(l)
            || (est_message(l)
                && l.split_once(" : ")
                    .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty()))
    };
    let mauvaises = tail_n(&dir.boite(), DOCTOR_FORMAT_N)
        .iter()
        .filter(|l| !valide(l))
        .count();
    if mauvaises > 0 {
        println!("WARN {mauvaises} lignes hors format (200 dernières)");
        alertes += 1;
    } else {
        println!("OK format des 200 dernières lignes");
    }
    alertes
}

/// Contrôle bus + ordres + heartbeats. Rend les alertes.
fn verif_canaux(dir: &BcastDir) -> u32 {
    let mut alertes = 0;
    let presents = SUJETS_BUS.iter().filter(|s| dir.bus(s).is_file()).count();
    for sujet in SUJETS_BUS.iter().filter(|s| !dir.bus(s).is_file()) {
        println!("WARN bus {sujet} illisible");
        alertes += 1;
    }
    if presents == SUJETS_BUS.len() {
        println!("OK bus {} topics", SUJETS_BUS.len());
    }
    if dir.ordres().is_file() {
        let n = lire_lignes(&dir.ordres())
            .iter()
            .filter(|l| !l.trim().is_empty())
            .count();
        println!("OK ordres {n} ligne(s)");
    } else {
        println!("WARN ORDRES.log illisible");
        alertes += 1;
    }
    let now = epoque();
    let Ok(entrees) = std::fs::read_dir(dir.joindre(".hb")) else {
        return alertes;
    };
    let mut chemins: Vec<PathBuf> = entrees
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    chemins.sort();
    let mut frais = 0;
    for chemin in chemins {
        let nom = chemin.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        if hb_perime(&chemin, now) {
            let age = epoque_fichier(&chemin)
                .map(|m| now.saturating_sub(m))
                .unwrap_or(0);
            println!("WARN heartbeat périmé : {nom} ({age}s)");
            alertes += 1;
        } else {
            frais += 1;
        }
    }
    if frais > 0 {
        println!("OK hb {frais} frais");
    }
    alertes
}

/// Contrôle systemd, seulement sur le dossier mono-user (bcast:290-295).
fn verif_systemd(dir: &BcastDir) -> u32 {
    if dir.boite() != BcastDir::defaut().boite() {
        return 0;
    }
    let mut alertes = 0;
    for svc in ["broadcast-watch.service", "agents-watch.service"] {
        match std::process::Command::new("systemctl")
            .args(["--user", "is-active", "-q", svc])
            .status()
        {
            Err(_) => break, // `systemctl` absent : on saute (`command -v`).
            Ok(st) if !st.success() => {
                println!("WARN {svc} inactif");
                alertes += 1;
            }
            Ok(_) => {}
        }
    }
    alertes
}
