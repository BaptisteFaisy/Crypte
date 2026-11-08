//! Signatures HMAC-SHA1 des ordres : `sig=` VERIFIEE (lot signature).
//!
//! CABLAGE (assembleur) — `pub mod signature;` dans `mod.rs` entre
//! `outils` et `sonde` (ordre alphabétique : `sig` < `son`). Zéro
//! duplication : réutilise `super::{normalise, sha1_hex, BcastDir}`,
//! `super::ordres::{Extensions, Ordre}`, `super::fichier::lire_lignes`.
//! Aucune commande CLI nouvelle : scellement automatique dans `ordre
//! new` (jeton `--token` déjà exigé), vérification automatique dans
//! `ordre ack`. Aucun secret affiché ni journalisé (jamais de `{:?}`
//! ni de `println!` sur `secret`, `jeton` ou `sig`).
//!
//! H1 — `journal.rs` `creer`/`creer_ordre` (+1L utile) : ajouter un
//! paramètre `secret: Option<&str>` aux deux signatures (le secret
//! validé par `porte_jeton`, `None` = pas de sceau), puis DANS le
//! verrou après `let id = prochain_id(&contenu, &jour);` :
//! `let texte_p = super::signature::texte_scelle(secret, &id, &dest_p, &texte_p);`
//! (`Option<&str>` est `Copy`, la fermeture `Fn` le capture sans
//! bouger). Appelants à mettre à jour : `cli_gestion.rs`
//! `ordre_nouveau` (+1L : passer `opt(&opts, "token").and_then(|b|
//! b.rsplit_once(':').map(|(_, s)| s))`), tests `journal.rs` (passer
//! `None` aux deux appels `creer_ordre`). Créations shell (sans
//! jeton) : texte inchangé, pas de `sig=`, interop préservée.
//!
//! H2 — `ack.rs` `valider_ack` (+4L), après `auth.verifier_jeton(jeton)?;` :
//! `let secret = super::signature::secret_du_jeton(jeton);`
//! `if super::signature::etat_signature(ordre, secret) == super::signature::EtatSignature::Invalide {`
//! `    return Err("bcast: signature invalide".to_string());`
//! `}`
//! Sémantique : `sig=` absente (ordres shell) → `Sans` → ACK accepté
//! (interop) ; présente et valide → accepté ; présente et invalide →
//! refus fail-closed. LIMITE CONNUE : la clef est le secret du jeton
//! PRESENTE à l'ACK, donc un ordre scellé par X n'est ackable que par
//! un détenteur du même secret (l'autorité ne ré-expose jamais les
//! secrets, `autorite.rs` : pas de recherche par nom possible).
//!
//! H3 — `metriques.rs` `cmd_metriques` (+2L) : après `let (ok, ko,
//! sans) = verifie_integrite(dir);`, ajouter `let (s_ok, s_ko,
//! s_sans) = super::signature::compte_journal(dir);` et remplacer le
//! `println!` par `println!("integrite ordres: {ok} ok {ko} ko {sans}
//! sans hash, signatures: {s_ok} ok {s_ko} ko {s_sans} sans");`.
//! Comptage STRUCTUREL (présence + forme 40-hex) : `metriques` n'a
//! aucun secret sous la main, la preuve crypto a lieu à l'ACK (H2).

use super::fichier::lire_lignes;
use super::ordres::{Extensions, Ordre};
use super::{normalise, sha1_hex, BcastDir};

/// HMAC-SHA1 (RFC 2104, bloc 64) en hexadécimal, via `sha1_hex` du
/// socle. Clef longue (> 64 o.) hachée d'abord, clef courte bourrée
/// de zéros ; le condensé interne transite par `octets_hex` (jamais
/// en hex dans le tour externe).
pub fn hmac_sha1(secret: &[u8], message: &[u8]) -> String {
    let mut cle = secret.to_vec();
    if cle.len() > 64 {
        cle = octets_hex(&sha1_hex(&cle));
    }
    cle.resize(64, 0);
    let mut dedans = Vec::with_capacity(64 + message.len());
    let mut dehors = Vec::with_capacity(64 + 20);
    for octet in &cle {
        dedans.push(octet ^ 0x36);
        dehors.push(octet ^ 0x5c);
    }
    dedans.extend_from_slice(message);
    dehors.extend_from_slice(&octets_hex(&sha1_hex(&dedans)));
    sha1_hex(&dehors)
}

/// Décode 40 hex en 20 octets (nibbles invalides = 0 ; en pratique
/// ne reçoit que des sorties de `sha1_hex`, toujours valides).
fn octets_hex(hex: &str) -> Vec<u8> {
    let chiffre = |c: u8| match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    };
    hex.as_bytes()
        .chunks(2)
        .map(|p| {
            if p.len() == 2 {
                chiffre(p[0]) * 16 + chiffre(p[1])
            } else {
                0
            }
        })
        .collect()
}

/// Scelle un corps : HMAC-SHA1 du couple `(secret, corps)`, 40 hex.
pub fn scelle(secret: &str, corps: &str) -> String {
    hmac_sha1(secret.as_bytes(), corps.as_bytes())
}

/// Vérifie un sceau : recalcule puis compare sans court-circuit
/// (calcul toujours exécuté, même si `sig` est mal formée : pas
/// d'oracle sur la forme). `false` si longueurs différentes.
pub fn verifie(secret: &str, corps: &str, sig: &str) -> bool {
    comparaison_constante(&scelle(secret, corps), sig)
}

/// Comparaison sans court-circuit, octet à octet (miroir local :
/// celle d'`autorite` est privée et liée au fichier des jetons).
fn comparaison_constante(a: &str, b: &str) -> bool {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    if x.len() != y.len() {
        return false;
    }
    let mut ecart = 0u8;
    for i in 0..x.len() {
        ecart |= x[i] ^ y[i];
    }
    ecart == 0
}

/// Corps signé : `id|dest|texte normalisé` (même `normalise` que le
/// `hash=` via `empreinte` : dates/heures/casse neutralisées). Le
/// texte est le corps SANS extensions (`Extensions::extraire`).
pub fn corps_message(id: &str, dest: &str, texte: &str) -> String {
    format!("{}|{}|{}", id, dest, normalise(texte))
}

/// Issue du contrôle d'un sceau : valide, invalide, ou absent
/// (ordres shell créés sans jeton : interop, pas un refus).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EtatSignature {
    Valide,
    Invalide,
    Sans,
}

impl EtatSignature {
    /// Forme canonique affichable (`sans-signature`, jamais `sans`).
    pub fn texte(&self) -> &'static str {
        match self {
            EtatSignature::Valide => "valide",
            EtatSignature::Invalide => "invalide",
            EtatSignature::Sans => "sans-signature",
        }
    }
}

/// Contrôle le `sig=` d'un ordre avec un secret présenté : `Sans`
/// si absent (interop shell), sinon `Valide`/`Invalide` crypto.
pub fn etat_signature(ordre: &Ordre, secret: &str) -> EtatSignature {
    let (corps, ext) = Extensions::extraire(&ordre.texte);
    match ext.sig {
        None => EtatSignature::Sans,
        Some(sig) => {
            let corps_msg = corps_message(&ordre.id, &ordre.dest, &corps);
            if verifie(secret, &corps_msg, &sig) {
                EtatSignature::Valide
            } else {
                EtatSignature::Invalide
            }
        }
    }
}

/// Secret d'un jeton `nom:secret` (découpe au dernier `:`, comme
/// `outils::porte_jeton`) ; `""` si malformé (vérif invalide, pas
/// de panique ; le jeton lui-même est déjà validé par la porte).
pub fn secret_du_jeton(jeton: &str) -> &str {
    jeton.rsplit_once(':').map(|(_, s)| s).unwrap_or("")
}

/// Scelle un texte d'ordre déjà suffixé (`hash=` posé par `creer_ordre`)
/// : `None`/vide = texte inchangé (création shell/sans jeton), sinon
/// `sig=` ajoutée au suffixe canonique (`hash=` préservé devant).
pub fn texte_scelle(secret: Option<&str>, id: &str, dest: &str, texte: &str) -> String {
    let Some(secret) = secret.filter(|s| !s.is_empty()) else {
        return texte.to_string();
    };
    let (corps, mut ext) = Extensions::extraire(texte);
    ext.sig = Some(scelle(secret, &corps_message(id, dest, &corps)));
    let suffixe = ext.suffixe();
    format!("{corps} {suffixe}").trim_start().to_string()
}

/// `sig=` bien formée : 40 hex (casse indifférente ici ; la preuve
/// crypto reste exacte dans `verifie`, jamais assouplie).
fn bien_formee(sig: &str) -> bool {
    sig.len() == 40 && sig.bytes().all(|o| o.is_ascii_hexdigit())
}

/// Compte structurel `(ok, ko, sans)` sur un journal : `ok` = `sig=`
/// présente et bien formée, `ko` = présente mais mal formée, `sans`
/// = absente ; lignes malformées ignorées (miroir `verifie_integrite`).
pub fn compte_signatures(contenu_ordres: &str) -> (usize, usize, usize) {
    let (mut ok, mut ko, mut sans) = (0, 0, 0);
    for ligne in contenu_ordres.lines() {
        let Some(ordre) = Ordre::parse_ligne(ligne) else {
            continue;
        };
        match Extensions::extraire(&ordre.texte).1.sig {
            None => sans += 1,
            Some(s) if bien_formee(&s) => ok += 1,
            Some(_) => ko += 1,
        }
    }
    (ok, ko, sans)
}

/// Compte structurel `(ok, ko, sans)` sur `ORDRES.log` (fail-open :
/// journal absent = `(0, 0, 0)`), hook d'une ligne pour `metriques`.
pub fn compte_journal(dir: &BcastDir) -> (usize, usize, usize) {
    compte_signatures(&lire_lignes(&dir.ordres()).join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ordre factice minimal autour d'un texte (`id`/`dest` fixés).
    fn fabrique_ordre(texte: &str) -> Ordre {
        Ordre::parse_ligne(&format!(
            "O-20261108-001|1794145579|2026-11-08 14:46|ALL|24|actif|t|{texte}"
        ))
        .expect("ligne factice valide")
    }

    #[test]
    fn vecteurs_hmac_conformnes_oracle() {
        assert_eq!(
            hmac_sha1(b"key", b"The quick brown fox jumps over the lazy dog"),
            "de7c9b85b8b78aa6bc8a7a36f70a90701c9db4d9"
        );
        assert_eq!(
            hmac_sha1(&[b'x'; 100], "vecteur cle longue".as_bytes()),
            "7af85358d38617b7077a2303d2cf44389fa614fb"
        );
    }

    #[test]
    fn falsification_refusee() {
        let corps = corps_message("O-1", "ALL", "Relire le plan");
        let sig = scelle("secret-test", &corps);
        assert!(verifie("secret-test", &corps, &sig));
        assert!(!verifie("mauvais-secret", &corps, &sig));
        let change = corps_message("O-1", "ALL", "Relire le plan demain");
        assert!(!verifie("secret-test", &change, &sig));
        let bouge = corps_message("O-2", "ALL", "Relire le plan");
        assert!(!verifie("secret-test", &bouge, &sig));
        assert!(!verifie(
            "secret-test",
            &corps,
            "de7c9b85b8b78aa6bc8a7a36f70a90701c9db4d9"
        ));
        assert!(!verifie("secret-test", &corps, ""));
        assert!(!verifie("secret-test", &corps, &sig[..39]));
    }

    #[test]
    fn interop_shell_sans_sig_acceptee() {
        let ordre = fabrique_ordre("Faire X");
        assert_eq!(etat_signature(&ordre, ""), EtatSignature::Sans);
        assert_eq!(
            etat_signature(&ordre, "n-importe-quel-secret"),
            EtatSignature::Sans
        );
        assert_eq!(EtatSignature::Sans.texte(), "sans-signature");
        assert_eq!(texte_scelle(None, "O-1", "ALL", "Faire X"), "Faire X");
        assert_eq!(texte_scelle(Some(""), "O-1", "ALL", "Faire X"), "Faire X");
        assert_eq!(secret_du_jeton("agent:s3cr3t"), "s3cr3t");
        assert_eq!(secret_du_jeton("malformé"), "");
    }

    #[test]
    fn cycle_sceau_valide() {
        let scelle_texte = texte_scelle(
            Some("sceau-test"),
            "O-20261108-001",
            "ALL",
            "Relire le plan",
        );
        let (corps, ext) = Extensions::extraire(&scelle_texte);
        assert_eq!(corps, "Relire le plan");
        let sig = ext.sig.expect("sig posée");
        assert!(bien_formee(&sig));
        let ordre = fabrique_ordre(&scelle_texte);
        assert_eq!(etat_signature(&ordre, "sceau-test"), EtatSignature::Valide);
        assert_eq!(
            etat_signature(&ordre, "autre-secret"),
            EtatSignature::Invalide
        );
        assert_eq!(EtatSignature::Valide.texte(), "valide");
        assert_eq!(EtatSignature::Invalide.texte(), "invalide");
    }

    #[test]
    fn compte_journal_lit_ordres_log() {
        let racine = std::env::temp_dir().join(format!("mbox-signature-{}", std::process::id()));
        std::fs::create_dir_all(&racine).expect("bac factice");
        let dir = BcastDir::depuis(racine.clone());
        let bon = texte_scelle(Some("s"), "O-1", "ALL", "Corps signé");
        let journal = format!("O-1|1000|d|ALL|0|actif|s|{bon}\nO-2|1000|d|ALL|0|actif|s|Nu\n");
        std::fs::write(dir.ordres(), journal).expect("journal factice");
        assert_eq!(compte_journal(&dir), (1, 0, 1));
        assert_eq!(
            compte_journal(&BcastDir::depuis(racine.join("absent"))),
            (0, 0, 0)
        );
        let _ = std::fs::remove_dir_all(&racine);
    }

    #[test]
    fn comptage_structurel_classe_ok_ko_sans() {
        let bon = texte_scelle(Some("s"), "O-1", "ALL", "Corps signé");
        let journal = format!(
            "O-1|1000|d|ALL|0|actif|s|{bon}\n\
             O-2|1000|d|ALL|0|actif|s|Corps bricolé sig=xyz\n\
             O-3|1000|d|ALL|0|actif|s|Ordre shell nu\n\
             malformée\n"
        );
        assert_eq!(compte_signatures(&journal), (1, 1, 1));
    }
}
