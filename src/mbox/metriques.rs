//! Compteurs durables de la boîte aux lettres (vague 3, V3-METRICS).
//!
//! Chaque écriture CLI incrémente son compteur via un hook d'une ligne
//! (`incremente` / `incremente_agent`), best-effort fail-open : une
//! métrique perdue ne casse jamais la commande. Persistance lisible par
//! le shell : `.state/metriques`, lignes `cle=valeur` (malformées
//! ignorées), lecture-écriture sous le verrou frère partagé.
//!
//! Clefs globales : `send_ok`, `send_dup`, `send_rate`, `send_same`,
//! `bus_ok`, `bus_dup`, `ordre_new`, `ordre_revoque`, `ack_ok`, `hb_ok`.
//! Compteurs par agent (différenciateur D4 du benchmark : personne ne
//! les a en durable) : `par_agent.<san>.cle` (`san` = identité
//! assainie, `sanitize` du socle, jamais le secret).
//!
//! `cmd_metriques` affiche globales + par agent (+ `--agent` filtre),
//! `resume` tient la ligne `doctor`, `verifie_integrite` contrôle les
//! `hash=` des ordres (différenciateur D2 : `hash=` scellé à la
//! création par `journal`, vérifié ici avec `empreinte`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::fichier::lire_lignes;
use super::format::{sanitize, BcastDir, Sortie};
use super::ordres::{empreinte, Extensions, Ordre};
use super::outils::{decoupage, opt};
use super::verrou::{chemin_verrou, verrouiller, EnCasDEchec};

/// Fichier des compteurs (`.state`, épargné par la purge `rotate` qui
/// ne vise que `*.dedup` et `*.ratel`).
fn chemin(dir: &BcastDir) -> PathBuf {
    dir.joindre(".state/metriques")
}

/// Incrémente un compteur global (`send_ok`, `bus_dup`…), best-effort.
pub(crate) fn incremente(dir: &BcastDir, cle: &str) {
    incremente_cle(dir, cle);
}

/// Incrémente un compteur par agent (D4), best-effort.
pub(crate) fn incremente_agent(dir: &BcastDir, agent: &str, cle: &str) {
    incremente_cle(dir, &format!("par_agent.{}.{}", sanitize(agent), cle));
}

/// Traduit un verdict throttle (`dup`/`rate`/`same-state`) en clef.
/// Hook d'une ligne pour `cli.rs`, qui ne connaît que le verdict.
pub(crate) fn incremente_motif(dir: &BcastDir, verdict: &str) {
    let cle = match verdict {
        "dup" => "send_dup",
        "rate" => "send_rate",
        _ => "send_same",
    };
    incremente_cle(dir, cle);
}

/// Cœur unique : lecture-modification-écriture sous verrou fail-open.
/// Verrou impossible ou écriture manquée = compteur perdu, silence.
fn incremente_cle(dir: &BcastDir, cle: &str) {
    let cible = chemin(dir);
    let Ok(Some(_garde)) = verrouiller(&chemin_verrou(&cible), 10, EnCasDEchec::FailOpen) else {
        return;
    };
    let mut compteurs = lire(dir);
    let entree = compteurs.entry(cle.to_string()).or_insert(0);
    *entree = entree.saturating_add(1);
    let _ = std::fs::write(&cible, serialise(&compteurs));
}

/// Lit les compteurs (fail-open : absent ou malformé = partiel ou vide).
pub(crate) fn lire(dir: &BcastDir) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for ligne in lire_lignes(&chemin(dir)) {
        let Some((cle, valeur)) = ligne.split_once('=') else {
            continue;
        };
        let Ok(n) = valeur.trim().parse::<u64>() else {
            continue;
        };
        if !cle.is_empty() {
            out.insert(cle.to_string(), n);
        }
    }
    out
}

/// Sérialise au format shell (`cle=valeur`, trié, newline finale).
fn serialise(compteurs: &BTreeMap<String, u64>) -> String {
    let mut texte = String::new();
    for (cle, valeur) in compteurs {
        texte.push_str(&format!("{cle}={valeur}\n"));
    }
    texte
}

/// Ligne unique pour `doctor` : que des compteurs, jamais de secret.
pub(crate) fn resume(dir: &BcastDir) -> String {
    let c = lire(dir);
    let n = |k: &str| c.get(k).copied().unwrap_or(0);
    format!(
        "metriques send={} dup={} rate={} same={} bus={} bus_dup={} ordres={} revoques={} acks={} hb={}",
        n("send_ok"),
        n("send_dup"),
        n("send_rate"),
        n("send_same"),
        n("bus_ok"),
        n("bus_dup"),
        n("ordre_new"),
        n("ordre_revoque"),
        n("ack_ok"),
        n("hb_ok"),
    )
}

/// Contrôle d'intégrité des ordres (D2) : `(ok, ko, sans_hash)`.
/// Chaque `hash=` est rejoué contre `empreinte` du corps ; les ordres
/// shell (sans `hash=`) et les lignes malformées ne comptent ni ok ni ko.
pub(crate) fn verifie_integrite(dir: &BcastDir) -> (usize, usize, usize) {
    let (mut ok, mut ko, mut sans) = (0, 0, 0);
    for ligne in lire_lignes(&dir.ordres()) {
        let Some(ordre) = Ordre::parse_ligne(&ligne) else {
            continue;
        };
        let (corps, ext) = Extensions::extraire(&ordre.texte);
        match ext.hash {
            None => sans += 1,
            Some(h) if h == empreinte(&corps) => ok += 1,
            Some(_) => ko += 1,
        }
    }
    (ok, ko, sans)
}

/// `metriques [--agent ID]` : globales, par agent, intégrité des ordres.
pub(crate) fn cmd_metriques(dir: &BcastDir, args: &[String]) -> Sortie {
    let Ok((opts, _)) = decoupage(args, &[("agent", true)]) else {
        return Sortie::Usage;
    };
    let compteurs = lire(dir);
    if compteurs.is_empty() {
        println!("aucune métrique");
    }
    for (cle, valeur) in compteurs
        .iter()
        .filter(|(k, _)| !k.starts_with("par_agent."))
    {
        println!("{cle}={valeur}");
    }
    let filtre = opt(&opts, "agent").map(sanitize);
    for (cle, valeur) in compteurs
        .iter()
        .filter(|(k, _)| k.starts_with("par_agent."))
    {
        let Some(reste) = cle.strip_prefix("par_agent.") else {
            continue;
        };
        let Some((san, sous)) = reste.split_once('.') else {
            continue;
        };
        if filtre.as_deref().is_some_and(|f| f != san) {
            continue;
        }
        println!("agent {san} {sous}={valeur}");
    }
    let (ok, ko, sans) = verifie_integrite(dir);
    let (s_ok, s_ko, s_sans) = super::signature::compte_journal(dir);
    println!("integrite ordres: {ok} ok {ko} ko {sans} sans hash, signatures: {s_ok} ok {s_ko} ko {s_sans} sans");
    Sortie::Ok
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bac jetable unique par test (jamais le vrai `.scratch`).
    fn bac(tag: &str) -> BcastDir {
        let dir = std::env::temp_dir().join(format!("mbox-metriques-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(dir.join(".state")).expect("bac");
        BcastDir::depuis(dir)
    }

    #[test]
    fn compte_et_relit_global_et_agent() {
        let dir = bac("compte");
        incremente(&dir, "send_ok");
        incremente(&dir, "send_ok");
        incremente_agent(&dir, "muse", "send_ok");
        incremente_motif(&dir, "dup");
        incremente_motif(&dir, "rate");
        incremente_motif(&dir, "same-state");
        let c = lire(&dir);
        assert_eq!(c.get("send_ok"), Some(&2));
        assert_eq!(c.get("send_dup"), Some(&1));
        assert_eq!(c.get("send_rate"), Some(&1));
        assert_eq!(c.get("send_same"), Some(&1));
        let cle_agent = format!("par_agent.{}.send_ok", sanitize("muse"));
        assert_eq!(c.get(&cle_agent), Some(&1));
        assert!(chemin(&dir).is_file());
        let _ = std::fs::remove_dir_all(dir.racine());
    }

    #[test]
    fn lecture_tolerante_aux_malformees() {
        let dir = bac("tolere");
        std::fs::write(chemin(&dir), "send_ok=3\npourri\nsans_valeur=\nneg=-1\n").expect("seed");
        let c = lire(&dir);
        assert_eq!(c.len(), 1);
        assert_eq!(c.get("send_ok"), Some(&3));
        assert!(resume(&dir).contains("send=3"));
        let _ = std::fs::remove_dir_all(dir.racine());
    }

    #[test]
    fn commande_affiche_et_filtre() {
        let dir = bac("cmd");
        incremente(&dir, "hb_ok");
        incremente_agent(&dir, "muse", "ack_ok");
        assert_eq!(cmd_metriques(&dir, &[]).code(), 0);
        let filtre = vec!["--agent".to_string(), "muse".to_string()];
        assert_eq!(cmd_metriques(&dir, &filtre).code(), 0);
        let mauvais = vec!["--nope".to_string()];
        assert_eq!(cmd_metriques(&dir, &mauvais).code(), 2);
        let _ = std::fs::remove_dir_all(dir.racine());
    }

    #[test]
    fn integrite_classe_ok_ko_sans() {
        let dir = bac("integrite");
        let corps = "Relire le plan";
        let bon = empreinte(corps);
        let lignes = format!(
            "O-1|1000|d|ALL|0|actif|s|{corps} hash={bon}\n\
             O-2|1000|d|ALL|0|actif|s|{corps} hash=faux\n\
             O-3|1000|d|ALL|0|actif|s|Ordre shell nu\n\
             malformée\n"
        );
        std::fs::write(dir.ordres(), lignes).expect("journal");
        assert_eq!(verifie_integrite(&dir), (1, 1, 1));
        let _ = std::fs::remove_dir_all(dir.racine());
    }
}
