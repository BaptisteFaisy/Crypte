//! Autorité locale à jetons, style PHONE_TOKEN (lot auth).
//!
//! Fichier `mbox-jetons` (0600) : une ligne par agent,
//! `nom|secret|epoque|ttl_h|statut`, calqué sur ORDRES.log
//! (lib-ordres.sh:3). Le secret (32 octets, 256 bits, hex) n'est montré
//! qu'une fois, à l'émission ; jamais listé, loggué, ni dans les erreurs.
//! « Jeton » ici = identifiant d'agent, PAS `crate::tokens` (compteur LLM).
//!
//! # Signatures partagées utilisées (pour l'assembleur)
//! - `crate::mbox::format::TTL_DEFAUT_HEURES: u64` (= 24, lot socle).
//! - `crate::mbox::format::normalise_emetteur(&str) -> anyhow::Result<String>`
//!   (règles bcast_sender, lib-common.sh:33-41 ; ne pas confondre avec
//!   `crate::mbox::normalise`, passe de déduplication des corps).
//! - `crate::mbox::format::analyser_ttl(&str) -> Result<u64>`
//!   (décimal, 0 = permanent, lib-ordres.sh:43).
//! - `crate::mbox::format::est_expire(epoque, ttl_h, maintenant: u64) -> bool`
//!   (lib-ordres.sh:29-31).
//! - `crate::settings::store::Store::default_dir()` (settings/store.rs:86-92).
//! - `crate::settings::store::write_private` rendue `pub(crate)` (+0 ligne,
//!   settings/store.rs:236-263) : atomique tmp+rename en 0600.
//! - Verrou via le garde partagé `crate::mbox::verrou::avec_verrou`
//!   (`flock -w 10`, tranche assembleur : zéro duplication du verrou).
//! - Porte calquée sur `Decision::Deny` de `tool_gate`
//!   (permissions_cmds.rs:66-89) ; `Debug` masqué calqué sur `Credential`
//!   (settings/store.rs:42-50).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rand::RngCore;

use crate::mbox::format::{
    analyser_ttl, epoque, est_expire, normalise_emetteur, TTL_DEFAUT_HEURES,
};
use crate::settings::store::{write_private, Store};

/// Nom du fichier des jetons dans le dossier des réglages.
const FICHIER_JETONS: &str = "mbox-jetons";
/// Secret brut : 32 octets = 256 bits (exigence ≥ 128 bits).
const SECRET_OCTETS: usize = 32;
/// Motifs interdits dans tout texte écrit (garde anti-secret).
const INTERDITS: &[&str] = &[
    "vck_",
    "ghp_",
    "gho_",
    "sk-",
    "PHONE_TOKEN",
    "CLOUDFLARE_API_TOKEN",
];

/// Statut d'un jeton (vocabulaire ORDRES.log : actif|revoque|expire).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Statut {
    Actif,
    Revoque,
    Expire,
}

impl Statut {
    /// Lit un statut depuis le fichier.
    fn parse(brut: &str) -> Result<Self> {
        match brut {
            "actif" => Ok(Self::Actif),
            "revoque" => Ok(Self::Revoque),
            "expire" => Ok(Self::Expire),
            _ => bail!("statut de jeton inconnu"),
        }
    }

    /// Forme fichier du statut.
    fn etiquette(self) -> &'static str {
        match self {
            Self::Actif => "actif",
            Self::Revoque => "revoque",
            Self::Expire => "expire",
        }
    }
}

/// Jeton stocké (secret masqué en `Debug`, comme `Credential`).
#[derive(Clone)]
struct Jeton {
    nom: String,
    secret: String,
    epoque: u64,
    ttl_h: u64,
    statut: Statut,
}

impl std::fmt::Debug for Jeton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Jeton")
            .field("nom", &self.nom)
            .field("secret", &"[redacted]")
            .field("epoque", &self.epoque)
            .field("ttl_h", &self.ttl_h)
            .field("statut", &self.statut)
            .finish()
    }
}

/// Fiche publique : sans secret, listable et affichable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fiche {
    pub nom: String,
    pub epoque: u64,
    pub ttl_h: u64,
    pub statut: Statut,
}

/// Émission : secret montré une seule fois, masqué en `Debug`.
pub struct Emission {
    pub nom: String,
    pub secret: String,
}

impl std::fmt::Debug for Emission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Emission")
            .field("nom", &self.nom)
            .field("secret", &"[redacted]")
            .finish()
    }
}

/// Dossier des jetons : le dossier des réglages (`Store::default_dir`).
pub fn dossier_defaut() -> Result<PathBuf> {
    Store::default_dir()
}

/// Chemin du fichier des jetons dans `dir`.
fn chemin_jetons(dir: &Path) -> PathBuf {
    dir.join(FICHIER_JETONS)
}

/// Secret aléatoire : 32 octets (`rand`, getrandom déjà au lock) en hex.
fn secret_aleatoire() -> String {
    let mut brut = [0u8; SECRET_OCTETS];
    rand::rng().fill_bytes(&mut brut);
    let mut hex = String::with_capacity(SECRET_OCTETS * 2);
    for octet in brut {
        hex.push_str(&format!("{octet:02x}"));
    }
    hex
}

/// Comparaison sans court-circuit (secret stocké en hex fixe).
/// Point unique partagé (`signature.rs` l'utilise aussi).
pub(crate) fn comparaison_constante(a: &str, b: &str) -> bool {
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

/// Lit `nom|secret|epoque|ttl_h|statut` (erreur sans contenu secret).
fn analyser_ligne(ligne: &str, numero: usize) -> Result<Jeton> {
    let mut champs = ligne.split('|');
    let extrait = |o: Option<&str>| o.unwrap_or("").trim().to_string();
    let (nom, secret) = (extrait(champs.next()), extrait(champs.next()));
    let epoque = extrait(champs.next()).parse::<u64>().unwrap_or(u64::MAX);
    let ttl_h = extrait(champs.next()).parse::<u64>().unwrap_or(u64::MAX);
    if nom.is_empty() || secret.is_empty() || epoque == u64::MAX || ttl_h == u64::MAX {
        bail!("ligne {numero} du fichier des jetons malformée");
    }
    let statut = Statut::parse(champs.next().unwrap_or("").trim())
        .with_context(|| format!("ligne {numero} du fichier des jetons malformée"))?;
    Ok(Jeton {
        nom,
        secret,
        epoque,
        ttl_h,
        statut,
    })
}

/// Forme fichier d'un jeton (`|` refusé dans le nom par `normalise`).
fn formater_ligne(jeton: &Jeton) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        jeton.nom,
        jeton.secret,
        jeton.epoque,
        jeton.ttl_h,
        jeton.statut.etiquette()
    )
}

/// Charge les jetons (absent = aucun ; sans verrou comme `ordre_get`,
/// lib-ordres.sh:58-67). Fail-closed à la première ligne malformée.
fn charger(chemin: &Path) -> Result<Vec<Jeton>> {
    let brut = match fs::read_to_string(chemin) {
        Ok(brut) => brut,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("lecture {}", chemin.display())),
    };
    let mut jetons = Vec::new();
    for (i, ligne) in brut.lines().enumerate() {
        if ligne.trim().is_empty() {
            continue;
        }
        jetons.push(analyser_ligne(ligne, i + 1)?);
    }
    Ok(jetons)
}

/// Sauve les jetons : dossier 0700 (miroir store.rs:130-136) puis 0600.
fn sauver(chemin: &Path, jetons: &[Jeton]) -> Result<()> {
    if let Some(dossier) = chemin.parent() {
        fs::create_dir_all(dossier).with_context(|| format!("création {}", dossier.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dossier, fs::Permissions::from_mode(0o700)).ok();
        }
    }
    let mut brut = String::new();
    for jeton in jetons {
        brut.push_str(&formater_ligne(jeton));
        brut.push('\n');
    }
    write_private(chemin, brut.as_bytes())
}

/// Exécute `tache` sous `<cible>.lock` via le garde partagé du lot
/// store (`flock -w 10`, fail-closed). Les erreurs `anyhow` transitent
/// en texte (messages préservés, sources perdues).
fn avec_verrou<T>(cible: &Path, tache: impl FnOnce() -> Result<T>) -> Result<T> {
    use crate::mbox::verrou::{avec_verrou as garde, chemin_verrou, ATTENTE_VERROU_SECS};
    let verrou = chemin_verrou(cible);
    if let Some(dossier) = verrou.parent() {
        fs::create_dir_all(dossier).with_context(|| format!("création {}", dossier.display()))?;
    }
    garde(&verrou, ATTENTE_VERROU_SECS, || {
        tache().map_err(|e| std::io::Error::other(e.to_string()))
    })
    .map_err(|e| anyhow::anyhow!(e.to_string()))
}

/// Émet un jeton (`ttl` brut en heures, `None` = 24 h, `0` = permanent).
/// Le secret n'est rendu qu'ici, une seule fois.
pub fn emettre(dir: &Path, nom: &str, ttl: Option<&str>) -> Result<Emission> {
    let nom = normalise_emetteur(nom)?;
    let ttl_h = match ttl {
        None => TTL_DEFAUT_HEURES,
        Some(brut) => analyser_ttl(brut).map_err(|e| anyhow::anyhow!(e))?,
    };
    let chemin = chemin_jetons(dir);
    avec_verrou(&chemin, || {
        let mut jetons = charger(&chemin)?;
        if jetons
            .iter()
            .any(|j| j.nom == nom && j.statut == Statut::Actif)
        {
            bail!("un jeton actif existe déjà pour « {nom} »");
        }
        let secret = secret_aleatoire();
        jetons.push(Jeton {
            nom: nom.clone(),
            secret: secret.clone(),
            epoque: epoque(),
            ttl_h,
            statut: Statut::Actif,
        });
        sauver(&chemin, &jetons)?;
        Ok(Emission { nom, secret })
    })
}

/// Vérifie un jeton (actif + non expiré + secret exact), fail-closed.
/// Expiration paresseuse : `purger` marque, ici on refuse seulement.
pub fn verifier(dir: &Path, nom: &str, secret: &str) -> Result<Fiche> {
    let nom = normalise_emetteur(nom)?;
    let jetons = charger(&chemin_jetons(dir))?;
    let cible = jetons
        .iter()
        .find(|j| j.nom == nom && j.statut == Statut::Actif);
    let Some(jeton) = cible else {
        bail!("jeton inconnu ou révoqué");
    };
    if est_expire(jeton.epoque, jeton.ttl_h, epoque()) {
        bail!("jeton expiré");
    }
    if !comparaison_constante(secret, &jeton.secret) {
        bail!("jeton refusé");
    }
    Ok(Fiche {
        nom: jeton.nom.clone(),
        epoque: jeton.epoque,
        ttl_h: jeton.ttl_h,
        statut: jeton.statut,
    })
}

/// Révoque le jeton actif de `nom` (tmp+rename sous verrou, patron
/// lib-ordres.sh:80-88 via `sauver`).
pub fn revoquer(dir: &Path, nom: &str) -> Result<()> {
    let nom = normalise_emetteur(nom)?;
    let chemin = chemin_jetons(dir);
    avec_verrou(&chemin, || {
        let mut jetons = charger(&chemin)?;
        let cible = jetons
            .iter_mut()
            .find(|j| j.nom == nom && j.statut == Statut::Actif);
        let Some(cible) = cible else {
            bail!("jeton « {nom} » introuvable ou non actif");
        };
        cible.statut = Statut::Revoque;
        sauver(&chemin, &jetons)
    })
}

/// Marque expirés les jetons dépassés ; ne réécrit que si un statut a
/// changé (patron `ordre_expire_pass`, lib-ordres.sh:20-38). Rend le
/// nombre de jetons nouvellement expirés.
pub fn purger(dir: &Path) -> Result<usize> {
    let chemin = chemin_jetons(dir);
    avec_verrou(&chemin, || {
        let mut jetons = charger(&chemin)?;
        let maintenant = epoque();
        let mut marques = 0;
        for jeton in jetons.iter_mut() {
            if jeton.statut == Statut::Actif && est_expire(jeton.epoque, jeton.ttl_h, maintenant) {
                jeton.statut = Statut::Expire;
                marques += 1;
            }
        }
        if marques > 0 {
            sauver(&chemin, &jetons)?;
        }
        Ok(marques)
    })
}

/// Fiches sans secret : actives seules, ou toutes si `tout`.
pub fn lister(dir: &Path, tout: bool) -> Result<Vec<Fiche>> {
    Ok(charger(&chemin_jetons(dir))?
        .iter()
        .filter(|j| tout || j.statut == Statut::Actif)
        .map(|j| Fiche {
            nom: j.nom.clone(),
            epoque: j.epoque,
            ttl_h: j.ttl_h,
            statut: j.statut,
        })
        .collect())
}

/// Garde anti-secret : refuse tout texte contenant un motif interdit.
/// L'erreur nomme le motif, jamais le contenu.
pub fn garde_antisecret(texte: &str) -> Result<()> {
    for motif in INTERDITS {
        if texte.contains(motif) {
            bail!("refus d'écriture : motif « {motif} » interdit");
        }
    }
    Ok(())
}

/// Porte des écritures sensibles : jeton valide exigé puis texte passé au
/// crible. Tout échec refuse sans confirmation (miroir `Decision::Deny`,
/// permissions_cmds.rs:69-82).
pub fn porte_ecriture(dir: &Path, nom: &str, secret: &str, texte: &str) -> Result<Fiche> {
    let fiche = verifier(dir, nom, secret)?;
    garde_antisecret(texte)?;
    Ok(fiche)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bac jetable unique pour un test.
    fn bac(nom: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mbox-autorite-{}-{nom}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        dir
    }

    /// Plante un jeton fabriqué (époques passées pour l'expiration).
    fn plante(dir: &Path, nom: &str, secret: &str, epoque: u64, ttl_h: u64) {
        fs::create_dir_all(dir).unwrap();
        sauver(
            &chemin_jetons(dir),
            &[Jeton {
                nom: nom.into(),
                secret: secret.into(),
                epoque,
                ttl_h,
                statut: Statut::Actif,
            }],
        )
        .unwrap();
    }

    #[test]
    fn emission_puis_verification_ok() {
        let dir = bac("emet");
        let emis = emettre(&dir, "agent-bleu", None).unwrap();
        assert_eq!(emis.secret.len(), SECRET_OCTETS * 2);
        assert!(emis.secret.bytes().all(|b| b.is_ascii_hexdigit()));
        let fiche = verifier(&dir, "agent-bleu", &emis.secret).unwrap();
        assert_eq!(
            (fiche.ttl_h, fiche.statut),
            (TTL_DEFAUT_HEURES, Statut::Actif)
        );
        assert!(emettre(&dir, "agent-bleu", None).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(chemin_jetons(&dir))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mauvais_secret_refuse_et_debug_masque() {
        let dir = bac("verif");
        let emis = emettre(&dir, "agent-vert", Some("0")).unwrap();
        assert!(verifier(&dir, "agent-vert", "00").is_err());
        assert!(verifier(&dir, "inconnu", &emis.secret).is_err());
        let trace = format!("{:?}", charger(&chemin_jetons(&dir)).unwrap()[0]);
        assert!(trace.contains("[redacted]") && !trace.contains(&emis.secret));
        assert!(!format!("{emis:?}").contains(&emis.secret));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn revocation_interdit_puis_laisse_reemettre() {
        let dir = bac("revoq");
        let emis = emettre(&dir, "agent-rouge", None).unwrap();
        revoquer(&dir, "agent-rouge").unwrap();
        assert!(verifier(&dir, "agent-rouge", &emis.secret).is_err());
        assert!(revoquer(&dir, "agent-rouge").is_err());
        assert!(revoquer(&dir, "fantome").is_err());
        let emis2 = emettre(&dir, "agent-rouge", None).unwrap();
        verifier(&dir, "agent-rouge", &emis2.secret).unwrap();
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn expiration_paresseuse_purge_et_marque() {
        let dir = bac("expir");
        plante(&dir, "vieux", "ab", 1, 1);
        assert!(verifier(&dir, "vieux", "ab").is_err());
        assert_eq!((purger(&dir).unwrap(), purger(&dir).unwrap()), (1, 0));
        assert_eq!(lister(&dir, true).unwrap()[0].statut, Statut::Expire);
        assert!(lister(&dir, false).unwrap().is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn permanent_ne_meurt_jamais() {
        let dir = bac("perma");
        plante(&dir, "roc", "cd", 1, 0);
        assert!(verifier(&dir, "roc", "cd").is_ok());
        assert_eq!(purger(&dir).unwrap(), 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn garde_et_porte_bout_en_bout() {
        let dir = bac("porte");
        let emis = emettre(&dir, "portier", None).unwrap();
        assert!(garde_antisecret("bonjour le monde").is_ok());
        for motif in INTERDITS {
            let err = garde_antisecret(&format!("fuite {motif}XYZ"))
                .unwrap_err()
                .to_string();
            assert!(err.contains(motif) && !err.contains("XYZ"));
        }
        assert!(porte_ecriture(&dir, "portier", &emis.secret, "message sain").is_ok());
        assert!(porte_ecriture(&dir, "portier", "faux", "message sain").is_err());
        assert!(porte_ecriture(&dir, "portier", &emis.secret, "clef sk-abc").is_err());
        fs::remove_dir_all(&dir).ok();
    }
}
