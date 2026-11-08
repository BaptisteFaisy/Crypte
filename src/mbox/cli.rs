//! CLI Rust du broadcast v2, miroir de `bcast` shell (lot cli, base).
//!
//! Expose `exec(args, dir) -> i32` que l'assembleur branchera dans `main.rs`
//! (parsing existant intouché) : `args[0]` = sous-commande. Ce fichier porte
//! `send`, `bus`, `tail`, `digest`, `hb`, `version`, `help` + le découpage
//! d'options et la porte à jeton partagés ; `cli_gestion.rs` porte `rotate`,
//! `ordre` et `doctor` (périmètre trop large pour un seul fichier < 500 L).
//! Mêmes options, mêmes sorties (`sent`, `throttled:…`, `same-state`,
//! `digests=`, `ok`), mêmes codes (0 ok, 1 exécution, 2 usage).
//!
//! Signatures partagées utilisées (tranche assembleur) :
//! - `format::{BcastDir}` : `depuis`, `defaut`, `racine`, `joindre`, `init`,
//!   `boite`, `bus`, `hb` ; `format::{epoque, maintenant, construire_ligne,
//!   decouper_corps, normalise_emetteur, Priorite, Sortie, SUJETS_BUS}`.
//! - `fichier::{append_verrouille, lire_hb, tail_n}` (+ `lire_lignes`, tests).
//! - `throttle::{garde_inbox, garde_bus, etat_change}` +
//!   `MotifCompteur::verdict` (`dup`, `rate`) ; `digest::digest_flush`.
//! - `journal::lister` (pont assembleur) pour `tail --all`.
//! - `outils::{decoupage, opt, expediteur, porte_jeton, usg}` (découpage,
//!   identité, porte `--token`, partagés avec `cli_gestion` et `jeton`).
//! - `cli_gestion::{cmd_rotate, cmd_ordre}` + `docteur::cmd_doctor` + `jeton::cmd_jeton`.
//!
//! `--token "nom:secret"` exigé pour `send --force` (contournement de
//! garde = sensible). Le jeton n'est jamais affiché ni journalisé.

use std::path::{Path, PathBuf};

use super::cli_gestion::{cmd_ordre, cmd_rotate};
use super::digest::digest_flush;
use super::docteur::cmd_doctor;
use super::fichier::{append_verrouille, lire_hb, tail_n};
use super::format::{
    construire_ligne, decouper_corps, epoque, maintenant, BcastDir, Priorite, Sortie, SUJETS_BUS,
};
use super::jeton::cmd_jeton;
use super::journal::lister;
use super::outils::{decoupage, expediteur, opt, porte_jeton, usg};
use super::throttle::{etat_change, garde_bus, garde_inbox};

/// Version affichée (`VERSION`, bcast:16).
const VERSION: &str = "2.0.0";

/// Texte de `help` (miroir de `cmd_help`, bcast:25-43 + lignes Rust).
const AIDE: &str = "bcast 2.0 — broadcast user + bus inter-agents
  bcast send [-p P0|P1|P2] [--from ID] [--dest @x]... [--state k=v] [--force] -- <msg>
  bcast bus -t phone|gpu|file|divers [--from ID] -- <msg>
  bcast tail [-n N] [--bus t] [--all] | bcast digest | bcast rotate [--keep-days 7]
  bcast hb <nom> <état...> | bcast hb list | bcast ordre new|list|revoque|ack|pull | bcast doctor
  bcast jeton new [--ttl H] <nom> | bcast jeton revoque <nom> | bcast jeton list | bcast jeton purge
Inbox : P0 popup+son, P1 popup, P2 log seul. Throttle : dedup 30 min, 6 msg/h.
Sorties : 0 ok (throttled:/same-state inclus), 1 exécution, 2 usage, 3 déjà-en-cours.
Écritures sensibles (--force, ordre new/revoque/ack) : --token \"nom:secret\" (voir jeton new).";

/// Point d'entrée CLI : exécute `args` sur `dir`, rend le code shell.
/// Sans argument : aide (`cmd="${1:-help}"`, bcast:305).
pub fn exec(args: &[String], dir: &Path) -> i32 {
    let dir = BcastDir::depuis(dir.to_path_buf());
    let (nom, reste) = match args.split_first() {
        Some((n, r)) => (n.as_str(), r),
        None => ("help", &[][..]),
    };
    match nom {
        "send" => cmd_send(&dir, reste),
        "bus" => cmd_bus(&dir, reste),
        "tail" => cmd_tail(&dir, reste),
        "rotate" => cmd_rotate(&dir, reste),
        "digest" => {
            let _ = dir.init();
            println!("digests={}", digest_flush(&dir));
            Sortie::Ok
        }
        "hb" => cmd_hb(&dir, reste),
        "ordre" => cmd_ordre(&dir, reste),
        "doctor" => cmd_doctor(&dir, reste),
        "jeton" => cmd_jeton(&dir, reste),
        "version" => {
            println!("bcast {VERSION}");
            Sortie::Ok
        }
        "help" | "--help" | "-h" => {
            println!("{AIDE}");
            Sortie::Ok
        }
        inconnu => usg(&format!("commande inconnue : {inconnu} (bcast help)")),
    }
    .code()
}

/// Point d'entrée sur le dossier défaut (`BCAST_DIR` ou défaut shell),
/// branché par `main.rs` (`--mbox`, dossier jamais écrit par les tests).
pub fn exec_defaut(args: &[String]) -> i32 {
    let dir = BcastDir::defaut();
    exec(args, dir.racine())
}

/// Vrai si un drapeau sans valeur est présent (`--force`, `--all`).
fn flag(opts: &[(String, String)], nom: &str) -> bool {
    opts.iter().any(|(n, _)| n == nom)
}

/// Assemble la ligne v2 + continuations (`wrap_body` + `build_line`).
fn enveloppe(exp: &str, pri: Priorite, dest: &str, corps: &str) -> Vec<String> {
    let lignes = decouper_corps(corps);
    let premiere = lignes.first().cloned().unwrap_or_default();
    let mut sortie = vec![construire_ligne(exp, pri, dest, &premiere)];
    sortie.extend(lignes.into_iter().skip(1));
    sortie
}

/// Options de `send` (`-p`, `--from`, `--dest`, `--state`, `--force`, `--token`).
const SPECS_SEND: [(&str, bool); 6] = [
    ("p", true),
    ("from", true),
    ("dest", true),
    ("state", true),
    ("force", false),
    ("token", true),
];

/// Résout `-p` (défaut P1) ou échec d'usage.
fn resout_priorite(brut: Option<&str>) -> Result<Priorite, Sortie> {
    match brut.unwrap_or("P1") {
        "P0" => Ok(Priorite::P0),
        "P1" => Ok(Priorite::P1),
        "P2" => Ok(Priorite::P2),
        _ => Err(usg("priorité P0|P1|P2")),
    }
}

/// Assemble `--dest` répétés + marqueur de contournement (`--force`).
fn assemble_dest(opts: &[(String, String)], force: bool) -> String {
    let mut dest = opts
        .iter()
        .filter(|(n, _)| n == "dest")
        .map(|(_, v)| v.clone())
        .collect::<Vec<_>>()
        .join(" ");
    if force {
        dest = format!("{dest} [bypass-throttle]").trim_start().to_string();
    }
    dest
}

/// Évalue `--state` (`same-state` si inchangé) : `Ok(saute_dedup)` pour
/// continuer, `Err(sortie)` déjà affichée sinon (bcast:68-79).
fn passe_etat(dir: &BcastDir, exp: &str, brut: Option<&str>) -> Result<bool, Sortie> {
    let Some(s) = brut else { return Ok(false) };
    match s.split_once('=') {
        None => Err(usg("--state attend cle=valeur")),
        Some((cle, valeur)) if etat_change(dir, exp, cle, valeur) => Ok(true),
        Some(_) => {
            println!("same-state");
            Err(Sortie::Ok)
        }
    }
}

/// `send` : écrit dans l'inbox user (`cmd_send`, bcast:46-87).
fn cmd_send(dir: &BcastDir, args: &[String]) -> Sortie {
    let Ok((opts, msg)) = decoupage(args, &SPECS_SEND) else {
        return Sortie::Usage;
    };
    if msg.is_empty() {
        return usg("message vide");
    }
    let pri = match resout_priorite(opt(&opts, "p")) {
        Ok(pri) => pri,
        Err(sortie) => return sortie,
    };
    let Some(exp) = expediteur(opt(&opts, "from")) else {
        return Sortie::Usage;
    };
    let corps = msg.join(" ");
    let force = flag(&opts, "force");
    if force && !porte_jeton(&corps, opt(&opts, "token")) {
        return Sortie::ErreurExecution;
    }
    let dest = assemble_dest(&opts, force);
    let _ = dir.init();
    let saute_dedup = match passe_etat(dir, &exp, opt(&opts, "state")) {
        Ok(saute) => saute,
        Err(sortie) => return sortie,
    };
    if !force {
        if let Err(motif) = garde_inbox(dir, &exp, &corps, saute_dedup) {
            println!("throttled:{}", motif.verdict());
            return Sortie::Ok;
        }
    }
    match append_verrouille(&dir.boite(), &enveloppe(&exp, pri, &dest, &corps)) {
        Ok(()) => {
            println!("sent");
            Sortie::Ok
        }
        Err(_) => usg("écriture impossible"), // `|| die` du shell (bcast:85).
    }
}

/// `bus` : écrit sur un sujet sans popup (`cmd_bus`, bcast:90-114).
fn cmd_bus(dir: &BcastDir, args: &[String]) -> Sortie {
    let Ok((opts, msg)) = decoupage(args, &[("t", true), ("from", true)]) else {
        return Sortie::Usage;
    };
    let Some(sujet) = opt(&opts, "t").filter(|s| SUJETS_BUS.contains(s)) else {
        return usg(&format!("topic : {}", SUJETS_BUS.join(" ")));
    };
    if msg.is_empty() {
        return usg("message vide");
    }
    let Some(exp) = expediteur(opt(&opts, "from")) else {
        return Sortie::Usage;
    };
    let _ = dir.init();
    let corps = msg.join(" ");
    if !garde_bus(dir, &exp, sujet, &corps) {
        println!("throttled:dup");
        return Sortie::Ok;
    }
    match append_verrouille(
        &dir.bus(sujet),
        &enveloppe(&exp, Priorite::P2, &format!("@{sujet}"), &corps),
    ) {
        Ok(()) => {
            println!("sent");
            Sortie::Ok
        }
        Err(_) => usg("écriture impossible"), // `|| die` du shell (bcast:112).
    }
}

/// `tail` : lit l'inbox, un sujet ou tout (`cmd_tail`, bcast:117-139).
/// Fichiers absents = silence (redirections `|| true` du shell).
fn cmd_tail(dir: &BcastDir, args: &[String]) -> Sortie {
    let Ok((opts, _)) = decoupage(args, &[("n", true), ("bus", true), ("all", false)]) else {
        return Sortie::Usage;
    };
    let n: usize = match opt(&opts, "n").map(str::parse) {
        None => 20,
        Some(Ok(v)) => v,
        Some(Err(_)) => return usg("-n attend un nombre"),
    };
    if let Some(sujet) = opt(&opts, "bus") {
        if !SUJETS_BUS.contains(&sujet) {
            return usg(&format!("topic : {}", SUJETS_BUS.join(" ")));
        }
        tail_n(&dir.bus(sujet), n)
            .iter()
            .for_each(|l| println!("{l}"));
        return Sortie::Ok;
    }
    if flag(&opts, "all") {
        println!("=== inbox (user) ===");
        tail_n(&dir.boite(), n).iter().for_each(|l| println!("{l}"));
        for sujet in SUJETS_BUS {
            println!("=== bus/{sujet} ===");
            tail_n(&dir.bus(sujet), n)
                .iter()
                .for_each(|l| println!("{l}"));
        }
        println!("=== ordres actifs ===");
        print!("{}", lister(dir, false));
        return Sortie::Ok;
    }
    tail_n(&dir.boite(), n).iter().for_each(|l| println!("{l}"));
    Sortie::Ok
}

/// Liste les heartbeats (`nom age=…s <première ligne>`, bcast:201-211).
/// Illisibles et futurs sautés, comme le shell.
fn liste_hb(dir: &BcastDir) {
    let now = epoque();
    let Ok(entrees) = std::fs::read_dir(dir.joindre(".hb")) else {
        return;
    };
    let mut chemins: Vec<PathBuf> = entrees
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    chemins.sort();
    for chemin in chemins {
        let mtime = std::fs::metadata(&chemin)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        let Some(m) = mtime.filter(|m| *m <= now) else {
            continue;
        }; // Illisible/futur : sauté.
        let nom = chemin.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        println!(
            "{nom} age={}s {}",
            now - m,
            lire_hb(&chemin).unwrap_or_default()
        );
    }
}

/// `hb` : écrit ou liste les heartbeats (`cmd_hb`, bcast:196-218).
fn cmd_hb(dir: &BcastDir, args: &[String]) -> Sortie {
    let [nom, etat @ ..] = args else {
        return usg("usage : bcast hb <nom> <état...> | bcast hb list");
    };
    if nom == "list" {
        liste_hb(dir);
        return Sortie::Ok;
    }
    if etat.is_empty() {
        return usg("usage : bcast hb <nom> <état...>");
    }
    let _ = dir.init();
    // Écriture directe sans verrou, comme le shell (bcast:216).
    let contenu = format!("{} {} {}\n", epoque(), maintenant(), etat.join(" "));
    match std::fs::write(dir.hb(nom), contenu) {
        Ok(()) => {
            println!("ok");
            Sortie::Ok
        }
        Err(_) => {
            eprintln!("bcast: écriture heartbeat impossible");
            Sortie::ErreurExecution // Redirection `>` manquée = 1, pas `die`.
        }
    }
}

// ---------------------------------------------------------------------------
// Tests unitaires intra-fichier (convention Crypte).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::fichier::lire_lignes;
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COMPTEUR: AtomicU64 = AtomicU64::new(0);

    /// Dossier broadcast jetable unique (jamais le vrai `.scratch`).
    fn dir_temp() -> PathBuf {
        let n = COMPTEUR.fetch_add(1, Ordering::SeqCst);
        let racine = std::env::temp_dir().join(format!("mbox-cli-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&racine).expect("dossier de test");
        racine
    }

    /// Exécute `mots` sur `racine`, rend le code (`argv` pratique).
    fn lance(racine: &Path, mots: &[&str]) -> i32 {
        exec(
            &mots.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            racine,
        )
    }

    #[test]
    fn usages() {
        let racine = dir_temp();
        let cas: &[(&[&str], i32)] = &[
            (&[], 0),
            (&["version"], 0),
            (&["help"], 0),
            (&["nope"], 2),
            (&["send", "--from", "t"], 2),
            (&["send", "-p", "P9", "--from", "t", "--", "x"], 2),
            (&["send", "--from", "", "--", "x"], 2),
            (&["send", "--from", "t", "--state", "k", "--", "x"], 2),
            (&["bus", "--from", "t", "--", "x"], 2),
            (&["bus", "-t", "nope", "--from", "t", "--", "x"], 2),
            (&["tail", "-n", "xx"], 2),
            (&["tail", "--bus", "nope"], 2),
            (&["hb"], 2),
            (&["hb", "seul"], 2),
            (&["send", "--from", "t", "--force", "--", "x"], 1),
            (&["ordre", "new", "--token", "malformé", "--", "x"], 1),
        ];
        for (mots, code) in cas {
            assert_eq!(lance(&racine, mots), *code, "cas {mots:?}");
        }
        let _ = std::fs::remove_dir_all(&racine);
    }

    #[test]
    fn envois_dedup_et_etat() {
        let racine = dir_temp();
        let dir = BcastDir::depuis(racine.clone());
        assert_eq!(
            lance(&racine, &["send", "--from", "t-cli", "--", "bonjour"]),
            0
        );
        let boite = std::fs::read_to_string(dir.boite()).expect("inbox écrite");
        assert!(
            boite.contains("[P1]") && boite.contains(" : bonjour "),
            "ligne : {boite}"
        );
        assert_eq!(
            lance(&racine, &["send", "--from", "t-cli", "--", "bonjour"]),
            0
        );
        assert_eq!(lire_lignes(&dir.boite()).len(), 1); // dedup 30 min, code 0
        assert_eq!(
            lance(
                &racine,
                &["send", "--from", "t-e", "--state", "m=v1", "--", "un"]
            ),
            0
        );
        assert_eq!(
            lance(
                &racine,
                &["send", "--from", "t-e", "--state", "m=v1", "--", "un"]
            ),
            0
        );
        assert_eq!(lire_lignes(&dir.boite()).len(), 2); // same-state : pas de ligne
        let _ = std::fs::remove_dir_all(&racine);
    }

    #[test]
    fn bus_ecrit_et_dedup() {
        let racine = dir_temp();
        let dir = BcastDir::depuis(racine.clone());
        assert_eq!(
            lance(
                &racine,
                &["bus", "-t", "phone", "--from", "t-b", "--", "prise"]
            ),
            0
        );
        let bus = std::fs::read_to_string(dir.bus("phone")).expect("bus écrit");
        assert!(
            bus.contains("@phone") && bus.contains("[P2]"),
            "bus : {bus}"
        );
        assert_eq!(
            lance(
                &racine,
                &["bus", "-t", "phone", "--from", "t-b", "--", "prise"]
            ),
            0
        );
        assert_eq!(lire_lignes(&dir.bus("phone")).len(), 1); // dedup bus 5 min
        let _ = std::fs::remove_dir_all(&racine);
    }

    #[test]
    fn lecture_et_hb() {
        let racine = dir_temp();
        let dir = BcastDir::depuis(racine.clone());
        assert_eq!(
            lance(&racine, &["send", "--from", "t-l", "--", "à relire"]),
            0
        );
        assert_eq!(
            lance(
                &racine,
                &["bus", "-t", "gpu", "--from", "t-l", "--", "chaud"]
            ),
            0
        );
        for mots in [
            &["tail"][..],
            &["tail", "-n", "5"],
            &["tail", "--bus", "gpu"],
            &["tail", "--all"],
            &["digest"],
        ] {
            assert_eq!(lance(&racine, mots), 0, "cas {mots:?}");
        }
        assert_eq!(lance(&racine, &["hb", "sonde-t", "vu", "mode=ok"]), 0);
        assert!(dir.hb("sonde-t").is_file());
        assert_eq!(lance(&racine, &["hb", "list"]), 0);
        let _ = std::fs::remove_dir_all(&racine);
    }
}
