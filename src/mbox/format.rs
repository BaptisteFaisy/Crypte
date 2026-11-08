//! Grammaire des lignes v2 + TTL + identités (socle, écrit par l'assembleur).
//!
//! Aucun lot ne l'a livré : il manquait à `throttle`, `autorite`, `cli`,
//! `cli_gestion` et `sonde` (voir `CABLAGE.md` P4 du lot watch). Miroirs
//! shell : `line_is_msg`, `line_is_cont`, `line_priority`, `build_line`,
//! `wrap_body`, `bcast_sender` (`lib-common.sh`), TTL (`lib-ordres.sh`).
//!
//! Les réexportations ci-dessous servent les lots qui attendaient ces noms
//! sous `format::` (`BcastDir`, `epoque`…) : aucune logique dupliquée, le
//! socle reste défini une seule fois dans `super` (`mod.rs`).
//!
//! `normalise_emetteur` (identité `--from`, `bcast_sender`) ne doit pas être
//! confondu avec `super::normalise` (passe de déduplication des corps).

pub use super::{epoque, maintenant, sanitize, sha1_hex, BcastDir, Sortie, SUJETS_BUS};

/// TTL par défaut, en heures (`cmd_ordre new`, 0 = permanent).
pub const TTL_DEFAUT_HEURES: u64 = 24;

/// Priorité d'un message v2 (inbox : P0 popup+son, P1 popup, P2 log seul).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priorite {
    P0,
    P1,
    P2,
}

/// Valide une identité d'expéditeur (miroir de `bcast_sender`) : non vide,
/// sans saut de ligne ni ` : `, 80 caractères max. Sans rognage (le shell
/// n'en fait pas).
pub fn normalise_emetteur(identite: &str) -> anyhow::Result<String> {
    if identite.is_empty() {
        anyhow::bail!("identité requise (--from ou BCAST_SENDER)");
    }
    if identite.contains('\n') || identite.contains(" : ") {
        anyhow::bail!("identité invalide (saut de ligne ou ' : ')");
    }
    if identite.chars().count() > 80 {
        anyhow::bail!("identité trop longue (80 max)");
    }
    Ok(identite.to_string())
}

/// Parse un TTL CLI en heures : chiffres seuls (espaces rognés),
/// 0 = permanent. Erreur préfixée `bcast:`, comme le shell.
pub fn analyser_ttl(texte: &str) -> Result<u64, String> {
    let tasse = texte.trim();
    if tasse.is_empty() || !tasse.bytes().all(|o| o.is_ascii_digit()) {
        return Err("bcast: ttl invalide (heures, 0=permanent)".to_string());
    }
    tasse
        .parse()
        .map_err(|_| "bcast: ttl invalide (heures, 0=permanent)".to_string())
}

/// Vrai si un élément créé à `debut` avec `ttl_h` heures est dépassé à
/// `present` : 0 = permanent, jamais expiré (miroir de `ordre_expire_pass`).
pub fn est_expire(debut: u64, ttl_h: u64, present: u64) -> bool {
    if ttl_h == 0 {
        return false;
    }
    debut.saturating_add(ttl_h.saturating_mul(3600)) <= present
}

/// Construit une ligne v2 complète (miroir de `build_line`) : destination
/// brute sans crochets, absente si vide, tampon d'attribution en queue.
pub fn construire_ligne(expediteur: &str, priorite: Priorite, dest: &str, message: &str) -> String {
    let etiquette = match priorite {
        Priorite::P0 => "P0",
        Priorite::P1 => "P1",
        Priorite::P2 => "P2",
    };
    let mut tete = format!("- [{}] {expediteur} [{etiquette}]", super::maintenant());
    if !dest.is_empty() {
        tete.push(' ');
        tete.push_str(dest);
    }
    format!("{tete} : {message} {}", super::tampon())
}

/// Découpe un corps multi-ligne (miroir de `wrap_body`) : première ligne
/// brute, suivantes indentées de deux espaces (continuations).
pub fn decouper_corps(corps: &str) -> Vec<String> {
    corps
        .lines()
        .enumerate()
        .map(|(i, ligne)| {
            if i == 0 {
                ligne.to_string()
            } else {
                format!("  {ligne}")
            }
        })
        .collect()
}

/// Ligne message v2 stricte ou legacy (miroir de `line_is_msg`) :
/// `- [AAAA-MM-JJ HH:MM] …` contenant ` : `.
pub fn est_message(ligne: &str) -> bool {
    let o = ligne.as_bytes();
    if o.len() < 20 || o[0] != b'-' || o[1] != b' ' || o[2] != b'[' || o[19] != b']' {
        return false;
    }
    let chiffres = |i: usize| o[i].is_ascii_digit();
    let date_ok = chiffres(3)
        && chiffres(4)
        && chiffres(5)
        && chiffres(6)
        && o[7] == b'-'
        && chiffres(8)
        && chiffres(9)
        && o[10] == b'-'
        && chiffres(11)
        && chiffres(12)
        && o[13] == b' '
        && chiffres(14)
        && chiffres(15)
        && o[16] == b':'
        && chiffres(17)
        && chiffres(18);
    date_ok && ligne.contains(" : ")
}

/// Continuation multi-ligne (miroir de `line_is_cont`) : indentée d'au
/// moins une espace, jamais nue ni blanche.
pub fn est_suite(ligne: &str) -> bool {
    let reste = ligne.trim_start_matches(' ');
    reste.len() < ligne.len() && !reste.is_empty() && !reste.starts_with(char::is_whitespace)
}

/// Priorité d'une ligne (miroir de `line_priority`) : premier tag `[Pn]`,
/// sinon P1 (legacy sans tag).
pub fn priorite_ligne(ligne: &str) -> Priorite {
    let octets = ligne.as_bytes();
    let mut i = 0;
    while i + 4 <= octets.len() {
        if octets[i] == b'[' && octets[i + 1] == b'P' && octets[i + 3] == b']' {
            match octets[i + 2] {
                b'0' => return Priorite::P0,
                b'1' => return Priorite::P1,
                b'2' => return Priorite::P2,
                _ => {}
            }
        }
        i += 1;
    }
    Priorite::P1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emetteur_miroir_shell() {
        assert_eq!(normalise_emetteur("muse").unwrap(), "muse");
        assert!(normalise_emetteur("").is_err());
        assert!(normalise_emetteur("a\nb").is_err());
        assert!(normalise_emetteur("a : b").is_err());
        assert!(normalise_emetteur(&"x".repeat(80)).is_ok());
        assert!(normalise_emetteur(&"x".repeat(81)).is_err());
    }

    #[test]
    fn ttl_heures_zero_permanent() {
        assert_eq!(analyser_ttl("24").unwrap(), 24);
        assert_eq!(analyser_ttl(" 0 ").unwrap(), 0);
        assert!(analyser_ttl("").is_err());
        assert!(analyser_ttl("2h").is_err());
        assert!(!est_expire(1000, 0, u64::MAX));
        assert!(est_expire(1000, 1, 1000 + 3600));
        assert!(!est_expire(1000, 1, 1000 + 3599));
        assert_eq!(TTL_DEFAUT_HEURES, 24);
    }

    #[test]
    fn ligne_construite_passe_le_detecteur() {
        let ligne = construire_ligne("exp", Priorite::P1, "", "bonjour");
        assert!(est_message(&ligne), "ligne nue : {ligne}");
        assert!(ligne.contains(" : bonjour "), "forme : {ligne}");
        assert!(!ligne.contains("  : "), "dest vide = nue : {ligne}");
        let bus = construire_ligne("exp", Priorite::P2, "@gpu", "chaud");
        assert!(bus.contains("@gpu") && bus.contains("[P2]"), "bus : {bus}");
    }

    #[test]
    fn corps_decoupe_premiere_brute() {
        assert_eq!(decouper_corps("un"), vec!["un"]);
        assert_eq!(decouper_corps("un\ndeux"), vec!["un", "  deux"]);
        assert!(decouper_corps("").is_empty());
    }

    #[test]
    fn detecteurs_messages_et_suites() {
        assert!(est_message("- [2026-11-08 10:00] x [P1] : un {#p}"));
        assert!(est_message("- [2026-11-08 10:01] y : deux {#p}"));
        assert!(!est_message("  suite indentée"));
        assert!(!est_message("- [2026-11-08] sans heure : x"));
        assert!(!est_message(""));
        assert!(est_suite("  suite"));
        assert!(!est_suite(""));
        assert!(!est_suite("  "));
        assert!(!est_suite("- [2026-11-08 10:00] x : y"));
    }

    #[test]
    fn priorite_premier_tag_sinon_p1() {
        assert_eq!(priorite_ligne("- [d] x [P0] : a"), Priorite::P0);
        assert_eq!(priorite_ligne("- [d] x [P2] : a"), Priorite::P2);
        assert_eq!(priorite_ligne("- [d] x : legacy"), Priorite::P1);
        assert_eq!(priorite_ligne("x [P2] y [P0]"), Priorite::P2);
    }
}
