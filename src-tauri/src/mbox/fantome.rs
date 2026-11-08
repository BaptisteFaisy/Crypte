//! Mode fantôme : miroir d'observation sans écriture prod (vague 3, V3-SHADOW).
//!
//! `bcast --fantome <commande...>` exécute la commande sur un miroir
//! (`.fantome/`, même arborescence) au lieu de la prod, puis affiche le
//! diff : ce qui AURAIT changé. `bcast fantome rejouer|diff|etat|purge`
//! pilote le miroir : rejeu d'un corpus, diff, état, nettoyage.
//!
//! Garanties : zéro écriture prod en mode fantôme (tout `exec` reçoit la
//! racine miroir, le diff ne lit la prod qu'en fail-open) ; double
//! écriture de contrôle (chaque op fantôme trace `controle.log` en plus
//! du miroir) ; garde typée (D1 du benchmark) qui classe chaque commande
//! avant exécution et refuse avec un motif stable (`jeton` touche le XDG
//! prod, `rotate` est destructeur, imbrication interdite).

use std::path::{Path, PathBuf};

use super::fichier::{append_verrouille, lire_lignes};
use super::format::{epoque, BcastDir, Sortie, SUJETS_BUS};

/// Garde typée des écritures fantôme (D1) : classement avant exécution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cible {
    /// Exécutable sur le miroir (zéro écriture prod).
    Miroir,
    /// Refusé en fantôme, avec motif stable (jamais silencieux).
    Refuse(&'static str),
}

/// Classe une commande : miroir ou refus typé.
pub(crate) fn classe(commande: &str) -> Cible {
    match commande {
        "send" | "bus" | "tail" | "hb" | "ordre" | "digest" | "doctor" | "metriques" => {
            Cible::Miroir
        }
        "help" | "--help" | "-h" | "version" => Cible::Miroir,
        "jeton" => Cible::Refuse("jetons hors miroir (XDG prod)"),
        "rotate" => Cible::Refuse("rotation destructrice, sens interdit en miroir"),
        "fantome" | "--fantome" => Cible::Refuse("miroir imbriqué interdit"),
        _ => Cible::Refuse("commande inconnue"),
    }
}

/// Racine du miroir (sous-dossier de la prod, jamais l'inverse).
pub(crate) fn racine_fantome(dir: &BcastDir) -> PathBuf {
    dir.joindre(".fantome")
}

/// Miroir initialisé (idempotent, ne vide jamais un miroir existant).
fn dir_fantome(dir: &BcastDir) -> BcastDir {
    let fant = BcastDir::depuis(racine_fantome(dir));
    let _ = fant.init();
    fant
}

/// Replie un code `exec` vers la sortie typée.
fn code_vers_sortie(code: i32) -> Sortie {
    match code {
        0 => Sortie::Ok,
        2 => Sortie::Usage,
        3 => Sortie::DejaEnCours,
        _ => Sortie::ErreurExecution,
    }
}

/// Double écriture de contrôle : chaque op fantôme trace le journal de
/// contrôle EN PLUS du miroir (corpus rejouable, audit des essais).
fn note_controle(fant: &BcastDir, op: &[String], code: i32) {
    let ligne = format!("{} op={} code={code}", epoque(), op.join(" "));
    let _ = append_verrouille(&fant.joindre("controle.log"), &[ligne]);
}

/// Fichiers miroités comparés par le diff (relatifs à chaque racine).
fn fichiers_miroir() -> Vec<String> {
    let mut fichiers = vec![
        "BOITE-USER.md".to_string(),
        "ORDRES.log".to_string(),
        "ORDRES-ACK.log".to_string(),
        "ORDRE.md".to_string(),
        ".state/metriques".to_string(),
    ];
    for sujet in SUJETS_BUS {
        fichiers.push(format!("bus/{sujet}.log"));
    }
    fichiers
}

/// Diff prod/miroir : `(relatif, lignes_prod, lignes_miroir)` par fichier.
/// Lectures fail-open des deux côtés, aucune écriture nulle part.
fn difference(prod: &BcastDir, fant: &BcastDir) -> Vec<(String, usize, usize)> {
    fichiers_miroir()
        .into_iter()
        .map(|rel| {
            let p = lire_lignes(&prod.joindre(&rel)).len();
            let m = lire_lignes(&fant.joindre(&rel)).len();
            (rel, p, m)
        })
        .collect()
}

/// Affiche le diff (fichiers divergents seuls) et rend les lignes ajoutées.
/// Miroir vide + prod vide = silence, zéro.
fn imprime_diff(dir: &BcastDir) -> usize {
    let fant = BcastDir::depuis(racine_fantome(dir));
    let mut ajoutees = 0;
    for (rel, p, m) in difference(dir, &fant) {
        if p != m {
            println!("fantome diff {rel}: prod={p} miroir={m}");
            ajoutees += m.saturating_sub(p);
        }
    }
    ajoutees
}

/// `bcast --fantome <commande...>` : exécute sur le miroir, trace le
/// contrôle, affiche le diff. Zéro écriture prod (garanti : `exec` ne
/// voit que la racine miroir).
pub(crate) fn exec_fantome(dir: &BcastDir, args: &[String]) -> Sortie {
    let Some(premiere) = args.first() else {
        eprintln!("bcast: usage : bcast --fantome <commande...>");
        return Sortie::Usage;
    };
    if let Cible::Refuse(motif) = classe(premiere) {
        eprintln!("bcast: fantôme refuse {premiere} : {motif}");
        return Sortie::ErreurExecution;
    }
    let fant = dir_fantome(dir);
    let code = super::cli::exec(args, fant.racine());
    note_controle(&fant, args, code);
    let ajoutees = imprime_diff(dir);
    println!("fantome: {premiere} code={code} +{ajoutees} lignes (miroir seul, prod intacte)");
    code_vers_sortie(code)
}

/// `bcast fantome rejouer|diff|etat|purge`.
pub(crate) fn cmd_fantome(dir: &BcastDir, args: &[String]) -> Sortie {
    match args.first().map(String::as_str) {
        Some("rejouer") => match args.get(1) {
            Some(f) => rejouer(dir, Path::new(f)),
            None => {
                eprintln!("bcast: usage : bcast fantome rejouer <corpus>");
                Sortie::Usage
            }
        },
        Some("diff") => {
            imprime_diff(dir);
            Sortie::Ok
        }
        Some("etat") => etat_fantome(dir),
        Some("purge") => purge_fantome(dir),
        _ => {
            eprintln!("bcast: usage : bcast fantome rejouer|diff|etat|purge");
            Sortie::Usage
        }
    }
}

/// Découpe une ligne de corpus : espaces sauf entre guillemets doubles
/// (guillemet non fermé = tout le reste en un jeton, documenté).
fn decoupe(ligne: &str) -> Vec<String> {
    let mut mots = Vec::new();
    let mut courant = String::new();
    let mut guillemets = false;
    let mut rempli = false;
    for c in ligne.chars() {
        match c {
            '"' => {
                guillemets = !guillemets;
                rempli = true;
            }
            _ if c.is_whitespace() && !guillemets => {
                if rempli {
                    mots.push(std::mem::take(&mut courant));
                    rempli = false;
                }
            }
            _ => {
                courant.push(c);
                rempli = true;
            }
        }
    }
    if rempli {
        mots.push(courant);
    }
    mots
}

/// Rejoue un corpus (une op par ligne, `#` = commentaire) sur le miroir
/// puis affiche le diff. Les ops refusées par la garde sont sautées
/// (comptées, jamais silencieuses). Zéro écriture prod.
fn rejouer(dir: &BcastDir, corpus: &Path) -> Sortie {
    if !corpus.is_file() {
        eprintln!("bcast: corpus illisible");
        return Sortie::ErreurExecution;
    }
    let fant = dir_fantome(dir);
    let (mut ok, mut ko, mut refusees) = (0, 0, 0);
    for ligne in lire_lignes(corpus) {
        let tasse = ligne.trim();
        if tasse.is_empty() || tasse.starts_with('#') {
            continue;
        }
        let op = decoupe(tasse);
        if op.is_empty() {
            continue;
        }
        if let Cible::Refuse(motif) = classe(&op[0]) {
            println!("fantome: refuse {} : {motif}", op[0]);
            refusees += 1;
            continue;
        }
        let code = super::cli::exec(&op, fant.racine());
        note_controle(&fant, &op, code);
        if code == 0 {
            ok += 1;
        } else {
            ko += 1;
        }
        println!("fantome: rejouée `{}` code={code}", op.join(" "));
    }
    let ajoutees = imprime_diff(dir);
    println!(
        "fantome: corpus {ok} ok {ko} ko {refusees} refusées +{ajoutees} lignes (miroir seul)"
    );
    if ko > 0 {
        Sortie::ErreurExecution
    } else {
        Sortie::Ok
    }
}

/// État du miroir : présence, contrôle, diff.
fn etat_fantome(dir: &BcastDir) -> Sortie {
    let racine = racine_fantome(dir);
    if !racine.is_dir() {
        println!("fantome: aucun miroir");
        return Sortie::Ok;
    }
    let fant = BcastDir::depuis(racine);
    let traces = lire_lignes(&fant.joindre("controle.log"));
    println!(
        "fantome: miroir {} ({} ops tracées)",
        fant.racine().display(),
        traces.len()
    );
    if let Some(derniere) = traces.last() {
        println!("fantome: dernière {derniere}");
    }
    imprime_diff(dir);
    Sortie::Ok
}

/// Purge le miroir (prod intacte par construction : autre dossier).
fn purge_fantome(dir: &BcastDir) -> Sortie {
    let racine = racine_fantome(dir);
    match std::fs::remove_dir_all(&racine) {
        Ok(()) => println!("fantome: miroir purgé"),
        Err(_) if !racine.exists() => println!("fantome: aucun miroir"),
        Err(_) => {
            eprintln!("bcast: purge du miroir impossible");
            return Sortie::ErreurExecution;
        }
    }
    Sortie::Ok
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bac jetable unique par test (jamais le vrai `.scratch`).
    fn bac(tag: &str) -> BcastDir {
        let dir = std::env::temp_dir().join(format!("mbox-fantome-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("bac");
        BcastDir::depuis(dir)
    }

    /// Exécute `mots` sur `dir` via le CLI réel, rend le code.
    fn lance(dir: &BcastDir, mots: &[&str]) -> i32 {
        let args: Vec<String> = mots.iter().map(|s| s.to_string()).collect();
        super::super::cli::exec(&args, dir.racine())
    }

    #[test]
    fn garde_classe_avant_execution() {
        for cmd in [
            "send",
            "bus",
            "tail",
            "hb",
            "ordre",
            "digest",
            "doctor",
            "metriques",
        ] {
            assert_eq!(classe(cmd), Cible::Miroir, "{cmd}");
        }
        assert!(matches!(classe("jeton"), Cible::Refuse(_)));
        assert!(matches!(classe("rotate"), Cible::Refuse(_)));
        assert!(matches!(classe("fantome"), Cible::Refuse(_)));
        assert!(matches!(classe("nope"), Cible::Refuse(_)));
    }

    #[test]
    fn decoupe_guillemets_regroupes() {
        assert_eq!(
            decoupe("send --from a -- salut"),
            vec!["send", "--from", "a", "--", "salut"]
        );
        assert_eq!(
            decoupe("send --from a -- \"bonjour le monde\""),
            vec!["send", "--from", "a", "--", "bonjour le monde"]
        );
        assert!(decoupe("   ").is_empty());
    }

    #[test]
    fn exec_zero_ecriture_prod() {
        let dir = bac("zero-prod");
        assert_eq!(lance(&dir, &["send", "--from", "t-f", "--", "prod"]), 0);
        let avant = std::fs::read_to_string(dir.boite()).expect("prod");
        let op: Vec<String> = ["send", "--from", "t-f", "--", "ombre"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(exec_fantome(&dir, &op).code(), 0);
        let apres = std::fs::read_to_string(dir.boite()).expect("prod intacte");
        assert_eq!(avant, apres, "la prod ne bouge pas");
        let fant = BcastDir::depuis(racine_fantome(&dir));
        let miroir = std::fs::read_to_string(fant.boite()).expect("miroir");
        assert!(miroir.contains("ombre"), "miroir : {miroir}");
        assert!(
            !miroir.contains("prod"),
            "miroir vierge de la prod : {miroir}"
        );
        assert!(fant.joindre("controle.log").is_file(), "double écriture");
        let refuse: Vec<String> = ["jeton", "new", "x"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(exec_fantome(&dir, &refuse).code(), 1);
        let _ = std::fs::remove_dir_all(dir.racine());
    }

    #[test]
    fn rejouer_corpus_puis_purge() {
        let dir = bac("corpus");
        let corpus = dir.joindre("corpus.txt");
        std::fs::write(
            &corpus,
            "# essai\nsend --from t-c -- \"bonjour ombre\"\njeton new x\nbus -t gpu --from t-c -- chaud\n",
        )
        .expect("corpus");
        assert_eq!(rejouer(&dir, &corpus).code(), 0);
        let fant = BcastDir::depuis(racine_fantome(&dir));
        let miroir = std::fs::read_to_string(fant.boite()).expect("miroir");
        assert!(miroir.contains("bonjour ombre"), "rejeu : {miroir}");
        assert_eq!(etat_fantome(&dir).code(), 0);
        assert_eq!(purge_fantome(&dir).code(), 0);
        assert!(!racine_fantome(&dir).exists());
        assert_eq!(purge_fantome(&dir).code(), 0); // idempotent
        assert_eq!(rejouer(&dir, &dir.joindre("absent.txt")).code(), 1);
        let _ = std::fs::remove_dir_all(dir.racine());
    }
}
