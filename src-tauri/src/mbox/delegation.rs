//! Délégation agent → agent à portée MINIMALE (D3, vague 4).
//!
//! Un transfert ne joint JAMAIS tout l'historique : uniquement un résumé
//! explicite rédigé par l'émetteur + des pièces allowlistées (liste
//! explicite, ≤ 8 fichiers, taille cappée, scellées par SHA-1). Seules les
//! métadonnées des pièces voyagent (chemin, taille, SHA-1), jamais leur
//! contenu : le destinataire relit les fichiers lui-même.
//!
//! Réutilisations (zéro duplication, symboles existants lus en place) :
//! - `super::ordres::{Ordre, Extensions, StatutOrdre, TTL_DEFAUT_HEURES,
//!   VerifieJeton}` (`ordres.rs` : parse, extensions, TTL 24 h) ;
//! - `super::ack::Verdict` (`ack.rs` : `parse`, `est_ack_utilisateur` ; le
//!   destinataire accuse via `ordre ack <nouvel-id> lu|applique|refuse`) ;
//! - `super::journal::creer_ordre` (`journal.rs` : identifiant sous verrou,
//!   `hash=` scellé, persistance dans `ORDRES.log`) ;
//! - `super::autorite::{dossier_defaut, verifier}` (`autorite.rs` : porte
//!   jeton, même motif que `PorteOrdres` de `journal.rs`) ;
//! - `super::outils::{porte_jeton, usg}` (`outils.rs`) et
//!   `super::cli_gestion::extrait_option` (id positionnel premier) ;
//! - `super::{sha1_hex, BcastDir, Sortie}` (socle `mod.rs`) ;
//! - `super::metriques::{incremente, incremente_agent}` (`metriques.rs`,
//!   `pub(crate)`, hook best-effort comme les autres commandes).
//!
//! Persistance : le transfert est un ordre normal dans `ORDRES.log`
//! (`to=@destinataire`, `src=delegation:<agent>`, TTL 24 h), lié à sa
//! source par `supersede=<id>` (extension lue par `Extensions`).
//!
//! SPEC DE CÂBLAGE (assembleur) :
//! - `mod.rs` (+1 L, 439 L/26 fn → 440 L/26 fn) : ajouter
//!   `pub mod delegation;` entre `cli_gestion` et `digest` (alpha).
//! - `cli_gestion.rs` (+3 L, 444 L/19 fn → ~447 L/19 fn) : dans
//!   `cmd_ordre`, ajouter la branche
//!   `Some("deleguer") => super::delegation::cmd_deleguer(dir, &args[1..]),`
//!   et étendre l'usage en
//!   `usg("ordre : new|list|revoque|ack|pull|deleguer")`.
//! - CLI : `ordre deleguer <id> --to @agent --resume <texte>
//!   [--piece f...] --token "nom:secret"` (jeton exigé, porte existante).
//! - Aucune retouche à `app.rs`/`bus.rs`/`drain.rs` (interdits), aucune
//!   dépendance nouvelle, aucun secret affiché ni journalisé.

#[cfg(test)]
use super::ack::Verdict;
use super::autorite::{dossier_defaut, verifier};
use super::journal::creer_ordre;
#[cfg(test)]
use super::ordres::Extensions;
use super::ordres::{Ordre, StatutOrdre, VerifieJeton, TTL_DEFAUT_HEURES};
use super::outils::{porte_jeton, usg};
use super::{sans_retours, sha1_hex, BcastDir, Sortie};

/// Pièces jointes max par transfert (allowliste explicite, D3).
pub const MAX_PIECES: usize = 8;

/// Taille max d'une pièce, en octets (64 Kio, portée minimale).
pub const TAILLE_MAX_PIECE: u64 = 64 * 1024;

/// Longueur max du résumé, en caractères (résumé, pas historique).
pub const RESUME_MAX: usize = 2000;

/// Préfixe du champ `src` des ordres de transfert (`src=delegation:<agent>`).
pub const PREFIXE_SRC: &str = "delegation:";

/// Demande de transfert validée : source, destinataire, résumé, pièces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemandeDelegation {
    pub id_source: String,
    pub destinataire: String,
    pub resume: String,
    pub pieces: Vec<String>,
}

/// Métadonnées d'une pièce scellée : chemin, taille, SHA-1 (jamais le
/// contenu — portée minimale, le destinataire relit les fichiers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PieceJointe {
    pub chemin: String,
    pub octets: u64,
    pub sha1: String,
}

/// Vrai si `texte` est un accusé d'agent valide (`lu|applique|refuse`,
/// `revoque` réservé au système). Le destinataire accuse le transfert
/// via `ordre ack <nouvel-id> <verdict>` (porte existante, rien ici).
/// Spécification testée, sans consommateur hors tests (verdicts déjà
/// validés par `valider_ack` sur le chemin réel).
#[cfg(test)]
pub fn est_accuse_valide(texte: &str) -> bool {
    Verdict::parse(texte).is_some_and(|v| v.est_ack_utilisateur())
}

/// Valide le destinataire : `@agent` non vide, sans `|` ni retour
/// (assaini comme `valider_nouvel_ordre`, durci comme `dest`).
pub fn valider_destinataire(destinataire: &str) -> Result<String, String> {
    let propre = destinataire.trim();
    if !propre.starts_with('@') || propre.len() < 2 {
        return Err("bcast: destinataire invalide (--to @agent)".to_string());
    }
    if propre.contains('|') || propre.contains('\n') || propre.contains('\r') {
        return Err("bcast: destinataire invalide (caractères interdits)".to_string());
    }
    Ok(propre.to_string())
}

/// Valide la demande (pure, sans lecture disque) : identifiant source,
/// destinataire `@agent`, résumé non vide et cappé, pièces ≤ 8.
pub fn analyser_demande(
    id: &str,
    destinataire: &str,
    resume: &str,
    pieces: &[String],
) -> Result<DemandeDelegation, String> {
    if id.trim().is_empty() || id.contains('|') {
        return Err("bcast: identifiant d'ordre invalide".to_string());
    }
    let destinataire = valider_destinataire(destinataire)?;
    let resume = sans_retours(resume);
    if resume.trim().is_empty() {
        return Err("bcast: résumé de délégation vide".to_string());
    }
    if resume.chars().count() > RESUME_MAX {
        return Err("bcast: résumé trop long (2000 max)".to_string());
    }
    if pieces.len() > MAX_PIECES {
        return Err("bcast: trop de pièces (8 max)".to_string());
    }
    Ok(DemandeDelegation {
        id_source: id.trim().to_string(),
        destinataire,
        resume,
        pieces: pieces.to_vec(),
    })
}

/// Vrai si `chemin` est une pièce admissible : relatif, sans `..`, sans
/// `|` ni retour, 256 caractères max (allowliste explicite, D3).
fn chemin_piece_valide(chemin: &str) -> bool {
    if chemin.is_empty() || chemin.len() > 256 {
        return false;
    }
    if chemin.starts_with('/') || chemin.contains("..") {
        return false;
    }
    !chemin.contains('|') && !chemin.contains('\n') && !chemin.contains('\r')
}

/// Contrôle et scelle les pièces : chemins admissibles, fichiers
/// lisibles, taille cappée, SHA-1 du contenu (socle). Manquant ou trop
/// gros = refus fail-closed (transfert explicite ou rien).
pub fn controler_pieces(pieces: &[String]) -> Result<Vec<PieceJointe>, String> {
    let mut scellees = Vec::with_capacity(pieces.len());
    for chemin in pieces {
        if !chemin_piece_valide(chemin) {
            return Err(format!("bcast: pièce refusée : {chemin}"));
        }
        let contenu =
            std::fs::read(chemin).map_err(|_| format!("bcast: pièce illisible : {chemin}"))?;
        let octets = contenu.len() as u64;
        if octets > TAILLE_MAX_PIECE {
            return Err(format!("bcast: pièce trop grosse : {chemin}"));
        }
        scellees.push(PieceJointe {
            chemin: chemin.clone(),
            octets,
            sha1: sha1_hex(&contenu),
        });
    }
    Ok(scellees)
}

/// Retrouve l'ordre source : présent dans `ORDRES.log` et actif, sinon
/// refus (on ne délègue ni un ordre révoqué ni un ordre expiré).
pub fn trouver_ordre(contenu_log: &str, id: &str) -> Result<Ordre, String> {
    contenu_log
        .lines()
        .filter_map(Ordre::parse_ligne)
        .find(|o| o.id == id && o.statut == StatutOrdre::Actif)
        .ok_or_else(|| format!("bcast: ordre {id} introuvable ou non actif"))
}

/// Construit le texte du transfert : résumé explicite + métadonnées des
/// pièces (chemin, taille, SHA-1 — jamais le contenu) + lien
/// `supersede=<id>` en queue (lu par `Extensions::extraire` ; `creer_ordre`
/// scelle ensuite `hash=` comme pour tout ordre).
pub fn construire_texte(
    demande: &DemandeDelegation,
    agent: &str,
    pieces: &[PieceJointe],
) -> String {
    let mut texte = format!(
        "Délégation de {} par {} : {}",
        demande.id_source, agent, demande.resume
    );
    if !pieces.is_empty() {
        let details: Vec<String> = pieces
            .iter()
            .map(|p| format!("{} ({} o, sha1:{})", p.chemin, p.octets, p.sha1))
            .collect();
        texte.push_str(&format!(" [pièces : {}]", details.join(" ; ")));
    }
    texte.push_str(&format!(" supersede={}", demande.id_source));
    texte
}

/// Relit un transfert : rend `(id_source, corps)` si le texte porte un
/// lien `supersede=` (extension existante, `Extensions::extraire`).
/// Spécification testée, sans consommateur hors tests (`pull` affiche
/// déjà le suffixe complet, `supersede=` inclus).
#[cfg(test)]
pub fn lire_transfert(ordre: &Ordre) -> Option<(String, String)> {
    let (corps, ext) = Extensions::extraire(&ordre.texte);
    ext.supersede.map(|id| (id, corps))
}

/// Porte jetons du lot, même contrat que `PorteOrdres` (`journal.rs`) :
/// `nom:secret` vérifié contre le dossier défaut de l'autorité.
struct PorteDelegation;

impl VerifieJeton for PorteDelegation {
    fn verifier_jeton(&self, jeton: &str) -> Result<String, String> {
        let (nom, secret) = jeton
            .rsplit_once(':')
            .filter(|(n, s)| !n.is_empty() && !s.is_empty())
            .ok_or_else(|| "jeton invalide (forme \"nom:secret\" attendue)".to_string())?;
        let dir = dossier_defaut().map_err(|e| format!("bcast: {e:#}"))?;
        verifier(&dir, nom, secret)
            .map(|fiche| fiche.nom)
            .map_err(|e| format!("bcast: {e:#}"))
    }
}

/// Exécute le transfert : jeton exigé (identité = `src`), source active,
/// pièces scellées, puis ordre `to=@dest`, `src=delegation:<agent>`, TTL
/// 24 h via `creer_ordre` (identifiant sous verrou, `hash=` scellé).
/// Rend l'identifiant du nouvel ordre.
pub fn executer(
    dirs: &BcastDir,
    auth: &dyn VerifieJeton,
    jeton: &str,
    demande: &DemandeDelegation,
) -> Result<String, String> {
    let agent = auth.verifier_jeton(jeton)?;
    let contenu = std::fs::read_to_string(dirs.ordres()).unwrap_or_default();
    trouver_ordre(&contenu, &demande.id_source)?;
    let pieces = controler_pieces(&demande.pieces)?;
    let texte = construire_texte(demande, &agent, &pieces);
    let src = format!("{}{}", PREFIXE_SRC, agent.replace('|', "/"));
    // Transfert NON scellé (`None`) : le `sig=` lierait l'ACK au secret du
    // délégant, que le destinataire n'a pas (limite H2 documentée).
    creer_ordre(
        dirs,
        &demande.destinataire,
        &TTL_DEFAUT_HEURES.to_string(),
        &src,
        &texte,
        None,
    )
}

/// `ordre deleguer <id> --to @agent --resume <texte> [--piece f...]`
/// (jeton `--token` exigé, porte existante + garde anti-secret).
/// Échec de validation = usage (2), autre échec = exécution (1).
/// Extrait `--to/--resume/--token/--piece...` après l'identifiant :
/// `decoupage` s'arrêterait au premier positionnel, d'où l'extraction
/// manuelle partagée avec `revoque`/`ack` (`extrait_option`).
fn extraire_cible(
    args: &[String],
) -> Result<(String, String, Option<String>, Vec<String>), Sortie> {
    let extrait = |reste: &[String], nom: &str| super::cli_gestion::extrait_option(reste, nom);
    let Ok((dest, r1)) = extrait(&args[1..], "to") else {
        return Err(Sortie::Usage);
    };
    let Ok((resume, r2)) = extrait(&r1, "resume") else {
        return Err(Sortie::Usage);
    };
    let Ok((token, r3)) = extrait(&r2, "token") else {
        return Err(Sortie::Usage);
    };
    let (mut pieces, mut courant) = (Vec::new(), r3);
    while let Ok((trouvee, suite)) = extrait(&courant, "piece") {
        let Some(p) = trouvee else {
            courant = suite;
            break;
        };
        pieces.push(p);
        courant = suite;
    }
    pieces.reverse();
    if !courant.is_empty() {
        return Err(usg("option inconnue ou argument en trop"));
    }
    let (Some(dest), Some(resume)) = (dest, resume) else {
        return Err(usg("--to @agent et --resume <texte> requis"));
    };
    Ok((dest, resume, token, pieces))
}

pub(crate) fn cmd_deleguer(dir: &BcastDir, args: &[String]) -> Sortie {
    let Some(id) = args.first() else {
        return usg("usage : bcast ordre deleguer <id> --to @agent --resume <texte>");
    };
    let Ok((dest, resume, token, pieces)) = extraire_cible(args) else {
        return Sortie::Usage;
    };
    let Ok(demande) = analyser_demande(id, &dest, &resume, &pieces) else {
        return usg("demande de délégation invalide");
    };
    let garde = format!(
        "{} {} {}",
        demande.id_source, demande.destinataire, demande.resume
    );
    if !porte_jeton(&garde, token.as_deref()) {
        return Sortie::ErreurExecution;
    }
    let _ = dir.init();
    match executer(
        dir,
        &PorteDelegation,
        token.as_deref().unwrap_or(""),
        &demande,
    ) {
        Ok(id) => {
            super::metriques::incremente(dir, "ordre_deleguer");
            super::metriques::incremente_agent(dir, &demande.destinataire, "ordre_deleguer");
            println!("delegue={id}");
            Sortie::Ok
        }
        Err(_) => Sortie::ErreurExecution,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AuthTest;

    impl VerifieJeton for AuthTest {
        fn verifier_jeton(&self, jeton: &str) -> Result<String, String> {
            if jeton == "bon" {
                Ok("alice".to_string())
            } else {
                Err("jeton refuse".to_string())
            }
        }
    }

    fn demande_saine() -> DemandeDelegation {
        analyser_demande("O-20261108-001", "@bob", "Relire le plan", &[]).expect("saine")
    }

    #[test]
    fn validation_destinataire_resume_pieces() {
        assert!(valider_destinataire("@bob").is_ok());
        assert!(valider_destinataire("bob").is_err());
        assert!(valider_destinataire("@").is_err());
        assert!(valider_destinataire("@a|b").is_err());
        assert!(analyser_demande("O-1", "@b", "", &[]).is_err());
        assert!(analyser_demande("O-1", "@b", &"x".repeat(2001), &[]).is_err());
        let neuf: Vec<String> = (0..9).map(|i| format!("f{i}")).collect();
        assert!(analyser_demande("O-1", "@b", "ok", &neuf).is_err());
        let huit: Vec<String> = (0..8).map(|i| format!("f{i}")).collect();
        assert!(analyser_demande("O-1", "@b", "ok", &huit).is_ok());
        assert!(analyser_demande("", "@b", "ok", &[]).is_err());
    }

    #[test]
    fn accuse_limite_aux_verdicts_agents() {
        assert!(est_accuse_valide("lu"));
        assert!(est_accuse_valide("applique"));
        assert!(est_accuse_valide("refuse"));
        assert!(!est_accuse_valide("revoque"));
        assert!(!est_accuse_valide("vu"));
    }

    #[test]
    fn pieces_scelees_chemins_et_tailles() {
        let dir = std::env::temp_dir().join(format!("mbox-deleg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dossier");
        let petit = dir.join("note.txt");
        std::fs::write(&petit, "bonjour").expect("piece");
        let chemin = petit.to_string_lossy().to_string();
        // Absolu refusé : portée minimale = chemins relatifs explicites.
        assert!(controler_pieces(std::slice::from_ref(&chemin)).is_err());
        assert!(controler_pieces(&["../echappe.txt".to_string()]).is_err());
        assert!(controler_pieces(&["a|b".to_string()]).is_err());
        assert!(controler_pieces(&["n-existe-pas-xyz.txt".to_string()]).is_err());
        // Cas passant : fichier relatif (cwd des tests = racine crate).
        let relatif = format!("target/mbox-deleg-rel-{}.txt", std::process::id());
        std::fs::create_dir_all("target").ok();
        std::fs::write(&relatif, "bonjour").expect("relatif");
        let scellees = controler_pieces(std::slice::from_ref(&relatif)).expect("scelle");
        assert_eq!(scellees.len(), 1);
        assert_eq!(scellees[0].octets, 7);
        assert_eq!(scellees[0].sha1, sha1_hex(b"bonjour"));
        let _ = std::fs::remove_file(&relatif);
        // Plafond : 64 Kio + 1 refusé par le contrôleur lui-même.
        let gros = format!("target/mbox-deleg-gros-{}.bin", std::process::id());
        std::fs::write(&gros, vec![0u8; TAILLE_MAX_PIECE as usize + 1]).expect("gros");
        assert!(controler_pieces(std::slice::from_ref(&gros)).is_err());
        let _ = std::fs::remove_file(&gros);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn texte_transfert_minimal_lie_par_supersede() {
        let demande = demande_saine();
        let piece = PieceJointe {
            chemin: "notes/plan.md".to_string(),
            octets: 12,
            sha1: "abc123".to_string(),
        };
        let texte = construire_texte(&demande, "alice", &[piece]);
        assert!(texte.contains("Relire le plan"), "résumé : {texte}");
        assert!(
            texte.contains("notes/plan.md (12 o, sha1:abc123)"),
            "méta : {texte}"
        );
        assert!(
            texte.ends_with("supersede=O-20261108-001"),
            "lien : {texte}"
        );
        assert!(!texte.contains("historique"), "jamais tout l'historique");
        let ordre = Ordre::parse_ligne(&format!("O-9|1|d|@bob|24|actif|s|{texte}")).expect("parse");
        let (id, corps) = lire_transfert(&ordre).expect("lien relu");
        assert_eq!(id, "O-20261108-001");
        assert!(corps.contains("Relire le plan"));
        let brut = Ordre::parse_ligne("O-1|1|d|ALL|0|actif|s|Sans lien").expect("parse");
        assert!(lire_transfert(&brut).is_none());
    }

    #[test]
    fn cycle_transfert_persiste_ordre_lie() {
        let base = std::env::temp_dir().join(format!("mbox-deleg-j-{}", std::process::id()));
        let dirs = BcastDir::depuis(base);
        dirs.init().expect("init");
        let id = creer_ordre(&dirs, "ALL", "24", "user-direct", "Mission X", None).expect("source");
        let auth = AuthTest;
        assert!(executer(&dirs, &auth, "faux", &demande_saine()).is_err());
        let mut mauvaise = demande_saine();
        mauvaise.id_source = "O-0000-000".to_string();
        assert!(executer(&dirs, &auth, "bon", &mauvaise).is_err());
        let mut demande = demande_saine();
        demande.id_source = id.clone();
        let nouvel = executer(&dirs, &auth, "bon", &demande).expect("transfert");
        let contenu = std::fs::read_to_string(dirs.ordres()).expect("lit");
        let ordre = contenu
            .lines()
            .filter_map(Ordre::parse_ligne)
            .find(|o| o.id == nouvel)
            .expect("persisté");
        assert_eq!(ordre.dest, "@bob");
        assert_eq!(ordre.src, "delegation:alice");
        assert_eq!(ordre.ttl_h, TTL_DEFAUT_HEURES);
        assert!(ordre.texte.contains(&format!("supersede={id}")));
        assert!(ordre.texte.contains("hash="), "scellé : {}", ordre.texte);
        std::fs::remove_dir_all(dirs.racine()).ok();
    }

    #[test]
    fn cli_usages_sans_jeton_ni_disque() {
        let base = std::env::temp_dir().join(format!("mbox-deleg-c-{}", std::process::id()));
        let dirs = BcastDir::depuis(base.clone());
        let argv = |mots: &[&str]| mots.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(cmd_deleguer(&dirs, &argv(&[])).code(), 2);
        assert_eq!(cmd_deleguer(&dirs, &argv(&["O-1"])).code(), 2);
        assert_eq!(
            cmd_deleguer(&dirs, &argv(&["O-1", "--to", "@b", "--resume", "r"])).code(),
            1
        );
        assert_eq!(
            cmd_deleguer(&dirs, &argv(&["O-1", "O-2", "--to", "@b", "--resume", "r"])).code(),
            2
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
