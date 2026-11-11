//! Accusés de réception des ordres v2 + file pull par agent.
//!
//! LOT ordres, fichier 2/3 (avec `ordre.rs` → `ordres.rs` et `journal.rs`).
//! Les références ci-dessous visent déjà `super::ordres` (nom final).
//!
//! ASSEMBLEUR : ajouter `pub mod ack;` en tête des déclarations de `mod.rs`
//! (ordre alphabétique : avant `pub mod autorite;` ; `journal.rs` porte
//! sa propre note pour `pub mod journal;`).
//!
//! Signatures partagées utilisées (fichier 1/3 du présent lot) :
//! - `super::ordres` : `Ordre`, `StatutOrdre`, `Extensions`, `empreinte`,
//!   `VerifieJeton` (dépendance mutuelle `ordres` ↔ `ack`, même crate).
//!
//! Porte jeton : `valider_ack` exige un jeton valide (lot auth, via le
//! trait `VerifieJeton`) ; lecture (parse, file pull) sans jeton.
//! `revoque` est réservé au système (ligne écrite lors d'une révocation).

use super::ordres::{empreinte, Extensions, Ordre, StatutOrdre, VerifieJeton};
use super::sans_retours;

/// Verdict d'ACK (colonne 4 de `ORDRES-ACK.log`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Lu,
    Applique,
    Refuse,
    Revoque,
}

impl Verdict {
    /// Parse les 4 verdicts écrits par le shell.
    pub fn parse(s: &str) -> Option<Verdict> {
        match s {
            "lu" => Some(Verdict::Lu),
            "applique" => Some(Verdict::Applique),
            "refuse" => Some(Verdict::Refuse),
            "revoque" => Some(Verdict::Revoque),
            _ => None,
        }
    }

    /// Forme canonique v2.
    pub fn texte(&self) -> &'static str {
        match self {
            Verdict::Lu => "lu",
            Verdict::Applique => "applique",
            Verdict::Refuse => "refuse",
            Verdict::Revoque => "revoque",
        }
    }

    /// Seuls lu|applique|refuse sont des ACK d'agent ; `revoque` est
    /// réservé au système (ligne écrite lors d'une révocation).
    pub fn est_ack_utilisateur(&self) -> bool {
        !matches!(self, Verdict::Revoque)
    }
}

/// ACK v2 : `id_ordre|epoch|expediteur|verdict|motif` (5 champs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ack {
    pub id_ordre: String,
    pub epoch: u64,
    pub expediteur: String,
    pub verdict: Verdict,
    pub motif: String,
}

impl Ack {
    /// Parse une ligne de `ORDRES-ACK.log` (`None` si malformée).
    pub fn parse_ligne(ligne: &str) -> Option<Ack> {
        let c: Vec<&str> = ligne.split('|').collect();
        if c.len() != 5 {
            return None;
        }
        Some(Ack {
            id_ordre: c[0].to_string(),
            epoch: c[1].parse().ok()?,
            expediteur: c[2].to_string(),
            verdict: Verdict::parse(c[3])?,
            motif: c[4].to_string(),
        })
    }

    /// Forme canonique v2, octet pour octet comme le shell.
    pub fn format_ligne(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            self.id_ordre,
            self.epoch,
            self.expediteur,
            self.verdict.texte(),
            self.motif
        )
    }

    /// Ligne système écrite lors d'une révocation (miroir shell :
    /// `$id|$epoch|bcast|revoque|$motif`, motif assaini).
    pub fn ligne_revoque_systeme(id: &str, epoch: u64, motif: &str) -> String {
        Ack {
            id_ordre: id.to_string(),
            epoch,
            expediteur: "bcast".to_string(),
            verdict: Verdict::Revoque,
            motif: motif.replace('|', "/").replace('\n', " "),
        }
        .format_ligne()
    }
}

/// Valide un ACK d'agent (miroir de `ordre_ack`) : jeton valide exigé,
/// ordre actif, verdict lu|applique|refuse, motif obligatoire si refus.
/// Champs assainis comme le shell (`|` vers `/`, sans retours).
pub fn valider_ack(
    ordre: &Ordre,
    auth: &dyn VerifieJeton,
    jeton: &str,
    expediteur: &str,
    verdict_texte: &str,
    motif: &str,
    epoch: u64,
) -> Result<Ack, String> {
    auth.verifier_jeton(jeton)?;
    let secret = super::signature::secret_du_jeton(jeton);
    let etat = super::signature::etat_signature(ordre, secret);
    if etat == super::signature::EtatSignature::Invalide {
        return Err(format!("bcast: signature {}", etat.texte()));
    }
    let verdict = Verdict::parse(verdict_texte)
        .filter(|v| v.est_ack_utilisateur())
        .ok_or_else(|| "bcast: verdict lu|applique|refuse".to_string())?;
    if verdict == Verdict::Refuse && motif.trim().is_empty() {
        return Err("bcast: refus exige un motif".to_string());
    }
    if ordre.statut != StatutOrdre::Actif {
        return Err(format!(
            "bcast: ordre {} {} (ACK refusé)",
            ordre.id,
            ordre.statut.texte()
        ));
    }
    Ok(Ack {
        id_ordre: ordre.id.clone(),
        epoch,
        expediteur: expediteur.replace('|', "/"),
        verdict,
        motif: sans_retours(&motif.replace('|', "/")),
    })
}

/// File pull d'un agent : ordres actifs qui le visent (`ALL` ou son nom),
/// sans ACK `applique`/`refuse` de sa part (`lu` seul ne solde pas),
/// dédoublonnés par empreinte sur le corps (premier gardé). Les ordres
/// déjà soldés occupent leur slot : un doublon réémis ne réapparaît pas.
pub fn pendants_pour(contenu_ordres: &str, contenu_acks: &str, agent: &str) -> Vec<Ordre> {
    use std::collections::HashSet;
    let mut soldes: HashSet<String> = HashSet::new();
    for ligne in contenu_acks.lines() {
        if let Some(a) = Ack::parse_ligne(ligne) {
            if a.expediteur == agent && matches!(a.verdict, Verdict::Applique | Verdict::Refuse) {
                soldes.insert(a.id_ordre);
            }
        }
    }
    let tous: Vec<Ordre> = contenu_ordres
        .lines()
        .filter_map(Ordre::parse_ligne)
        .collect();
    let vise = |o: &Ordre| o.statut == StatutOrdre::Actif && (o.dest == "ALL" || o.dest == agent);
    let mut empreintes: HashSet<String> = HashSet::new();
    for o in &tous {
        if vise(o) && soldes.contains(&o.id) {
            let (corps, _) = Extensions::extraire(&o.texte);
            empreintes.insert(empreinte(&corps));
        }
    }
    let mut file = Vec::new();
    for o in &tous {
        if !vise(o) || soldes.contains(&o.id) {
            continue;
        }
        let (corps, _) = Extensions::extraire(&o.texte);
        if empreintes.insert(empreinte(&corps)) {
            file.push(o.clone());
        }
    }
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AuthTest;

    impl VerifieJeton for AuthTest {
        fn verifier_jeton(&self, jeton: &str) -> Result<String, String> {
            if jeton == "bon" {
                Ok("agent".to_string())
            } else {
                Err("jeton refuse".to_string())
            }
        }
    }

    fn ordre_actif() -> Ordre {
        Ordre::parse_ligne("O-1|1000|d|ALL|0|actif|s|Faire X").expect("fixe")
    }

    #[test]
    fn ack_aller_retour_ligne_reelle() {
        let ligne = "O-20261108-001|1794145607|muse (broadcast-v2)|applique|-- chaîne P0 vérifiée";
        let a = Ack::parse_ligne(ligne).expect("parse");
        assert_eq!(a.id_ordre, "O-20261108-001");
        assert_eq!(a.verdict, Verdict::Applique);
        assert_eq!(a.format_ligne(), ligne);
        assert!(Ack::parse_ligne("trop|court").is_none());
    }

    #[test]
    fn ligne_revoque_format_systeme() {
        let l = Ack::ligne_revoque_systeme("O-20261108-001", 1794145607, "fin du test");
        assert_eq!(l, "O-20261108-001|1794145607|bcast|revoque|fin du test");
    }

    #[test]
    fn validation_jeton_verdict_motif_statut() {
        let o = ordre_actif();
        let auth = AuthTest;
        let ok = valider_ack(&o, &auth, "bon", "ag", "applique", "", 5).expect("ok");
        assert_eq!(ok.expediteur, "ag");
        assert!(valider_ack(&o, &auth, "faux", "ag", "lu", "", 5).is_err());
        assert!(valider_ack(&o, &auth, "bon", "ag", "revoque", "", 5).is_err());
        assert!(valider_ack(&o, &auth, "bon", "ag", "bidon", "", 5).is_err());
        assert!(valider_ack(&o, &auth, "bon", "ag", "refuse", "", 5).is_err());
        assert!(valider_ack(&o, &auth, "bon", "ag", "refuse", "car X", 5).is_ok());
        let mort = Ordre::parse_ligne("O-1|1000|d|ALL|0|revoque|s|t").expect("fixe");
        assert!(valider_ack(&mort, &auth, "bon", "ag", "lu", "", 5).is_err());
    }

    #[test]
    fn file_pull_filtre_solde_dedup() {
        let ordres = "O-1|1000|d|ALL|0|actif|s|Relire le plan\n\
                      O-2|1000|d|bob|0|actif|s|Pour Bob\n\
                      O-3|1000|d|ALL|0|actif|s|relire le plan\n\
                      O-4|1000|d|ALL|0|revoque|s|Vieux\n";
        let acks = "O-1|1001|alice|lu|\n";
        let file = pendants_pour(ordres, acks, "alice");
        assert_eq!(file.len(), 1, "lu ne solde pas, doublon fusionné");
        assert_eq!(file[0].id, "O-1");
        let acks2 = "O-1|1001|alice|applique|\n";
        assert!(pendants_pour(ordres, acks2, "alice").is_empty());
        let file_bob = pendants_pour(ordres, "", "bob");
        assert_eq!(file_bob.len(), 2, "ALL + nominatif, doublon fusionné");
    }

    #[test]
    fn verdicts_quatre_et_utilisateur() {
        for (t, v) in [
            ("lu", Verdict::Lu),
            ("applique", Verdict::Applique),
            ("refuse", Verdict::Refuse),
        ] {
            let p = Verdict::parse(t).expect("parse");
            assert_eq!(p, v);
            assert_eq!(p.texte(), t);
            assert!(p.est_ack_utilisateur());
        }
        let r = Verdict::parse("revoque").expect("parse");
        assert!(!r.est_ack_utilisateur());
        assert!(Verdict::parse("vu").is_none());
    }
}
