//! Piste d'audit locale des agents (vague 4, D5 tracing 100 % LOCAL).
//!
//! Zéro export réseau par construction : les spans sont ajoutés au fichier
//! `piste.log` (racine du dossier broadcast), jamais envoyés ailleurs.
//! Format : `run_id|epoque|agent|action|ordre_id|verdict` (6 champs).
//! Corrélation par `ordre_id` ; `run_id` = 12 hexa stables par processus.
//! Jamais de secret dans les spans : agent assaini (`sanitize`), pas de
//! texte d'ordre, pas de jeton (seul le nom avant `:` est traçable).
//!
//! SPEC DE CÂBLAGE (assembleur) :
//! - `mod.rs` : ajouter `pub mod piste;` (alpha : entre `outils` et `sonde`).
//! - `journal.rs` (396 L / 15 fn, budget OK), 3 hooks x 1 L via `emet` :
//!   1. `creer` : `let out = creer_ordre(...).map_err(|_| Sortie::ErreurExecution);`
//!      + `if let Ok(id) = &out { super::piste::emet(dirs, src, super::piste::Action::OrdreCree, id, "ok"); }` + `out`
//!   2. `revoquer` : `let out = revoquer_journal(...).map_err(|_| Sortie::ErreurExecution);`
//!      + `if out.is_ok() { super::piste::emet(dirs, super::piste::nom_depuis_jeton(jeton), super::piste::Action::OrdreRevoque, id, "revoque"); }` + `out`
//!   3. `ack` : `let out = append_verrouille(...).map_err(|_| Sortie::ErreurExecution);`
//!      + `if out.is_ok() { super::piste::emet(dirs, expediteur, super::piste::Action::OrdreAck, id, verdict_texte); }` + `out`
//! - `docteur.rs` (150 L) : 1 L après la ligne métriques :
//!   `println!("OK {}", super::piste::resume(dir));`
//! - `cli.rs` (489 L, budget hooks 5 L) : 1 L dispatch
//!   `"piste" => super::piste::cmd_piste(&dir, reste),`
//!   + 1 L d'aide (`piste [--ordre ID] [--agent ID]`).
//! - Futur (hors lot) : `envoi` depuis `cmd_send`/`cmd_bus`, `delegation`
//!   depuis le routeur, `jeton_emis` depuis `autorite::emettre` — mêmes
//!   appels `emet` 1 L, quand les budgets le permettront.
//!
//! Réutilise le socle, zéro duplication : `BcastDir`, `epoque`, `sanitize`,
//! `sha1_hex`, `Sortie` (`format`), `lire_lignes` (`fichier`), `decoupage` +
//! `opt` (`outils`), `verrouiller` + `chemin_verrou` + `EnCasDEchec`
//! (`verrou`, patron fail-open de `metriques::incremente_cle`).

use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use super::fichier::lire_lignes;
use super::format::{epoque, sanitize, sha1_hex, BcastDir, Sortie};
use super::outils::{decoupage, opt};
use super::verrou::{chemin_verrou, verrouiller, EnCasDEchec};

/// Action traçable : 6 valeurs fermées, corrélation par `ordre_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Envoi,
    OrdreCree,
    OrdreAck,
    OrdreRevoque,
    Delegation,
    JetonEmis,
}

impl Action {
    /// Parse une action ; `None` si inconnue (span à ignorer).
    pub fn parse(s: &str) -> Option<Action> {
        match s {
            "envoi" => Some(Action::Envoi),
            "ordre_cree" => Some(Action::OrdreCree),
            "ordre_ack" => Some(Action::OrdreAck),
            "ordre_revoque" => Some(Action::OrdreRevoque),
            "delegation" => Some(Action::Delegation),
            "jeton_emis" => Some(Action::JetonEmis),
            _ => None,
        }
    }

    /// Forme canonique stockée dans `piste.log`.
    pub fn texte(&self) -> &'static str {
        match self {
            Action::Envoi => "envoi",
            Action::OrdreCree => "ordre_cree",
            Action::OrdreAck => "ordre_ack",
            Action::OrdreRevoque => "ordre_revoque",
            Action::Delegation => "delegation",
            Action::JetonEmis => "jeton_emis",
        }
    }
}

/// Span local : `run_id|epoque|agent|action|ordre_id|verdict`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub run_id: String,
    pub epoch: u64,
    pub agent: String,
    pub action: Action,
    pub ordre_id: String,
    pub verdict: String,
}

impl Span {
    /// Parse un span (`None` si malformé : lignes étrangères ignorées).
    pub fn parse_ligne(ligne: &str) -> Option<Span> {
        let c: Vec<&str> = ligne.split('|').collect();
        if c.len() != 6 {
            return None;
        }
        Some(Span {
            run_id: c[0].to_string(),
            epoch: c[1].parse().ok()?,
            agent: c[2].to_string(),
            action: Action::parse(c[3])?,
            ordre_id: c[4].to_string(),
            verdict: c[5].to_string(),
        })
    }

    /// Sérialise au format 6 champs (sans retour ligne).
    pub fn format_ligne(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}",
            self.run_id,
            self.epoch,
            self.agent,
            self.action.texte(),
            self.ordre_id,
            self.verdict
        )
    }
}

/// Fichier des spans (racine, comme `ORDRES.log` : lisible au shell).
fn chemin(dir: &BcastDir) -> PathBuf {
    dir.joindre("piste.log")
}

/// Identifiant de run : 12 hexa stables par processus (pid + époque de
/// premier appel, hachés au `sha1_hex` du socle). Aucun secret dedans.
fn run_id() -> String {
    static BOOT: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    let boot = *BOOT.get_or_init(epoque);
    let brut = format!("{}-{boot}", std::process::id());
    sha1_hex(brut.as_bytes())[..12].to_string()
}

/// Nettoie un champ libre : sans `|` ni retour ligne, tronqué à `n`.
/// Garde l'`ordre_id` corrélable en clair (un id n'est pas un secret).
fn champ(texte: &str, n: usize) -> String {
    texte
        .replace(['|', '\n', '\r'], "_")
        .chars()
        .take(n)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Nom traçable d'un jeton `nom:secret` : le nom seul, jamais le secret.
/// Sans séparateur ou nom vide : `?` (aucune fuite par construction).
pub fn nom_depuis_jeton(jeton: &str) -> &str {
    match jeton.rsplit_once(':') {
        Some((nom, secret)) if !nom.is_empty() && !secret.is_empty() => nom,
        _ => "?",
    }
}

/// Émet un span, best-effort fail-open : verrou impossible ou écriture
/// manquée = span perdu, silence (patron `metriques::incremente_cle`).
/// L'agent est assaini (`sanitize`, jamais le secret) ; `ordre_id` et
/// verdict sont nettoyés (`champ`) pour garder le format 6 champs.
pub fn emet(dir: &BcastDir, agent: &str, action: Action, ordre_id: &str, verdict: &str) {
    let cible = chemin(dir);
    let Ok(Some(_garde)) = verrouiller(&chemin_verrou(&cible), 10, EnCasDEchec::FailOpen) else {
        return;
    };
    let span = Span {
        run_id: run_id(),
        epoch: epoque(),
        agent: sanitize(agent),
        action,
        ordre_id: champ(ordre_id, 80),
        verdict: champ(verdict, 80),
    };
    let mut f = match OpenOptions::new().create(true).append(true).open(&cible) {
        Ok(f) => f,
        Err(_) => return,
    };
    let _ = writeln!(f, "{}", span.format_ligne());
}

/// Émet un span depuis un jeton brut : seul le nom est tracé, jamais
/// le secret (`nom_depuis_jeton` + `emet`, hook d'une ligne pour la CLI).
pub fn emet_jeton(dir: &BcastDir, jeton: &str, action: Action, ordre_id: &str, verdict: &str) {
    emet(dir, nom_depuis_jeton(jeton), action, ordre_id, verdict);
}

/// Lit les spans (fail-open : absent ou malformé = partiel ou vide).
pub fn lire(dir: &BcastDir) -> Vec<Span> {
    lire_lignes(&chemin(dir))
        .iter()
        .filter_map(|l| Span::parse_ligne(l))
        .collect()
}

/// Ligne unique pour `doctor` : que des comptes, jamais de secret.
pub fn resume(dir: &BcastDir) -> String {
    let spans = lire(dir);
    let ordres: BTreeSet<&str> = spans
        .iter()
        .map(|s| s.ordre_id.as_str())
        .filter(|o| !o.is_empty() && *o != "-")
        .collect();
    format!("piste {} spans {} ordres traces", spans.len(), ordres.len())
}

/// `piste [--ordre ID] [--agent ID]` : spans corrélés, chronologiques.
pub fn cmd_piste(dir: &BcastDir, args: &[String]) -> Sortie {
    let Ok((opts, _)) = decoupage(args, &[("ordre", true), ("agent", true)]) else {
        return Sortie::Usage;
    };
    let filtre_ordre = opt(&opts, "ordre").map(|o| champ(o, 80));
    let filtre_agent = opt(&opts, "agent").map(sanitize);
    let mut n = 0;
    for span in lire(dir) {
        if filtre_ordre.as_deref().is_some_and(|f| f != span.ordre_id) {
            continue;
        }
        if filtre_agent.as_deref().is_some_and(|f| f != span.agent) {
            continue;
        }
        println!("{}", span.format_ligne());
        n += 1;
    }
    if n == 0 {
        println!("aucune trace");
    }
    Sortie::Ok
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bac jetable unique par test (jamais le vrai dossier broadcast).
    fn bac(tag: &str) -> BcastDir {
        let dir = std::env::temp_dir().join(format!("mbox-piste-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("bac");
        BcastDir::depuis(dir)
    }

    #[test]
    fn vocabulaire_ferme_des_actions() {
        let attendues = [
            (Action::Envoi, "envoi"),
            (Action::OrdreCree, "ordre_cree"),
            (Action::OrdreAck, "ordre_ack"),
            (Action::OrdreRevoque, "ordre_revoque"),
            (Action::Delegation, "delegation"),
            (Action::JetonEmis, "jeton_emis"),
        ];
        for (action, texte) in attendues {
            assert_eq!(action.texte(), texte);
            assert_eq!(Action::parse(texte), Some(action));
        }
        assert_eq!(Action::parse("rm -rf"), None);
        assert_eq!(Action::parse("ENVOI"), None);
    }

    #[test]
    fn emission_et_relecture_correlable() {
        let dir = bac("cycle");
        emet(&dir, "muse", Action::OrdreCree, "O-20261108-001", "ok");
        emet(&dir, "muse", Action::OrdreAck, "O-20261108-001", "applique");
        emet(&dir, "b", Action::OrdreRevoque, "O-20261108-001", "revoque");
        let spans = lire(&dir);
        assert_eq!(spans.len(), 3);
        assert!(spans.iter().all(|s| s.ordre_id == "O-20261108-001"));
        assert_eq!(spans[0].action, Action::OrdreCree);
        assert_eq!(spans[1].verdict, "applique");
        assert_eq!(spans[0].agent, sanitize("muse"));
        assert_eq!(spans[0].run_id, spans[1].run_id);
        assert_eq!(spans[0].run_id.len(), 12);
        assert!(chemin(&dir).is_file());
        assert!(resume(&dir).contains("3 spans"));
        assert!(resume(&dir).contains("1 ordres traces"));
        let _ = std::fs::remove_dir_all(dir.racine());
    }

    #[test]
    fn lecture_tolerante_aux_malformees() {
        let dir = bac("tolere");
        std::fs::write(
            chemin(&dir),
            "r1|1000|ag|ordre_cree|O-1|ok\npourri\nr2|abc|ag|ordre_cree|O-2|ok\nr3|1000|ag|inconnu|O-3|ok\n",
        )
        .expect("seed");
        let spans = lire(&dir);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].ordre_id, "O-1");
        let _ = std::fs::remove_dir_all(dir.racine());
    }

    #[test]
    fn champs_dangereux_ne_cassent_pas_le_format() {
        let dir = bac("champs");
        emet(
            &dir,
            "muse",
            Action::Envoi,
            "O-1|injecte\nligne",
            "ok|fuite\nx",
        );
        let spans = lire(&dir);
        assert_eq!(spans.len(), 1);
        assert!(!spans[0].ordre_id.contains('|'));
        assert!(!spans[0].verdict.contains('\n'));
        let brut = std::fs::read_to_string(chemin(&dir)).expect("brut");
        assert_eq!(brut.lines().count(), 1);
        let _ = std::fs::remove_dir_all(dir.racine());
    }

    #[test]
    fn nom_depuis_jeton_ne_fuit_jamais_le_secret() {
        assert_eq!(nom_depuis_jeton("cli-t:s3cret"), "cli-t");
        assert_eq!(nom_depuis_jeton("n:v:avec:colons"), "n:v:avec");
        assert_eq!(nom_depuis_jeton("sans-separateur"), "?");
        assert_eq!(nom_depuis_jeton(":vide"), "?");
        assert_eq!(nom_depuis_jeton("vide:"), "?");
        assert_eq!(nom_depuis_jeton(""), "?");
        let dir = bac("jeton");
        emet(
            &dir,
            nom_depuis_jeton("cli-t:s3cret"),
            Action::JetonEmis,
            "-",
            "ok",
        );
        let brut = std::fs::read_to_string(chemin(&dir)).expect("brut");
        assert!(!brut.contains("s3cret"));
        emet_jeton(&dir, "cli-t:s3cret", Action::OrdreCree, "O-9", "ok");
        let spans = lire(&dir);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[1].agent, sanitize("cli-t"));
        let _ = std::fs::remove_dir_all(dir.racine());
    }

    #[test]
    fn commande_filtre_par_ordre_et_agent() {
        let dir = bac("cmd");
        emet(&dir, "muse", Action::OrdreCree, "O-1", "ok");
        emet(&dir, "b", Action::OrdreAck, "O-2", "lu");
        assert_eq!(cmd_piste(&dir, &[]).code(), 0);
        let par_ordre = vec!["--ordre".to_string(), "O-2".to_string()];
        assert_eq!(cmd_piste(&dir, &par_ordre).code(), 0);
        let par_agent = vec!["--agent".to_string(), "b".to_string()];
        assert_eq!(cmd_piste(&dir, &par_agent).code(), 0);
        let mauvais = vec!["--nope".to_string()];
        assert_eq!(cmd_piste(&dir, &mauvais).code(), 2);
        let _ = std::fs::remove_dir_all(dir.racine());
    }

    #[test]
    fn piste_vide_et_resume_zero() {
        let dir = bac("vide");
        assert!(lire(&dir).is_empty());
        assert_eq!(resume(&dir), "piste 0 spans 0 ordres traces");
        assert_eq!(cmd_piste(&dir, &[]).code(), 0);
        let _ = std::fs::remove_dir_all(dir.racine());
    }
}
