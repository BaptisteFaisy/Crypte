//! CLI Rust du broadcast v2 (lot cli, gestion : `rotate`, `ordre`).
//!
//! Compagnon de `cli.rs` : `doctor` vit dans `docteur.rs`, les utilitaires
//! dans `outils.rs` (tranche assembleur : chaque fichier < 500 lignes).
//!
//! Signatures partagées utilisées (rappel assembleur) :
//! - `format::{BcastDir}` : `depuis`, `defaut`, `racine`, `joindre`, `init`,
//!   `boite`, `bus`, `ordres`, `tailles` ; `format::{epoque, analyser_ttl,
//!   est_message, Sortie, SUJETS_BUS, TTL_DEFAUT_HEURES}`.
//! - `outils::{decoupage, opt, usg, expediteur, porte_jeton}` (`pub(crate)`).
//! - `fichier::{epoque_fichier, lire_lignes, tail_n}`,
//!   `verrou::{avec_verrou, chemin_verrou}` (`doctor` vit dans `docteur`).
//! - `digest::digest_flush` (vidé avant chaque rotation, bcast:178).
//! - `journal::{creer, revoquer, ack, lister, tirer}` (pont assembleur,
//!   calqué sur `ordre_new`/`ordre_revoque`/`ordre_ack`/`ordre_list` shell ;
//!   `revoquer`/`ack` prennent le jeton brut en dernier paramètre et le
//!   revérifient au niveau journal) :
//!   `creer(&BcastDir, &str, u64, &str, &str) -> Result<String, Sortie>`,
//!   `revoquer(&BcastDir, &str, &str, &str) -> Result<(), Sortie>`,
//!   `ack(&BcastDir, &str, &str, &str, &str, &str) -> Result<(), Sortie>`,
//!   `lister(&BcastDir, bool) -> String`, `tirer(&BcastDir, &str) -> String`.
//!
//! `--token "nom:secret"` exigé pour `ordre new`, `revoque` et `ack`
//! (tranche assembleur : le shell n'a pas de jetons, la porte Rust les
//! impose sur les trois écritures sensibles des ordres).

use std::path::Path;

use super::digest::digest_flush;
use super::fichier::{epoque_fichier, lire_lignes, tail_n};
use super::format::{
    analyser_ttl, epoque, est_message, BcastDir, Sortie, SUJETS_BUS, TTL_DEFAUT_HEURES,
};
use super::journal::{ack, creer, lister, revoquer, tirer};
use super::outils::{decoupage, expediteur, opt, porte_jeton, usg};
use super::verrou::{avec_verrou, chemin_verrou};

/// Attente du verrou de rotation, secondes (`flock -w 30`, bcast:148).
const ATTENTE_ROTATE_SECS: u64 = 30;
/// Conservation des archives avant compression (`-mtime +30`, bcast:182).
const SEUIL_GZIP_JOURS: u64 = 30;
/// Conservation des états throttle (`-mtime +2`, bcast:186).
const SEUIL_ETAT_JOURS: u64 = 2;
/// Lignes conservées dans `.sizes` (`tail -n 500`, bcast:184).
const TAILLES_MAX: usize = 500;

/// Vrai si `mtime` a plus de `seuil_j` jours (`find -mtime +N`, bcast:182-186).
fn est_perime(mtime_epoch: u64, now: u64, seuil_j: u64) -> bool {
    now.saturating_sub(mtime_epoch) / 86_400 > seuil_j
}

/// `rotate` : archive, compresse, tronque, purge (`cmd_rotate`, bcast:170-188).
/// Échecs verrou/entrées ignorés (shell sans `set -e`) ; toujours 0.
pub(crate) fn cmd_rotate(dir: &BcastDir, args: &[String]) -> Sortie {
    let Ok((opts, _)) = decoupage(args, &[("keep-days", true)]) else {
        return Sortie::Usage;
    };
    let keep: u64 = match opt(&opts, "keep-days").map(str::parse) {
        None => 7,
        Some(Ok(v)) => v,
        Some(Err(_)) => return usg("--keep-days attend un nombre"),
    };
    let _ = dir.init();
    let _ = digest_flush(dir);
    let cutoff = (chrono::Local::now() - chrono::Duration::days(keep as i64))
        .format("%Y-%m-%d")
        .to_string();
    rotate_un(dir, &dir.boite(), "BOITE", &cutoff);
    for sujet in SUJETS_BUS {
        rotate_un(dir, &dir.bus(sujet), &format!("bus-{sujet}"), &cutoff);
    }
    balaye_vieux(dir);
    let tailles = dir.tailles();
    if tailles.is_file() {
        // `tail -n 500` + `mv` sous `flock -w 10` (bcast:183-185).
        let _ = avec_verrou(&chemin_verrou(&tailles), 10, || {
            let lignes = tail_n(&tailles, TAILLES_MAX);
            let fin = if lignes.is_empty() { "" } else { "\n" };
            std::fs::write(&tailles, lignes.join("\n") + fin)
        });
    }
    println!("rotated keep={keep} cutoff={cutoff}");
    Sortie::Ok
}

/// Date `AAAA-MM-JJ` valide (`[[ $dt =~ ^…$ ]]`, bcast:154).
fn date_valide(d: &str) -> bool {
    d.len() == 10
        && d.as_bytes()[4] == b'-'
        && d.as_bytes()[7] == b'-'
        && d.bytes()
            .enumerate()
            .all(|(k, b)| k == 4 || k == 7 || b.is_ascii_digit())
}

/// Ajoute une ligne à `archive/<prefixe>-<date>.md` (bcast:157).
fn ajoute_archive(dir: &BcastDir, prefixe: &str, cur: &str, ligne: &str) -> std::io::Result<()> {
    use std::io::Write;
    let archive = dir.joindre(&format!("archive/{prefixe}-{cur}.md"));
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&archive)?;
    writeln!(f, "{ligne}")
}

/// Note brute dans `.sizes`, sans flock comme le shell (bcast:167).
fn note_rotation(dir: &BcastDir, cible: &Path, deplace: u64, garde: u64) {
    use std::io::Write;
    let nom = cible
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ligne = format!("{} rotate {nom} moved={deplace} kept={garde}\n", epoque());
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.tailles())
        .and_then(|mut f| f.write_all(ligne.as_bytes()));
}

/// Archive les lignes avant `cutoff` vers `archive/<prefixe>-<date>.md`
/// (`rotate_one`, bcast:144-168). Les continuations suivent leur parent.
/// Comparaison de dates LEXICOGRAPHIQUE comme le shell (`[[ $cur < $cut ]]`).
fn rotate_un(dir: &BcastDir, cible: &Path, prefixe: &str, cutoff: &str) {
    if !cible.is_file() {
        return; // Absent : succès silencieux, sans ligne `.sizes`.
    }
    let issue = avec_verrou(&chemin_verrou(cible), ATTENTE_ROTATE_SECS, || {
        let mut gardees = String::new();
        let (mut deplace, mut garde, mut cur) = (0u64, 0u64, String::new());
        for ligne in lire_lignes(cible) {
            if est_message(&ligne) {
                // Date `${line:3:10}` validée `AAAA-MM-JJ` (bcast:153-154).
                if let Some(dt) = ligne.get(3..13).filter(|d| date_valide(d)) {
                    cur = dt.to_string();
                }
            }
            if !cur.is_empty() && cur.as_str() < cutoff {
                ajoute_archive(dir, prefixe, &cur, &ligne)?;
                deplace += 1;
            } else {
                gardees.push_str(&ligne);
                gardees.push('\n');
                garde += 1;
            }
        }
        let tmp = cible.with_extension(format!("rot.{}", std::process::id()));
        std::fs::write(&tmp, &gardees)?;
        std::fs::rename(&tmp, cible)?;
        std::io::Result::Ok((deplace, garde))
    });
    if let Ok((deplace, garde)) = issue {
        note_rotation(dir, cible, deplace, garde);
    }
}

/// Compresse les archives > 30 j (`gzip -f`, bcast:182) et purge les états
/// throttle > 2 j (`*.dedup`, `*.ratel` seuls, bcast:186). Best-effort.
fn balaye_vieux(dir: &BcastDir) {
    let now = epoque();
    if let Ok(entrees) = std::fs::read_dir(dir.joindre("archive")) {
        for chemin in entrees.flatten().map(|e| e.path()).filter(|p| p.is_file()) {
            let md = chemin.extension().and_then(|e| e.to_str()) == Some("md");
            if md && epoque_fichier(&chemin).is_some_and(|m| est_perime(m, now, SEUIL_GZIP_JOURS)) {
                let _ = std::process::Command::new("gzip")
                    .arg("-f")
                    .arg(&chemin)
                    .status();
            }
        }
    }
    if let Ok(entrees) = std::fs::read_dir(dir.joindre(".state")) {
        for chemin in entrees.flatten().map(|e| e.path()).filter(|p| p.is_file()) {
            let nom = chemin.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let cible = nom.ends_with(".dedup") || nom.ends_with(".ratel");
            if cible
                && epoque_fichier(&chemin).is_some_and(|m| est_perime(m, now, SEUIL_ETAT_JOURS))
            {
                let _ = std::fs::remove_file(&chemin);
            }
        }
    }
}

/// `ordre` : répartit vers new/list/revoque/ack/pull (`cmd_ordre`, bcast:221-265).
pub(crate) fn cmd_ordre(dir: &BcastDir, args: &[String]) -> Sortie {
    match args.first().map(String::as_str) {
        Some("new") => ordre_nouveau(dir, &args[1..]),
        Some("list") => {
            // Seul `--all` en $1 compte, le reste est ignoré (bcast:243-244).
            print!("{}", lister(dir, args.get(1).is_some_and(|a| a == "--all")));
            Sortie::Ok
        }
        Some("revoque") => ordre_revoque(dir, &args[1..]),
        Some("ack") => ordre_accuse(dir, &args[1..]),
        Some("pull") => ordre_tire(dir, &args[1..]),
        _ => usg("ordre : new|list|revoque|ack|pull"),
    }
}

/// `ordre new` : crée un ordre (`ordre_new`, lib-ordres.sh:41-56 + bcast:224-241).
fn ordre_nouveau(dir: &BcastDir, args: &[String]) -> Sortie {
    let specs = [("to", true), ("ttl", true), ("src", true), ("token", true)];
    let Ok((opts, msg)) = decoupage(args, &specs) else {
        return Sortie::Usage;
    };
    if msg.is_empty() {
        return usg("texte d'ordre vide");
    }
    let texte = msg.join(" ");
    let Ok(ttl_h) = opt(&opts, "ttl")
        .map(analyser_ttl)
        .unwrap_or(Ok(TTL_DEFAUT_HEURES))
    else {
        return usg("ttl invalide (heures, 0=permanent)"); // `|| exit 2` (bcast:239).
    };
    if !porte_jeton(&texte, opt(&opts, "token")) {
        return Sortie::ErreurExecution;
    }
    let _ = dir.init();
    match creer(
        dir,
        opt(&opts, "to").unwrap_or("ALL"),
        ttl_h,
        opt(&opts, "src").unwrap_or("user-direct"),
        &texte,
    ) {
        Ok(id) => {
            println!("ordre={id}");
            Sortie::Ok
        }
        Err(_) => Sortie::Usage, // Tout échec de `new` vaut 2 (bcast:239).
    }
}

/// Extrait `--token`/`--from` mêlés aux positionnels (`revoque`, `ack`).
fn extrait_option(args: &[String], nom: &str) -> Result<(Option<String>, Vec<String>), Sortie> {
    let (mut valeur, mut reste) = (None, Vec::new());
    let mut i = 0;
    while i < args.len() {
        if args[i] == format!("--{nom}") {
            let Some(v) = args.get(i + 1) else {
                return Err(usg(&format!("option --{nom} attend une valeur")));
            };
            valeur = Some(v.clone());
            i += 2;
        } else {
            reste.push(args[i].clone());
            i += 1;
        }
    }
    Ok((valeur, reste))
}

/// `ordre revoque` : révoque un ordre actif (bcast:247-251 + lib-ordres.sh:70-88).
fn ordre_revoque(dir: &BcastDir, args: &[String]) -> Sortie {
    let Some(id) = args.first() else {
        return usg("usage : bcast ordre revoque <id> [motif]");
    };
    let Ok((token, reste)) = extrait_option(&args[1..].to_vec(), "token") else {
        return Sortie::Usage;
    };
    let motif = reste.join(" ");
    if !porte_jeton(&format!("{id} {motif}"), token.as_deref()) {
        return Sortie::ErreurExecution;
    }
    match revoquer(dir, id, &motif, token.as_deref().unwrap_or("")) {
        Ok(()) => {
            println!("revoque={id}");
            Sortie::Ok
        }
        Err(s) => s,
    }
}

/// `ordre ack` : accuse réception (bcast:252-262 + lib-ordres.sh:91-99).
/// Extension Rust : `--token` exigé (comme `new`/`revoque`), revérifié
/// au niveau journal.
fn ordre_accuse(dir: &BcastDir, args: &[String]) -> Sortie {
    if args.len() < 2 {
        return usg(
            "usage : bcast ordre ack <id> <lu|applique|refuse> [motif] [--from ID] [--token T]",
        );
    }
    let Ok((from, reste)) = extrait_option(&args[2..].to_vec(), "from") else {
        return Sortie::Usage;
    };
    let Ok((token, reste)) = extrait_option(&reste, "token") else {
        return Sortie::Usage;
    };
    let Some(exp) = expediteur(from.as_deref()) else {
        return Sortie::Usage;
    };
    let motif = reste.join(" ");
    if !porte_jeton(
        &format!("{} {} {motif}", args[0], args[1]),
        token.as_deref(),
    ) {
        return Sortie::ErreurExecution;
    }
    match ack(
        dir,
        &args[0],
        &exp,
        &args[1],
        &motif,
        token.as_deref().unwrap_or(""),
    ) {
        Ok(()) => {
            println!("ack={} {}", args[0], args[1]); // Verdict tapé (bcast:261).
            Sortie::Ok
        }
        Err(s) => s,
    }
}

/// `ordre pull` : file d'un agent (extension Rust, sans équivalent shell).
fn ordre_tire(dir: &BcastDir, args: &[String]) -> Sortie {
    let Some(agent) = args.first() else {
        return usg("usage : bcast ordre pull <agent>");
    };
    print!("{}", tirer(dir, agent));
    Sortie::Ok
}

// ---------------------------------------------------------------------------
// Tests unitaires intra-fichier (convention Crypte).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::cli::exec;
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COMPTEUR: AtomicU64 = AtomicU64::new(0);

    /// Dossier broadcast jetable unique (jamais le vrai `.scratch`).
    fn dir_temp() -> PathBuf {
        let n = COMPTEUR.fetch_add(1, Ordering::SeqCst);
        let racine = std::env::temp_dir().join(format!("mbox-gest-{n}-{}", std::process::id()));
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
    fn usages_gestion() {
        let racine = dir_temp();
        let cas: &[(&[&str], i32)] = &[
            (&["rotate", "--keep-days", "xx"], 2),
            (&["rotate", "--nope"], 2),
            (&["ordre"], 2),
            (&["ordre", "nope"], 2),
            (&["ordre", "ack", "O-1"], 2),
            (&["ordre", "revoque"], 2),
            (&["ordre", "new", "--ttl", "xx", "--", "x"], 2),
            (&["ordre", "new", "--", "texte"], 1),
            (&["ordre", "revoque", "O-1"], 1),
            (&["ordre", "ack", "O-9", "lu", "--from", "t"], 1),
            (&["ordre", "pull"], 2),
            (&["ordre", "pull", "alice"], 0),
        ];
        for (mots, code) in cas {
            assert_eq!(lance(&racine, mots), *code, "cas {mots:?}");
        }
        let _ = std::fs::remove_dir_all(&racine);
    }

    #[test]
    fn rotation_archive_et_seuils() {
        let racine = dir_temp();
        let dir = BcastDir::depuis(racine.clone());
        dir.init().expect("init");
        let auj = chrono::Local::now().format("%Y-%m-%d").to_string();
        let contenu = format!("- [2020-01-01 10:00] vieux [P1] : archivé moi\n  suite du vieux\n- [{auj} 10:00] frais [P1] : gardé moi\n");
        std::fs::write(dir.boite(), contenu).expect("inbox de test");
        assert_eq!(lance(&racine, &["rotate", "--keep-days", "7"]), 0);
        let archive =
            std::fs::read_to_string(dir.joindre("archive/BOITE-2020-01-01.md")).expect("archive");
        assert!(
            archive.contains("archivé moi") && archive.contains("suite du vieux"),
            "parent+suite : {archive}"
        );
        let gardee = std::fs::read_to_string(dir.boite()).expect("boîte gardée");
        assert!(
            gardee.contains("gardé moi") && !gardee.contains("archivé"),
            "tri : {gardee}"
        );
        let tailles = std::fs::read_to_string(dir.tailles()).expect(".sizes");
        assert!(
            tailles.contains("rotate BOITE-USER.md moved=2 kept=1"),
            "journal : {tailles}"
        );
        assert!(est_perime(100, 100 + 31 * 86_400, 30) && !est_perime(100, 100 + 30 * 86_400, 30));
        assert!(est_perime(100, 100 + 3 * 86_400, 2) && !est_perime(200, 100, 2));
        let _ = std::fs::remove_dir_all(&racine);
    }

    #[test]
    fn cycle_ordres() {
        let racine = dir_temp();
        let dir = BcastDir::depuis(racine.clone());
        assert_eq!(lance(&racine, &["ordre", "list"]), 0);
        // Le stub d'ordres du harnais accepte tout ; le câblage CLI est testé.
        assert_eq!(
            lance(
                &racine,
                &["ordre", "new", "--token", "faux:jeton", "--", "x"]
            ),
            1
        );
        assert!(!dir.ordres().is_file() || lire_lignes(&dir.ordres()).is_empty());
        let _ = std::fs::remove_dir_all(&racine);
    }

    #[test]
    fn doctor_sain_et_absent() {
        let racine = dir_temp();
        assert_eq!(
            lance(&racine, &["send", "--from", "t-doc", "--", "sonde"]),
            0
        );
        assert_eq!(lance(&racine, &["doctor"]), 0);
        assert_eq!(
            exec(&["doctor".to_string()], &racine.join("n-existe-pas")),
            2
        );
        let _ = std::fs::remove_dir_all(&racine);
    }
}
