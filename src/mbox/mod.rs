//! mbox — boîte aux lettres des agents (miroir Rust du broadcast v2 shell).
//!
//! Socle partagé par tous les lots : chemins, horodatage, identités sûres,
//! normalisation de déduplication, codes de sortie et erreur commune.
//! Lecture interopérable à l'identique des formats v2 produits par le shell.
//!
//! Signatures partagées utilisées (figées par le delta, rappel assembleur) :
//! - `BCAST_DIR_ENV`, `BCAST_DIR_DEFAUT`, `SUJETS_BUS`
//! - `BcastDir` : `defaut`, `depuis`, `racine`, `joindre`, `init`, `boite`,
//!   `bus`, `ordres`, `acks`, `vue_ordres`, `hb`, `etat`, `tailles`
//! - `epoque`, `maintenant`, `sanitize`, `sha1_hex`, `tampon`, `normalise`
//! - `Sortie` (`Ok` = 0, `ErreurExecution` = 1, `Usage` = 2,
//!   `DejaEnCours` = 3) et `code`
//!
//! Les lots `format` (lignes v2), `autorite` (jetons), `verrou` + `fichier`
//! (écritures verrouillées), `ordres` + `ack` + `journal`, `throttle`,
//! `cli` + `cli_gestion` + `jeton`, `sonde` réutilisent ce socle via
//! `super::` ou `crate::mbox::` — aucune logique dupliquée.
//!
//! Conventions : code et commentaires en français, aucune donnée secrète
//! affichée ni journalisée, lecture seule sur les fichiers v2 existants.

pub mod ack;
pub mod autorite;
pub mod cli;
pub mod cli_gestion;
pub mod digest;
pub mod docteur;
pub mod fantome;
pub mod fichier;
pub mod format;
pub mod jeton;
pub mod journal;
pub mod metriques;
pub mod ordres;
pub mod outils;
pub mod sonde;
pub mod throttle;
pub mod verrou;

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Chemins : miroir de bcast_dir / bcast_init (lib-common.sh).
// ---------------------------------------------------------------------------

/// Nom de la variable d'environnement qui surcharge le dossier broadcast.
pub const BCAST_DIR_ENV: &str = "BCAST_DIR";

/// Dossier broadcast par défaut (défaut du shell, lib-common.sh).
pub const BCAST_DIR_DEFAUT: &str = "/home/baptiste/Bureau/.scratch/broadcast";

/// Sujets du bus inter-agents (miroir de BUS_TOPICS, bcast).
pub const SUJETS_BUS: [&str; 4] = ["phone", "gpu", "file", "divers"];

/// Racine du broadcast v2 : résout, joint et initialise l'arborescence.
#[derive(Debug, Clone)]
pub struct BcastDir {
    racine: PathBuf,
}

impl BcastDir {
    /// Résout la racine depuis l'environnement ou le défaut (comme bcast_dir).
    pub fn defaut() -> Self {
        let racine = std::env::var(BCAST_DIR_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(BCAST_DIR_DEFAUT));
        Self::depuis(racine)
    }

    /// Racine explicite (tests, dossiers isolés).
    pub fn depuis(racine: PathBuf) -> Self {
        Self { racine }
    }

    /// Chemin racine résolu.
    pub fn racine(&self) -> &Path {
        &self.racine
    }

    /// Joint un chemin relatif à la racine.
    pub fn joindre(&self, relatif: &str) -> PathBuf {
        self.racine.join(relatif)
    }

    /// Crée dossiers et fichiers vides (miroir de bcast_init).
    pub fn init(&self) -> std::io::Result<()> {
        for sous in ["bus", "archive", ".hb", ".state"] {
            std::fs::create_dir_all(self.joindre(sous))?;
        }
        let mut cibles = vec![
            "BOITE-USER.md".to_string(),
            "ORDRES.log".to_string(),
            "ORDRES-ACK.log".to_string(),
            ".sizes".to_string(),
        ];
        for sujet in SUJETS_BUS {
            cibles.push(format!("bus/{sujet}.log"));
        }
        for cible in &cibles {
            let chemin = self.joindre(cible);
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&chemin)?;
        }
        Ok(())
    }

    /// Boîte de réception de l'utilisateur.
    pub fn boite(&self) -> PathBuf {
        self.joindre("BOITE-USER.md")
    }

    /// Journal d'un sujet du bus (`bus/<sujet>.log`).
    pub fn bus(&self, sujet: &str) -> PathBuf {
        self.joindre(&format!("bus/{sujet}.log"))
    }

    /// Journal append-only des ordres.
    pub fn ordres(&self) -> PathBuf {
        self.joindre("ORDRES.log")
    }

    /// Journal des accusés de réception et révocations.
    pub fn acks(&self) -> PathBuf {
        self.joindre("ORDRES-ACK.log")
    }

    /// Vue lisible régénérée des ordres actifs.
    pub fn vue_ordres(&self) -> PathBuf {
        self.joindre("ORDRE.md")
    }

    /// Fichier de battement de cœur (nom assaini, comme `bcast hb`).
    pub fn hb(&self, nom: &str) -> PathBuf {
        self.joindre(&format!(".hb/{}", sanitize(nom)))
    }

    /// Fichier d'état throttle (`.state/<émetteur>.<suffixe>`, assainis).
    pub fn etat(&self, emetteur: &str, suffixe: &str) -> PathBuf {
        self.joindre(&format!(".state/{}.{}", sanitize(emetteur), suffixe))
    }

    /// Journal des rotations et sondes.
    pub fn tailles(&self) -> PathBuf {
        self.joindre(".sizes")
    }
}

// ---------------------------------------------------------------------------
// Horodatage : miroir de bcast_epoch / bcast_now (lib-common.sh).
// ---------------------------------------------------------------------------

/// Secondes écoulées depuis l'époque Unix.
pub fn epoque() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duree| duree.as_secs())
        .unwrap_or(0)
}

/// Date locale au format v2 `AAAA-MM-JJ HH:MM`.
pub fn maintenant() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

// ---------------------------------------------------------------------------
// SHA-1 maison : aucun ajout de dépendance (décision du delta).
// ---------------------------------------------------------------------------

/// Empreinte SHA-1 en hexadécimal (40 caractères), implémentation locale.
pub fn sha1_hex(donnees: &[u8]) -> String {
    let mut message = donnees.to_vec();
    let bits = (donnees.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_be_bytes());
    let mut h = [
        0x67452301u32,
        0xEFCDAB89,
        0x98BADCFE,
        0x10325476,
        0xC3D2E1F0,
    ];
    for bloc in message.chunks_exact(64) {
        comprime_bloc(&mut h, bloc);
    }
    let mut texte = String::with_capacity(40);
    for mot in h {
        texte.push_str(&format!("{mot:08x}"));
    }
    texte
}

/// Compression d'un bloc de 64 octets dans l'état SHA-1 (80 tours).
fn comprime_bloc(h: &mut [u32; 5], bloc: &[u8]) {
    let mut w = [0u32; 80];
    for (i, mot) in w.iter_mut().enumerate().take(16) {
        *mot = u32::from_be_bytes([
            bloc[4 * i],
            bloc[4 * i + 1],
            bloc[4 * i + 2],
            bloc[4 * i + 3],
        ]);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }
    let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
    for (i, &mot) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | (!b & d), 0x5A827999),
            20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
            _ => (b ^ c ^ d, 0xCA62C1D6),
        };
        let tour = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(mot);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = tour;
    }
    for (slot, delta) in h.iter_mut().zip([a, b, c, d, e]) {
        *slot = slot.wrapping_add(delta);
    }
}

// ---------------------------------------------------------------------------
// Identités : miroir de bcast_sanitize / bcast_stamp (lib-common.sh).
// ---------------------------------------------------------------------------

/// Nom de fichier sûr : 40 premiers caractères restreints, plus 8 de hachage.
/// Miroir exact de `bcast_sanitize` (locale UTF-8 des deux côtés).
pub fn sanitize(emetteur: &str) -> String {
    let propre: String = emetteur
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(40)
        .collect();
    format!("{propre}-{}", &sha1_hex(emetteur.as_bytes())[..8])
}

/// Tampon d'attribution `{#pid=… ppid=… tty=… sess=…}` (miroir de bcast_stamp).
/// Sans terminal (`/dev/null`, tube, échec de lecture) le tty vaut `-`,
/// exactement comme le shell quand `tty -s` échoue.
pub fn tampon() -> String {
    let pid = std::process::id();
    let (mut ppid, mut sess) = ("?".to_string(), "?".to_string());
    if let Ok(stat) = std::fs::read_to_string("/proc/self/stat") {
        if let Some(fin) = stat.rfind(')') {
            let champs: Vec<&str> = stat[fin + 1..].split_whitespace().collect();
            // Après `(comm)` : champ 3 = état, 4 = ppid, 6 = session.
            if let Some(valeur) = champs.get(1) {
                ppid = valeur.to_string();
            }
            if let Some(valeur) = champs.get(3) {
                sess = valeur.to_string();
            }
        }
    }
    let mut tty = "-".to_string();
    if let Ok(cible) = std::fs::read_link("/proc/self/fd/0") {
        let texte = cible.to_string_lossy();
        if let Some(nom) = texte.strip_prefix("/dev/") {
            if nom != "null" && !nom.is_empty() {
                tty = nom.to_string();
            }
        }
    }
    format!("{{#pid={pid} ppid={ppid} tty={tty} sess={sess}}}")
}

// ---------------------------------------------------------------------------
// Normalisation UNIQUE de déduplication (tous les lots passent par ici).
// ---------------------------------------------------------------------------

/// Normalise un corps pour la déduplication : rogne, retire les dates et
/// heures (`AAAA-MM-JJ`, `HH:MM`), condense les blancs, minuscules, supprime
/// les lignes vides. Deux messages identiques à dates différentes partagent
/// la même forme — le shell hache le corps brut, cette passe est le point
/// unique côté Rust et ne doit être réimplémentée nulle part ailleurs.
pub fn normalise(corps: &str) -> String {
    let chiffres = |octets: &[u8], debut: usize, n: usize| -> bool {
        octets.len() >= debut + n && octets[debut..debut + n].iter().all(u8::is_ascii_digit)
    };
    let mut lignes = Vec::new();
    for brute in corps.lines() {
        let octets = brute.trim().as_bytes();
        let mut garde: Vec<u8> = Vec::with_capacity(octets.len());
        let mut i = 0;
        while i < octets.len() {
            let date = chiffres(octets, i, 4)
                && octets.get(i + 4) == Some(&b'-')
                && chiffres(octets, i + 5, 2)
                && octets.get(i + 7) == Some(&b'-')
                && chiffres(octets, i + 8, 2);
            let heure = chiffres(octets, i, 2)
                && octets.get(i + 2) == Some(&b':')
                && chiffres(octets, i + 3, 2);
            if date {
                garde.push(b' ');
                i += 10;
            } else if heure {
                garde.push(b' ');
                i += 5;
            } else {
                garde.push(octets[i]);
                i += 1;
            }
        }
        let texte = String::from_utf8_lossy(&garde);
        let mots: Vec<&str> = texte.split_whitespace().collect();
        let ligne = mots.join(" ").to_lowercase();
        if !ligne.is_empty() {
            lignes.push(ligne);
        }
    }
    lignes.join("\n")
}

// ---------------------------------------------------------------------------
// Sorties et erreur commune (miroir de l'aide bcast : 0/1/2/3).
// ---------------------------------------------------------------------------

/// Code de sortie CLI, aligné sur le shell (0 ok, 1 exécution, 2 usage,
/// 3 déjà-en-cours — voir l'aide de bcast).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sortie {
    /// Succès (`sent`, `throttled:…`, `same-state` inclus).
    Ok = 0,
    /// Erreur d'exécution (verrou, écriture, ordre inactif, ACK refusé).
    ErreurExecution = 1,
    /// Erreur d'usage (option inconnue, argument manquant ou invalide).
    Usage = 2,
    /// Opération déjà en cours (garde anti-double).
    DejaEnCours = 3,
}

impl Sortie {
    /// Valeur numérique pour `std::process::exit`.
    pub fn code(self) -> i32 {
        self as i32
    }
}

// ---------------------------------------------------------------------------
// Tests unitaires intra-fichier (convention Crypte).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temps_et_sha1() {
        let premier = epoque();
        assert!(premier > 1_700_000_000);
        assert!(epoque() >= premier);
        let date = maintenant();
        let octets = date.as_bytes();
        assert_eq!(date.len(), 16);
        assert_eq!(&octets[4..5], b"-");
        assert_eq!(&octets[7..8], b"-");
        assert_eq!(&octets[10..11], b" ");
        assert_eq!(&octets[13..14], b":");
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn identites_assainies_et_tampon() {
        let nom = sanitize("agent:tel/dodo");
        assert!(nom.starts_with("agent_tel_dodo-"));
        assert_eq!(nom.len(), "agent_tel_dodo-".len() + 8);
        let long = sanitize(&"x".repeat(100));
        assert_eq!(long.len(), 40 + 1 + 8);
        assert_eq!(sanitize("muse"), sanitize("muse"));
        assert_ne!(sanitize("muse"), sanitize("muse!"));
        let t = tampon();
        assert!(t.starts_with("{#pid="), "tampon brut : {t}");
        assert!(t.contains(&format!("pid={}", std::process::id())));
        assert!(t.contains(" ppid=") && t.contains(" tty=") && t.contains(" sess="));
        assert!(t.ends_with('}'));
    }

    #[test]
    fn dedup_normalise() {
        assert_eq!(normalise("  Bonjour   MONDE  "), "bonjour monde");
        assert_eq!(
            normalise("Rapport 2026-11-08 lu à 15:24 ok"),
            "rapport lu à ok"
        );
        assert_eq!(
            normalise("ligne une\n\n   \nligne  DEUX "),
            "ligne une\nligne deux"
        );
        assert_eq!(normalise("   \n  "), "");
    }

    #[test]
    fn chemins_et_init() {
        let base =
            std::env::temp_dir().join(format!("mbox-socle-{}-{}", std::process::id(), epoque()));
        let dir = BcastDir::depuis(base.clone());
        dir.init().expect("init crée l'arborescence");
        assert!(dir.boite().ends_with("BOITE-USER.md"));
        assert!(dir.bus("gpu").ends_with("bus/gpu.log"));
        assert!(dir.ordres().is_file());
        assert!(dir.acks().is_file());
        assert!(dir.tailles().is_file());
        assert!(dir.hb("agent x").to_string_lossy().contains(".hb/agent_x-"));
        let etat = dir.etat("muse", "dedup");
        assert!(etat.to_string_lossy().contains(".state/"));
        assert!(etat.to_string_lossy().ends_with(".dedup"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn sorties_codes_shell() {
        assert_eq!(Sortie::Ok.code(), 0);
        assert_eq!(Sortie::ErreurExecution.code(), 1);
        assert_eq!(Sortie::Usage.code(), 2);
        assert_eq!(Sortie::DejaEnCours.code(), 3);
    }
}
