//! Coffre chiffré des providers : `providers.enc` + clé maîtresse locale.
//!
//! KEEP tel quel de `settings/store.rs` du TUI — **zéro réécriture
//! crypto** : même format (`KHSE1\0\0\0` + nonce 12 + ChaCha20-Poly1305),
//! même AAD, mêmes garde-fous (jamais de régénération sur un coffre
//! existant, fichiers 0600, clé jamais affichée ni logguée).

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};

/// Tag du format binaire (premiers octets de chaque fichier coffre).
const MAGIC: &[u8; 8] = b"KHSE1\0\0\0";
/// Longueur du nonce (ChaCha20-Poly1305).
const NONCE_LEN: usize = 12;
/// Longueur de la clé maîtresse.
const KEY_LEN: usize = 32;
/// Données associées : un coffre copié ailleurs n'est pas rouvert en silence.
const AAD: &[u8] = b"kiro-horror-tui/providers/v1";
/// Surcharge d'environnement de la clé maîtresse (64 caractères hex).
pub const KEY_ENV: &str = "KH_SETTINGS_KEY";

/// Un provider sauvegardé : endpoint, modèle et clé API (jamais affichée).
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Credential {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub key: String,
}

// Debug manuel : la clé ne doit jamais atteindre un log ni un panic.
impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("key", &"[redacted]")
            .finish()
    }
}

/// Contenu déchiffré du coffre.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Vault {
    #[serde(default = "current_version")]
    pub version: u8,
    /// Provider connecté en dernier (utilisé au démarrage sans env/CLI).
    #[serde(default)]
    pub last: Option<String>,
    #[serde(default)]
    pub providers: BTreeMap<String, Credential>,
}

/// Version courante du schéma du coffre.
fn current_version() -> u8 {
    1
}

impl Vault {
    /// Coffre vide à la version courante.
    pub fn new() -> Self {
        Self {
            version: current_version(),
            ..Self::default()
        }
    }
}

/// Le store : un dossier contenant le coffre et sa clé maîtresse.
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// Dossier data de l'app (`crate::paths`, migré depuis le legacy).
    pub fn default_dir() -> Result<PathBuf> {
        crate::paths::data_dir().context("HOME missing: settings store path unknown")
    }

    /// Store raciné à `dir` (les tests utilisent un dossier temporaire).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Store dans le dossier de données par défaut.
    pub fn default_store() -> Result<Self> {
        Ok(Self::new(Self::default_dir()?))
    }

    /// Fichier coffre chiffré.
    pub fn data_path(&self) -> PathBuf {
        self.dir.join("providers.enc")
    }

    /// Fichier clé maîtresse locale (32 octets bruts, mode 0600).
    pub fn master_path(&self) -> PathBuf {
        self.dir.join("master.key")
    }

    /// Lit et déchiffre le coffre ; un fichier manquant = coffre vide.
    pub fn load(&self) -> Result<Vault> {
        let path = self.data_path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vault::new()),
            Err(err) => {
                return Err(err).with_context(|| format!("reading {}", path.display()));
            }
        };
        let key = self.master(false)?;
        open(&bytes, &key).with_context(|| format!("opening {}", path.display()))
    }

    /// Chiffre et écrit le coffre (atomique : fichier tmp puis rename, 0600).
    pub fn save(&self, vault: &Vault) -> Result<()> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700)).ok();
        }
        let key = self.master(true)?;
        let bytes = seal(vault, &key)?;
        write_private(&self.data_path(), &bytes)
    }

    /// Clé maîtresse : `KH_SETTINGS_KEY` (hex), sinon `master.key` (créée
    /// une fois, à la sauvegarde seulement). Une clé existante de mauvaise
    /// taille est une erreur, jamais régénérée — idem pour une clé
    /// manquante alors qu'un coffre existe : une nouvelle clé orphelinerait
    /// en silence toutes les clés API sauvegardées.
    fn master(&self, create: bool) -> Result<[u8; KEY_LEN]> {
        if let Some(hex) = env_var(KEY_ENV) {
            return decode_hex(&hex);
        }
        let path = self.master_path();
        match fs::read(&path) {
            Ok(bytes) => {
                let len = bytes.len();
                bytes.try_into().map_err(|_| {
                    anyhow!("{} must hold {KEY_LEN} bytes (found {len})", path.display())
                })
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // Pas de clé + un coffre existant = clés irrécupérables :
                // jamais de nouvelle clé maîtresse, elle les écraserait.
                if !create || self.data_path().exists() {
                    bail!(
                        "{} missing: the existing vault cannot be opened or overwritten",
                        path.display()
                    );
                }
                let mut key = [0u8; KEY_LEN];
                rand::rng().fill_bytes(&mut key);
                write_private(&path, &key)?;
                Ok(key)
            }
            Err(err) => Err(err).with_context(|| format!("reading {}", path.display())),
        }
    }
}

/// Chiffre un coffre : magic + nonce + ciphertext.
pub fn seal(vault: &Vault, key: &[u8; KEY_LEN]) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plain = serde_json::to_vec(vault).context("serializing providers")?;
    let body = cipher
        .encrypt(
            nonce,
            Payload {
                msg: &plain,
                aad: AAD,
            },
        )
        .map_err(|_| anyhow!("encryption failed"))?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + body.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&body);
    Ok(out)
}

/// Déchiffre un coffre ; mauvaise clé ou altération = échec d'authentification.
pub fn open(bytes: &[u8], key: &[u8; KEY_LEN]) -> Result<Vault> {
    let header = MAGIC.len() + NONCE_LEN;
    if bytes.len() <= header || bytes.get(..MAGIC.len()) != Some(MAGIC.as_slice()) {
        bail!("not a providers vault");
    }
    let nonce = Nonce::from_slice(&bytes[MAGIC.len()..header]);
    let cipher = ChaCha20Poly1305::new(key.into());
    let plain = cipher
        .decrypt(
            nonce,
            Payload {
                msg: &bytes[header..],
                aad: AAD,
            },
        )
        .map_err(|_| anyhow!("vault unreadable: wrong master key or altered file"))?;
    serde_json::from_slice(&plain).context("decrypted vault is not valid JSON")
}

/// Décode 64 caractères hex vers la clé maîtresse de 32 octets.
fn decode_hex(hex: &str) -> Result<[u8; KEY_LEN]> {
    let digits = hex.trim();
    if digits.len() != KEY_LEN * 2 {
        bail!("{KEY_ENV} must be {} hex characters", KEY_LEN * 2);
    }
    let mut out = [0u8; KEY_LEN];
    for (i, byte) in out.iter_mut().enumerate() {
        let pair = digits.get(i * 2..i * 2 + 2).unwrap_or("zz");
        *byte =
            u8::from_str_radix(pair, 16).map_err(|_| anyhow!("{KEY_ENV} must be hexadecimal"))?;
    }
    Ok(out)
}

/// Écrit `bytes` à `path` atomiquement, en mode 0600 sur Unix.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| String::from("settings"));
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts
        .open(&tmp)
        .with_context(|| format!("writing {}", tmp.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("writing {}", tmp.display()))?;
    file.sync_all().ok();
    drop(file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod 600 {}", tmp.display()))?;
    }
    fs::rename(&tmp, path).with_context(|| format!("renaming to {}", path.display()))
}

/// Variable d'environnement non vide trimmée.
fn env_var(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier jetable unique pour un test.
    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kh-vault-{}-{name}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        dir
    }

    fn vault_with(key: &str) -> Vault {
        let mut vault = Vault::new();
        vault.last = Some(String::from("openai"));
        vault.providers.insert(
            String::from("openai"),
            Credential {
                base_url: String::from("https://api.openai.com/v1"),
                model: String::from("gpt-4o"),
                key: key.to_string(),
            },
        );
        vault
    }

    #[test]
    fn round_trip_and_file_is_encrypted() {
        let dir = dir("roundtrip");
        let store = Store::new(&dir);
        store.save(&vault_with("[REDACTED]")).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.last.as_deref(), Some("openai"));
        assert_eq!(loaded.providers["openai"].key, "[REDACTED]");
        let raw = fs::read(store.data_path()).unwrap();
        assert!(&raw[..5] == b"KHSE1");
        let text = String::from_utf8_lossy(&raw);
        assert!(!text.contains("[REDACTED]"), "key leaked in clear");
        assert!(!text.contains("api.openai.com"), "endpoint leaked in clear");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn master_key_is_created_once_with_0600() {
        let dir = dir("master");
        let store = Store::new(&dir);
        store.save(&vault_with("sk-a")).unwrap();
        let key = fs::read(store.master_path()).unwrap();
        assert_eq!(key.len(), KEY_LEN);
        // Seconde sauvegarde : même clé réutilisée (pas de rotation silencieuse).
        store.save(&vault_with("sk-b")).unwrap();
        assert_eq!(fs::read(store.master_path()).unwrap(), key);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(store.data_path())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_is_empty_vault() {
        let dir = dir("missing");
        let store = Store::new(&dir);
        let vault = store.load().unwrap();
        assert!(vault.providers.is_empty());
        assert!(vault.last.is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_master_key_never_regenerates_over_a_vault() {
        let dir = dir("orphan");
        let store = Store::new(&dir);
        store.save(&vault_with("sk-keep")).unwrap();
        let vault_bytes = fs::read(store.data_path()).unwrap();
        fs::remove_file(store.master_path()).unwrap();
        assert!(store.load().is_err());
        assert!(store.save(&vault_with("sk-other")).is_err());
        assert!(!store.master_path().exists(), "master key was regenerated");
        assert_eq!(fs::read(store.data_path()).unwrap(), vault_bytes);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn wrong_key_and_tampering_fail() {
        let key = [7u8; KEY_LEN];
        let sealed = seal(&vault_with("sk-x"), &key).unwrap();
        assert!(open(&sealed, &[8u8; KEY_LEN]).is_err());
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0x01;
        assert!(open(&tampered, &key).is_err());
        assert!(open(b"not a vault at all", &key).is_err());
    }

    #[test]
    fn debug_never_shows_the_key() {
        let cred = vault_with("[REDACTED]").providers["openai"].clone();
        let text = format!("{cred:?}");
        assert!(text.contains("[redacted]"));
        assert!(!text.contains("[REDACTED]"));
    }

    #[test]
    fn hex_key_decoding() {
        let hex = "aa".repeat(KEY_LEN);
        let key = decode_hex(&hex).unwrap();
        assert_eq!(key, [0xAA; KEY_LEN]);
        assert!(decode_hex("abcd").is_err());
        assert!(decode_hex(&"zz".repeat(KEY_LEN)).is_err());
    }
}
