//! Encrypted provider vault: `providers.enc` + a local 32-byte master key.
//!
//! File layout: `KHSE1\0\0\0` (8) + nonce (12) + ChaCha20-Poly1305 ciphertext
//! of the JSON vault, with a fixed associated-data tag. The master key lives
//! in `master.key` (mode 0600) or comes from `KH_SETTINGS_KEY` (64 hex chars).
//! API keys are never written in clear, never printed, never logged.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};

/// Binary format tag (first bytes of every vault file).
const MAGIC: &[u8; 8] = b"KHSE1\0\0\0";
/// Nonce length (ChaCha20-Poly1305).
const NONCE_LEN: usize = 12;
/// Master key length.
const KEY_LEN: usize = 32;
/// Associated data: a vault copied elsewhere is not silently reopened.
const AAD: &[u8] = b"kiro-horror-tui/providers/v1";
/// Environment override for the master key (64 hex chars).
pub const KEY_ENV: &str = "KH_SETTINGS_KEY";

/// One saved provider: endpoint, model and API key (never displayed).
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Credential {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub key: String,
}

// Manual Debug: the key must never reach a log or a panic message.
impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("key", &"[redacted]")
            .finish()
    }
}

/// Decrypted vault contents.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Vault {
    #[serde(default = "current_version")]
    pub version: u8,
    /// Provider connected last (used at startup when no env/--config).
    #[serde(default)]
    pub last: Option<String>,
    #[serde(default)]
    pub providers: BTreeMap<String, Credential>,
}

/// Current vault schema version.
fn current_version() -> u8 {
    1
}

impl Vault {
    /// Empty vault at the current version.
    pub fn new() -> Self {
        Self {
            version: current_version(),
            ..Self::default()
        }
    }
}

/// The store: one directory holding the vault and its master key.
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// `$XDG_DATA_HOME/kiro-horror-tui`, else `$HOME/.local/share/kiro-horror-tui`.
    pub fn default_dir() -> Result<PathBuf> {
        if let Some(dir) = env_var("XDG_DATA_HOME") {
            return Ok(PathBuf::from(dir).join("kiro-horror-tui"));
        }
        let home = env_var("HOME").context("HOME missing: settings store path unknown")?;
        Ok(PathBuf::from(home).join(".local/share/kiro-horror-tui"))
    }

    /// Store rooted at `dir` (tests use a temp dir).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Store in the default data directory.
    pub fn default_store() -> Result<Self> {
        Ok(Self::new(Self::default_dir()?))
    }

    /// Encrypted vault file.
    pub fn data_path(&self) -> PathBuf {
        self.dir.join("providers.enc")
    }

    /// Local master key file (32 raw bytes, mode 0600).
    pub fn master_path(&self) -> PathBuf {
        self.dir.join("master.key")
    }

    /// Reads and decrypts the vault; a missing file is an empty vault.
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

    /// Encrypts and writes the vault (atomic: temp file then rename, 0600).
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

    /// Master key: `KH_SETTINGS_KEY` (hex), else `master.key` (created once,
    /// on save only). An existing key of the wrong size is an error, never
    /// regenerated — same for a missing key while a vault exists: a new one
    /// would silently orphan every saved API key.
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
                // No key + an existing vault = the keys are unrecoverable:
                // never mint a new master key, it would overwrite them.
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

/// Encrypts a vault: magic + nonce + ciphertext.
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

/// Decrypts a vault; wrong key or tampering both fail authentication.
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

/// Decodes 64 hex characters into the 32-byte master key.
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

/// Writes `bytes` to `path` atomically, with mode 0600 on Unix.
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

/// Non-empty trimmed environment variable.
fn env_var(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unique throwaway directory for one test.
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
        store.save(&vault_with("sk-secret-123")).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.last.as_deref(), Some("openai"));
        assert_eq!(loaded.providers["openai"].key, "sk-secret-123");
        let raw = fs::read(store.data_path()).unwrap();
        assert!(&raw[..5] == b"KHSE1");
        let text = String::from_utf8_lossy(&raw);
        assert!(!text.contains("sk-secret-123"), "key leaked in clear");
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
        // Second save reuses the same key (no silent rotation).
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
        let cred = vault_with("sk-hidden-42").providers["openai"].clone();
        let text = format!("{cred:?}");
        assert!(text.contains("[redacted]"));
        assert!(!text.contains("sk-hidden-42"));
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
