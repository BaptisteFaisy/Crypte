//! Ordres broadcast v2 : statuts, TTL, extensions, orchestration.
//!
//! Ordres broadcast v2 : statuts, TTL, extensions, transitions pures.
//!
//! LOT ordres, fichier 1/3 (avec `ack.rs` et `journal.rs`). Lecture à
//! l'identique des formats shell (`ORDRES.log`, `ORDRES-ACK.log`).
//!
//! ASSEMBLEUR :
//! - renommer ce fichier en `ordres.rs` (le socle déclare `pub mod ordres`) ;
//! - ajouter `pub mod ack;` et `pub mod journal;` dans `mod.rs` (ordre
//!   alphabétique : `ack` avant `autorite`, `journal` avant `ordres`) ;
//! - les références `super::ordres` de `ack.rs`/`journal.rs` visent déjà
//!   ce nom final. Aucune dépendance vers `ack`/`journal` depuis ici.
//!
//! Signatures partagées RÉELLES (lues dans le `mod.rs` du socle) :
//! - `super::normalise()` (passe unique de déduplication, jamais
//!   réimplémentée) et `super::sha1_hex()` (40 caractères hexa).
//!
//! Porte jeton : le trait `VerifieJeton` ci-dessous est implémenté par
//! le lot auth pour son type `Autorite` (simple délégation vers sa
//! vérification locale). Révocation et ACK exigent un jeton valide ;
//! lecture et création (côté utilisateur, comme le shell) n'en demandent
//! pas. Graphe sans cycle : `journal` → {`ordres`, `ack`} → `ordres`.
//!
//! TTL : heures, défaut 24, 0 = permanent (miroir de `cmd_ordre new` et
//! `ordre_new` shell), défini une seule fois dans `super::format` et
//! réexporté ici (tranche assembleur : fin des doublons `TTL-SOCLE`).

use super::{normalise, sans_retours, sha1_hex};

pub use super::format::{analyser_ttl, est_expire, TTL_DEFAUT_HEURES};

/// Statut d'un ordre (colonne 6 de `ORDRES.log`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatutOrdre {
    Actif,
    Revoque,
    Expire,
}

impl StatutOrdre {
    /// Parse un statut v2 ; `None` si inconnu (ligne à ignorer).
    pub fn parse(s: &str) -> Option<StatutOrdre> {
        match s {
            "actif" => Some(StatutOrdre::Actif),
            "revoque" => Some(StatutOrdre::Revoque),
            "expire" => Some(StatutOrdre::Expire),
            _ => None,
        }
    }

    /// Forme canonique v2.
    pub fn texte(&self) -> &'static str {
        match self {
            StatutOrdre::Actif => "actif",
            StatutOrdre::Revoque => "revoque",
            StatutOrdre::Expire => "expire",
        }
    }
}

/// Extensions en suffixe du texte : jetons `prio=…`, `supersede=…`,
/// `hash=…`, `sig=…` séparés par des espaces en queue de texte.
/// Le texte brut reste stocké à l'identique ; valeurs verbatim.
#[derive(Debug, Clone, Default)]
pub struct Extensions {
    pub prio: Option<String>,
    pub supersede: Option<String>,
    pub hash: Option<String>,
    pub sig: Option<String>,
}

impl Extensions {
    /// Sépare le corps des extensions lues dans les jetons finaux
    /// `cle=valeur` connus ; tout jeton inconnu reste dans le corps.
    pub fn extraire(texte: &str) -> (String, Extensions) {
        let mut ext = Extensions::default();
        let jetons: Vec<&str> = texte.split_whitespace().collect();
        let mut garde = jetons.len();
        for jeton in jetons.iter().rev() {
            let Some((cle, valeur)) = jeton.split_once('=') else {
                break;
            };
            if valeur.is_empty() {
                break;
            }
            let range = match cle {
                "prio" => &mut ext.prio,
                "supersede" => &mut ext.supersede,
                "hash" => &mut ext.hash,
                "sig" => &mut ext.sig,
                _ => break,
            };
            *range = Some(valeur.to_string());
            garde -= 1;
        }
        if garde == jetons.len() {
            return (texte.to_string(), ext);
        }
        let debut = jetons[garde].as_ptr() as usize - texte.as_ptr() as usize;
        (texte[..debut].trim_end().to_string(), ext)
    }

    /// Reformate les extensions en suffixe canonique (ordre fixe),
    /// chaîne vide si aucune.
    pub fn suffixe(&self) -> String {
        let mut bouts = Vec::new();
        if let Some(v) = &self.prio {
            bouts.push(format!("prio={v}"));
        }
        if let Some(v) = &self.supersede {
            bouts.push(format!("supersede={v}"));
        }
        if let Some(v) = &self.hash {
            bouts.push(format!("hash={v}"));
        }
        if let Some(v) = &self.sig {
            bouts.push(format!("sig={v}"));
        }
        bouts.join(" ")
    }
}

/// Ordre v2 : `id|epoch|date|to|ttl|statut|src|texte` (8 champs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ordre {
    pub id: String,
    pub epoch: u64,
    pub date: String,
    pub dest: String,
    pub ttl_h: u64,
    pub statut: StatutOrdre,
    pub src: String,
    pub texte: String,
}

impl Ordre {
    /// Parse une ligne de `ORDRES.log` à l'identique du shell
    /// (`IFS='|'`, 8 champs ; `None` si malformée).
    pub fn parse_ligne(ligne: &str) -> Option<Ordre> {
        let c: Vec<&str> = ligne.split('|').collect();
        if c.len() != 8 {
            return None;
        }
        Some(Ordre {
            id: c[0].to_string(),
            epoch: c[1].parse().ok()?,
            date: c[2].to_string(),
            dest: c[3].to_string(),
            ttl_h: c[4].parse().ok()?,
            statut: StatutOrdre::parse(c[5])?,
            src: c[6].to_string(),
            texte: c[7].to_string(),
        })
    }

    /// Forme canonique v2, octet pour octet comme le shell.
    pub fn format_ligne(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.id,
            self.epoch,
            self.date,
            self.dest,
            self.ttl_h,
            self.statut.texte(),
            self.src,
            self.texte
        )
    }

    /// Vrai si l'ordre a dépassé son TTL (0 = permanent).
    pub fn est_expire(&self, present: u64) -> bool {
        est_expire(self.epoch, self.ttl_h, present)
    }

    /// Seules transitions permises : actif vers revoque ou expire.
    pub fn peut_passer_a(&self, nouveau: StatutOrdre) -> bool {
        matches!(self.statut, StatutOrdre::Actif)
            && matches!(nouveau, StatutOrdre::Revoque | StatutOrdre::Expire)
    }
}

/// Assainit une création comme `ordre_new` : `|` vers `/`, retours vers
/// espaces, TTL en heures (vide = défaut 24, 0 = permanent).
/// Durcissement : dest/src assainis aussi (le shell n'assainit que le texte).
pub fn valider_nouvel_ordre(
    dest: &str,
    ttl_texte: &str,
    src: &str,
    texte: &str,
) -> Result<(String, u64, String, String), String> {
    let ttl_h = if ttl_texte.trim().is_empty() {
        TTL_DEFAUT_HEURES
    } else {
        analyser_ttl(ttl_texte)?
    };
    let propre = sans_retours(&texte.replace('|', "/"));
    if propre.trim().is_empty() {
        return Err("bcast: texte d'ordre vide".to_string());
    }
    Ok((dest.replace('|', "/"), ttl_h, src.replace('|', "/"), propre))
}

/// Prochain identifiant `O-AAAAMMJJ-NNN` : compte les lignes commençant
/// par `O-<jour>-` (comme `grep -c`), +1, sur 3 chiffres.
/// À appeler SOUS le verrou d'écriture, juste avant l'ajout.
pub fn prochain_id(contenu_log: &str, jour: &str) -> String {
    let prefixe = format!("O-{jour}-");
    let n = contenu_log
        .lines()
        .filter(|l| l.starts_with(&prefixe))
        .count();
    format!("O-{jour}-{:03}", n + 1)
}

/// Passe d'expiration pure (miroir de `ordre_expire_pass`) : marque
/// `expire` les ordres actifs dépassés. L'appelant ne réécrit que si
/// le second retour est vrai (pas de churn, inode intact sinon).
/// Les lignes malformées sont préservées telles quelles.
pub fn passe_expiration(contenu_log: &str, present: u64) -> (String, bool) {
    let mut lignes = Vec::new();
    let mut change = false;
    for ligne in contenu_log.lines() {
        if ligne.is_empty() {
            continue;
        }
        match Ordre::parse_ligne(ligne) {
            Some(mut o) if o.statut == StatutOrdre::Actif && o.est_expire(present) => {
                o.statut = StatutOrdre::Expire;
                change = true;
                lignes.push(o.format_ligne());
            }
            _ => lignes.push(ligne.to_string()),
        }
    }
    let mut out = lignes.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    (out, change)
}

/// Révocation pure (miroir de `ordre_revoque`) : actif vers revoque.
/// La ligne `revoque` du journal des ACK est à la charge de l'appelant.
pub fn revoquer_dans(contenu_log: &str, id: &str) -> Result<String, String> {
    let mut lignes = Vec::new();
    let mut trouve = false;
    for ligne in contenu_log.lines() {
        if ligne.is_empty() {
            continue;
        }
        match Ordre::parse_ligne(ligne) {
            Some(mut o) if o.id == id && o.peut_passer_a(StatutOrdre::Revoque) => {
                o.statut = StatutOrdre::Revoque;
                trouve = true;
                lignes.push(o.format_ligne());
            }
            _ => lignes.push(ligne.to_string()),
        }
    }
    if !trouve {
        return Err(format!("bcast: ordre {id} introuvable ou non actif"));
    }
    let mut out = lignes.join("\n");
    out.push('\n');
    Ok(out)
}

/// Empreinte de dédoublonnage : SHA-1 du texte normalisé (socle).
pub fn empreinte(texte: &str) -> String {
    sha1_hex(normalise(texte).as_bytes())
}

/// Porte jeton du lot auth : tout refus coupe révocation et ACK avant
/// toute écriture. Lecture et création n'exigent aucun jeton.
pub trait VerifieJeton {
    fn verifier_jeton(&self, jeton: &str) -> Result<String, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_et_format_aller_retour() {
        let ligne = "O-20261108-001|1794145579|2026-11-08 14:46|ALL|24|actif|user-direct|Bonjour les agents";
        let o = Ordre::parse_ligne(ligne).expect("parse");
        assert_eq!(o.id, "O-20261108-001");
        assert_eq!(o.epoch, 1794145579);
        assert_eq!(o.ttl_h, 24);
        assert_eq!(o.statut, StatutOrdre::Actif);
        assert_eq!(o.format_ligne(), ligne);
        assert!(Ordre::parse_ligne("pas|assez").is_none());
        assert!(Ordre::parse_ligne("a|b|c|d|e|bidon|g|h").is_none());
    }

    #[test]
    fn validation_assainit_et_defaut() {
        let (d, t, s, x) = valider_nouvel_ordre("ALL", "", "moi", "Faire X").expect("ok");
        assert_eq!(
            (d.as_str(), t, s.as_str(), x.as_str()),
            ("ALL", 24, "moi", "Faire X")
        );
        let (_, t0, _, _) = valider_nouvel_ordre("ALL", "0", "m", "P").expect("perm");
        assert_eq!(t0, 0);
        let (_, _, _, x2) = valider_nouvel_ordre("A|L", "2", "s", "a|b\nc").expect("san");
        assert_eq!(x2, "a/b c");
        assert!(valider_nouvel_ordre("ALL", "x", "m", "P").is_err());
        assert!(valider_nouvel_ordre("ALL", "3", "m", "  ").is_err());
    }

    #[test]
    fn ttl_zero_permanent_sinon_epoque_plus_heures() {
        assert!(!est_expire(1000, 0, u64::MAX));
        assert!(est_expire(1000, 1, 1000 + 3600));
        assert!(!est_expire(1000, 1, 1000 + 3599));
        assert_eq!(TTL_DEFAUT_HEURES, 24);
    }

    #[test]
    fn identifiants_compteur_zero_rempli() {
        let log = "O-20261108-001|1|d|ALL|1|actif|s|t\nO-20261108-002|2|d|ALL|1|actif|s|t\n";
        assert_eq!(prochain_id(log, "20261108"), "O-20261108-003");
        assert_eq!(prochain_id("", "20261108"), "O-20261108-001");
        assert_eq!(prochain_id(log, "20261109"), "O-20261109-001");
    }

    #[test]
    fn expiration_marque_sans_churn() {
        let log = "O-1|1000|d|ALL|1|actif|s|vieux\nO-2|1000|d|ALL|0|actif|s|perm\n";
        let (neuf, change) = passe_expiration(log, 1000 + 3600);
        assert!(change);
        assert!(neuf.contains("|expire|"));
        assert!(neuf.contains("O-2|1000|d|ALL|0|actif|s|perm"));
        let (_, change2) = passe_expiration(&neuf, 1000 + 3600);
        assert!(!change2);
    }

    #[test]
    fn revocation_pure_transition_unique() {
        let log = "O-1|1000|d|ALL|1|actif|s|t\n";
        let neuf = revoquer_dans(log, "O-1").expect("ok");
        assert!(neuf.contains("|revoque|"));
        assert!(revoquer_dans(&neuf, "O-1").is_err());
        assert!(revoquer_dans(log, "O-X").is_err());
        let o = Ordre::parse_ligne(neuf.lines().next().unwrap()).unwrap();
        assert!(!o.peut_passer_a(StatutOrdre::Expire));
        assert!(!o.peut_passer_a(StatutOrdre::Actif));
        let frais = Ordre::parse_ligne(log.lines().next().unwrap()).unwrap();
        assert!(frais.peut_passer_a(StatutOrdre::Revoque));
        assert!(frais.peut_passer_a(StatutOrdre::Expire));
    }

    #[test]
    fn extensions_suffixe_et_empreinte() {
        let (corps, ext) = Extensions::extraire("Faire X prio=P0 hash=abc123");
        assert_eq!(corps, "Faire X");
        assert_eq!(ext.prio.as_deref(), Some("P0"));
        assert_eq!(ext.hash.as_deref(), Some("abc123"));
        assert!(ext.sig.is_none());
        assert_eq!(ext.suffixe(), "prio=P0 hash=abc123");
        let (corps2, ext2) = Extensions::extraire("sans jeton ni cle");
        assert_eq!(corps2, "sans jeton ni cle");
        assert!(ext2.suffixe().is_empty());
        assert_eq!(empreinte("Bonjour"), empreinte("  bonjour  "));
    }
}
