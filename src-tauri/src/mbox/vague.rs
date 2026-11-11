//! Vague D6 — orchestrateur de vagues CLI (file + N parallèles + dépendances).
//!
//! CLI UNIQUEMENT : `plan|run|lister|etat`, fichier `ID|APRES|COMMANDE`,
//! blanche `send bus ordre(new|ack) hb metriques` (jamais `jeton|rotate|
//! vague`), affichages caviardés. Exécution dans `vague_exec.rs` (ordre
//! topo, threads + poll 50 ms, file.log chmod 600, synthèse APPEND).
//!
//! CÂBLAGE ASSEMBLEUR (hooks ≤ 5 L / fichier) :
//! - `mod.rs` +2 L (alpha, entre `throttle` et `verrou`) :
//!   `pub mod vague;` puis `pub mod vague_exec;`
//! - `cli.rs` +1 L (`match` de `exec`, budget 489 L OK) :
//!   `"vague" => super::vague::cmd_vague(&dir, reste),`
//! - `cli.rs` AIDE +1 L (total 2 L) : `  bcast vague plan|run|lister|etat`
//! - `cli_gestion.rs` : RIEN (ne route que `rotate|ordre`, cf `cli.rs:63-89`
//!   et `cli_gestion.rs:187-200`). `docs/mbox.md` : assembleur.

use std::path::Path;

use super::fichier::lire_lignes;
use super::format::{BcastDir, Sortie};
use super::outils::{decoupage, opt, usg};
use super::vague_exec::{ecrit_synthese, execute, ordre_execution, sauve};
use EtatTache::*;

const PARALLELES_DEFAUT: usize = 4;
const DELAI_DEFAUT_SECS: u64 = 60;

/// État d'une tâche (`NOMS` suit l'ordre des variantes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EtatTache {
    Attente,
    Cours,
    Reussie,
    Echec,
    Bloque,
    Expiree,
}

impl EtatTache {
    const NOMS: [&'static str; 6] = ["attente", "cours", "ok", "echec", "bloque", "expiree"];
    /// Parse un état persisté (`None` si inconnu).
    pub(crate) fn parse(s: &str) -> Option<EtatTache> {
        Self::NOMS
            .iter()
            .position(|&n| n == s)
            .map(|i| [Attente, Cours, Reussie, Echec, Bloque, Expiree][i])
    }
    /// Forme canonique persistée et affichée.
    pub(crate) fn texte(&self) -> &'static str {
        Self::NOMS[*self as usize]
    }
}

/// Tâche : identifiant, dépendances (`apres`), commande mbox (argv).
#[derive(Debug, Clone)]
pub(crate) struct Tache {
    pub(crate) id: String,
    pub(crate) apres: Vec<String>,
    pub(crate) argv: Vec<String>,
}

/// `vague` : répartit vers plan/run/lister/etat.
pub(crate) fn cmd_vague(dir: &BcastDir, args: &[String]) -> Sortie {
    match args.first().map(String::as_str) {
        Some("plan") => cmd_plan(dir, &args[1..]),
        Some("run") => cmd_run(dir, &args[1..]),
        Some("lister") => cmd_lister(dir),
        Some("etat") => cmd_etat(dir),
        _ => usg("usage : bcast vague plan|run|lister|etat"),
    }
}

/// Dossier `vague` (créé au besoin), sous la racine broadcast.
pub(crate) fn dossier_vague(dir: &BcastDir) -> std::path::PathBuf {
    let dossier = dir.joindre("vague");
    let _ = std::fs::create_dir_all(&dossier);
    dossier
}

/// Liste blanche mbox (`ordre` = `new|ack` seuls). Ne montre que le 1er mot.
fn valide_argv(argv: &[String]) -> Result<(), String> {
    let premier = argv.first().map(String::as_str).unwrap_or("");
    let ok = match premier {
        "send" | "bus" | "hb" | "metriques" => true,
        "ordre" => matches!(argv.get(1).map(String::as_str), Some("new" | "ack")),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(format!("commande refusee : {premier}"))
    }
}

/// `argv` affichable : valeurs `--token` masquées (`--token ***`,
/// `--token=***`). Tout affichage passe par ici, jamais de secret.
pub(crate) fn caviarder(argv: &[String]) -> String {
    let mut sortie = Vec::with_capacity(argv.len());
    let mut mange = false;
    for mot in argv {
        if mange {
            sortie.push("***");
            mange = false;
        } else if mot == "--token" {
            sortie.push("--token");
            mange = true;
        } else if mot.starts_with("--token=") {
            sortie.push("--token=***");
        } else {
            sortie.push(mot.as_str());
        }
    }
    sortie.join(" ")
}

/// Parse `ID|APRES|COMMANDE` ou `ID|APRES|ETAT|EPOCH|COMMANDE`. Les erreurs
/// ne répètent jamais le contenu (il peut porter un `--token`).
fn parse_tache(n: usize, ligne: &str) -> Result<(Tache, Option<EtatTache>), String> {
    let champs: Vec<&str> = ligne.split('|').collect();
    let (id, apres, etat, cmd) = match champs.len() {
        3 => (champs[0], champs[1], None, champs[2]),
        5 => (champs[0], champs[1], Some(champs[2]), champs[4]),
        _ => return Err(format!("ligne {n} malformee")),
    };
    let id = id.trim();
    let id_ok = !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|o| o.is_ascii_alphanumeric() || o == b'_' || o == b'-');
    if !id_ok {
        return Err(format!("ligne {n} : id invalide"));
    }
    let brut = apres.trim();
    let apres: Vec<String> = brut
        .strip_prefix("apres:")
        .unwrap_or(brut)
        .split(',')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(str::to_string)
        .collect();
    let argv: Vec<String> = cmd.split_whitespace().map(str::to_string).collect();
    if argv.is_empty() {
        return Err(format!("ligne {n} : commande vide"));
    }
    valide_argv(&argv).map_err(|e| format!("ligne {n} : {e}"))?;
    let etat = etat
        .map(|s| EtatTache::parse(s).ok_or(format!("ligne {n} : etat inconnu")))
        .transpose()?;
    let tache = Tache {
        id: id.to_string(),
        apres,
        argv,
    };
    Ok((tache, etat))
}

/// Lit `--fichier F` (`#` et blancs ignorés, ids uniques, états jetés :
/// un `run` rejoue toute la file, jamais de reprise).
fn lire_fichier_taches(chemin: &Path) -> Result<Vec<Tache>, String> {
    use std::collections::HashSet;
    let contenu = std::fs::read_to_string(chemin)
        .map_err(|_| format!("fichier illisible : {}", chemin.display()))?;
    let mut taches = Vec::new();
    let mut vus = HashSet::new();
    for (i, ligne) in contenu.lines().enumerate() {
        let tasse = ligne.trim();
        if tasse.is_empty() || tasse.starts_with('#') {
            continue;
        }
        let (tache, _) = parse_tache(i + 1, tasse)?;
        if !vus.insert(tache.id.clone()) {
            return Err(format!("tache en double : {}", tache.id));
        }
        taches.push(tache);
    }
    Ok(taches)
}

/// File persistée + états (malformées ignorées, absente = vide).
fn charge_persiste(dir: &BcastDir) -> Vec<(Tache, EtatTache)> {
    lire_lignes(&dossier_vague(dir).join("file.log"))
        .iter()
        .enumerate()
        .filter_map(|(i, l)| parse_tache(i + 1, l).ok())
        .map(|(t, e)| (t, e.unwrap_or(Attente)))
        .collect()
}

/// Tâches visées : `--fichier F` sinon file persistée (états jetés).
fn charge_cible(dir: &BcastDir, fichier: Option<&str>) -> Result<Vec<Tache>, String> {
    match fichier {
        Some(f) => lire_fichier_taches(Path::new(f)),
        None => Ok(charge_persiste(dir).into_iter().map(|(t, _)| t).collect()),
    }
}

/// Charge + valide, rend tâches et ordre (`bcast: vague:` préfixé ici seul).
fn charge_valide(
    dir: &BcastDir,
    fichier: Option<&str>,
) -> Result<(Vec<Tache>, Vec<usize>), Sortie> {
    let taches = charge_cible(dir, fichier).map_err(|e| {
        eprintln!("bcast: vague: {e}");
        Sortie::Usage
    })?;
    let ordre = ordre_execution(&taches).map_err(|e| {
        eprintln!("bcast: vague: {e}");
        Sortie::Usage
    })?;
    Ok((taches, ordre))
}

/// `vague plan` : ordre d'exécution caviardé, sans exécuter.
fn cmd_plan(dir: &BcastDir, args: &[String]) -> Sortie {
    let Ok((opts, reste)) = decoupage(args, &[("fichier", true)]) else {
        return Sortie::Usage;
    };
    if !reste.is_empty() {
        return usg("usage : bcast vague plan [--fichier F]");
    }
    let (taches, ordre) = match charge_valide(dir, opt(&opts, "fichier")) {
        Ok(x) => x,
        Err(s) => return s,
    };
    println!("vague: {} taches", taches.len());
    for (rang, &i) in ordre.iter().enumerate() {
        let deps = if taches[i].apres.is_empty() {
            "-".to_string()
        } else {
            taches[i].apres.join(",")
        };
        let cmd = caviarder(&taches[i].argv);
        println!("{}. {} [apres: {deps}] :: {cmd}", rang + 1, taches[i].id);
    }
    Sortie::Ok
}

/// `vague run` : exécute, persiste, synthèse. rc=1 si KO.
fn cmd_run(dir: &BcastDir, args: &[String]) -> Sortie {
    let specs = [("paralleles", true), ("fichier", true), ("delai", true)];
    let Ok((opts, reste)) = decoupage(args, &specs) else {
        return Sortie::Usage;
    };
    if !reste.is_empty() {
        return usg("usage : bcast vague run [--paralleles N] [--fichier F] [--delai S]");
    }
    let paralleles: usize = match opt(&opts, "paralleles").map(str::parse) {
        None => PARALLELES_DEFAUT,
        Some(Ok(v @ 1..=32)) => v,
        _ => return usg("--paralleles attend 1..32 (defaut 4)"),
    };
    let delai: u64 = match opt(&opts, "delai").map(str::parse) {
        None => DELAI_DEFAUT_SECS,
        Some(Ok(v @ 1..=3600)) => v,
        _ => return usg("--delai attend 1..3600 secondes (defaut 60)"),
    };
    let (taches, _) = match charge_valide(dir, opt(&opts, "fichier")) {
        Ok(x) => x,
        Err(s) => return s,
    };
    if taches.is_empty() {
        println!("file vide");
        return Sortie::Ok;
    }
    super::metriques::incremente(dir, "vague_run");
    let etats = execute(dir.racine(), &taches, paralleles, delai);
    sauve(dir, &taches, &etats);
    ecrit_synthese(dir, &taches, &etats, paralleles, delai);
    let ko = etats.iter().filter(|e| **e != Reussie).count();
    println!("vague: ok={} ko={ko}", etats.len() - ko);
    let cle = if ko == 0 { "vague_ok" } else { "vague_echec" };
    super::metriques::incremente(dir, cle);
    if ko == 0 {
        Sortie::Ok
    } else {
        Sortie::ErreurExecution
    }
}

/// `vague lister` : file persistée avec états (caviardé).
fn cmd_lister(dir: &BcastDir) -> Sortie {
    let file = charge_persiste(dir);
    if file.is_empty() {
        println!("file vide");
        return Sortie::Ok;
    }
    for (tache, etat) in &file {
        println!(
            "{} [{}] :: {}",
            tache.id,
            etat.texte(),
            caviarder(&tache.argv)
        );
    }
    Sortie::Ok
}

/// `vague etat` : compteurs par état sur la file persistée.
fn cmd_etat(dir: &BcastDir) -> Sortie {
    let file = charge_persiste(dir);
    let compte = |voulu: EtatTache| file.iter().filter(|(_, e)| *e == voulu).count();
    println!(
        "vague: attente={} cours={} ok={} echec={} bloque={} expiree={} total={}",
        compte(Attente),
        compte(Cours),
        compte(Reussie),
        compte(Echec),
        compte(Bloque),
        compte(Expiree),
        file.len()
    );
    Sortie::Ok
}

#[cfg(test)]
mod tests {
    use super::super::format::maintenant;
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COMPTEUR: AtomicU64 = AtomicU64::new(0);

    /// Dossier broadcast jetable unique (jamais le vrai `.scratch`).
    fn dir_temp() -> PathBuf {
        let n = COMPTEUR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("mbox-vague-{n}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dossier de test");
        dir
    }

    /// Exécute `mots` (après `vague`) sur `racine`, rend le code.
    fn lance(racine: &Path, mots: &[&str]) -> i32 {
        let args: Vec<String> = mots.iter().map(|s| s.to_string()).collect();
        cmd_vague(&BcastDir::depuis(racine.to_path_buf()), &args).code()
    }

    #[test]
    fn parse_blanche_et_caviarde() {
        let (t, _) = parse_tache(1, "V-1|apres:V-0,V-9|bus -t gpu -- x").expect("parse");
        assert_eq!(t.id, "V-1");
        assert_eq!(t.apres.len(), 2);
        assert_eq!(t.argv[0], "bus");
        assert!(parse_tache(2, "V-2||ordre ack O-1 lu --from a").is_ok());
        let refus = [
            "V-3||ordre revoque O-1",
            "V-4||jeton list",
            "V-5||rotate",
            "V-6||vague plan",
            "mauvais id||bus -t gpu -- x",
            "V-8||",
            "pas-de-separateur",
        ];
        for (k, ligne) in refus.iter().enumerate() {
            assert!(parse_tache(10 + k, ligne).is_err(), "refus : {ligne}");
        }
        let mots: Vec<String> = ["ordre", "ack", "O-1", "--token", "N:S"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let affiche = caviarder(&mots);
        assert!(affiche.contains("--token ***"), "vue : {affiche}");
        assert!(!affiche.contains("N:S"), "fuite : {affiche}");
        let egal = [
            "ordre".to_string(),
            "new".to_string(),
            "--token=ABC".to_string(),
        ];
        assert_eq!(caviarder(&egal), "ordre new --token=***");
        let tous = [Attente, Cours, Reussie, Echec, Bloque, Expiree];
        for (texte, etat) in EtatTache::NOMS.iter().zip(tous) {
            assert_eq!(EtatTache::parse(texte), Some(etat));
            assert_eq!(etat.texte(), *texte);
        }
        assert_eq!(EtatTache::parse("bidon"), None);
    }

    #[test]
    fn run_mini_et_usages() {
        let racine = dir_temp();
        let dir = BcastDir::depuis(racine.clone());
        let fichier = racine.join("vague.txt");
        let contenu = "V-1||bus -t divers --from vague-t -- coucou\nV-2|V-1|hb sonde-vague ok\n";
        std::fs::write(&fichier, contenu).expect("fichier");
        let f = fichier.to_str().unwrap();
        assert_eq!(lance(&racine, &["plan", "--fichier", f]), 0);
        assert_eq!(
            lance(&racine, &["run", "--fichier", f, "--paralleles", "2"]),
            0
        );
        let bus = std::fs::read_to_string(dir.bus("divers")).expect("bus");
        assert!(bus.contains("coucou"), "bus : {bus}");
        assert!(dir.hb("sonde-vague").is_file());
        assert_eq!(lance(&racine, &["lister"]), 0);
        assert_eq!(lance(&racine, &["etat"]), 0);
        let cas: &[(&[&str], i32)] = &[
            (&[], 2),
            (&["nope"], 2),
            (&["run", "--paralleles", "0"], 2),
            (&["run", "--delai", "xx"], 2),
            (&["run", "--fichier", "/n/existe/pas"], 2),
        ];
        for (mots, code) in cas {
            assert_eq!(lance(&racine, mots), *code, "cas {mots:?}");
        }
        let _ = std::fs::remove_dir_all(&racine);
    }

    #[test]
    fn echec_bloque_synthese_sans_secret() {
        let racine = dir_temp();
        let dir = BcastDir::depuis(racine.clone());
        let fichier = racine.join("ko.txt");
        let contenu =
            "V-1||ordre ack O-x lu --from t --token N:SECRET123\nV-2|V-1|hb sonde-vague ok\n";
        std::fs::write(&fichier, contenu).expect("fichier");
        assert_eq!(
            lance(&racine, &["run", "--fichier", fichier.to_str().unwrap()]),
            1
        );
        let file = charge_persiste(&dir);
        assert_eq!(file[0].1, Echec);
        assert_eq!(file[1].1, Bloque);
        let synthese = dossier_vague(&dir).join(format!("vague-{}.md", &maintenant()[..10]));
        let texte = std::fs::read_to_string(&synthese).expect("synthese");
        assert!(texte.contains("***"), "caviarde : {texte}");
        assert!(!texte.contains("SECRET123"), "fuite : {texte}");
        let _ = std::fs::remove_dir_all(&racine);
    }
}
