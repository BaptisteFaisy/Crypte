//! User images: load, sniff, cache, data URLs.
//!
//! Same on-disk contract as the TUI branch (`src/images.rs` there):
//! same sniffs (PNG, JPEG, GIF, WebP), same 5 MB cap, same cache dir
//! (`~/.config/kiro-horror-tui/images/`), same `data:` URLs — a chat
//! saved by one UI restores in the other. The two branches are
//! mutually exclusive (this one has no TUI crate), so this module is
//! self-contained; merge both into one shared crate when they converge.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};

/// Max raw bytes per image (refused above, with an explicit message).
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
/// Max images staged on a single message.
pub const MAX_IMAGES_PER_MSG: usize = 4;
/// Disk cache, relative to `$HOME` (shared with the TUI branch).
const IMAGES_DIR: &str = ".config/kiro-horror-tui/images";

/// Counter for unique cache file names (one process, no collision).
static STORE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// One attached image: display name + mime + base64 bytes (+ cache file
/// name once stored on disk, `None` until [`ImageFile::store`]).
#[derive(Debug, Clone)]
pub struct ImageFile {
    pub name: String,
    pub mime: String,
    pub b64: String,
    pub size: usize,
    pub file: Option<String>,
}

impl ImageFile {
    /// From raw bytes (size + magic sniffed, base64 encoded).
    pub fn from_bytes(name: &str, bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_IMAGE_BYTES {
            anyhow::bail!(
                "image too large ({} — max {})",
                fmt_size(bytes.len()),
                fmt_size(MAX_IMAGE_BYTES)
            );
        }
        let Some((mime, _)) = sniff_mime(bytes) else {
            anyhow::bail!("unsupported image format (png, jpeg, gif, webp only): {name}");
        };
        Ok(Self {
            name: name.to_string(),
            mime: mime.to_string(),
            b64: b64(bytes),
            size: bytes.len(),
            file: None,
        })
    }

    /// From a disk file (name = file name, bytes sniffed + encoded).
    pub fn load(path: &str) -> Result<Self> {
        let bytes = std::fs::read(path).with_context(|| format!("cannot read image {path}"))?;
        let name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);
        Self::from_bytes(name, &bytes)
    }

    /// From a frontend upload (name + base64 bytes, validated like a file).
    pub fn from_upload(name: &str, encoded: &str) -> Result<Self> {
        let bytes = unb64(encoded).context("upload is not valid base64")?;
        Self::from_bytes(name, &bytes)
    }

    /// Writes the bytes to the disk cache (idempotent: once stored, kept).
    pub fn store(&mut self) -> Result<()> {
        if self.file.is_some() {
            return Ok(());
        }
        let dir = images_dir().context("HOME missing (image cache unavailable)")?;
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("cannot create {}", dir.display()))?;
        let ext = mime_ext(&self.mime);
        let stripe = STORE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let file = format!("{}-{}-{stripe}.{ext}", now_nanos(), std::process::id());
        let bytes = unb64(&self.b64).context("stored image is not valid base64")?;
        std::fs::write(dir.join(&file), bytes)
            .with_context(|| format!("cannot cache image as {file}"))?;
        self.file = Some(file);
        Ok(())
    }

    /// OpenAI `image_url` payload (`data:` URL, base64 inline). Also used
    /// as the frontend `<img>` preview source (same bytes, no re-read).
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.mime, self.b64)
    }
}

/// Standard base64 (alphabet + `=` padding, no line breaks).
pub fn b64(bytes: &[u8]) -> String {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(chunk.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(chunk.get(2).copied().unwrap_or(0));
        out.push(ALPHA[(n >> 18 & 63) as usize] as char);
        out.push(ALPHA[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHA[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHA[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Strict base64 decode (our own encoder output, padding required).
pub fn unb64(text: &str) -> Result<Vec<u8>> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        b'=' => Some(0),
        _ => None,
    };
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        anyhow::bail!("bad base64 length");
    }
    let quads = bytes.len() / 4;
    let mut out = Vec::with_capacity(quads * 3);
    for (qi, quad) in bytes.chunks(4).enumerate() {
        let mut n = 0u32;
        for &c in quad {
            let Some(v) = val(c) else {
                anyhow::bail!("bad base64 character");
            };
            n = (n << 6) | u32::from(v);
        }
        let pad = quad.iter().rev().take_while(|c| **c == b'=').count();
        if pad > 2 || (pad > 0 && qi + 1 != quads) {
            anyhow::bail!("misplaced base64 padding");
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

/// Magic sniff: `(mime, extension)` for PNG, JPEG, GIF, WebP.
fn sniff_mime(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(("image/png", "png"))
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("image/jpeg", "jpg"))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(("image/gif", "gif"))
    } else if bytes.len() > 12 && bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WEBP" {
        Some(("image/webp", "webp"))
    } else {
        None
    }
}

/// Cache file extension for a mime (sniffed values only).
fn mime_ext(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "bin",
    }
}

/// Disk cache dir (`None` when `HOME` is missing).
fn images_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| std::path::Path::new(&home).join(IMAGES_DIR))
}

/// Human size: `B`, `KB` or `MB` (1024-based, one decimal).
fn fmt_size(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// Clock for unique cache names (0 when the clock is unreadable).
fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scoped `HOME` (restored on drop, no test helper crate here).
    struct ScopedHome {
        prev: Option<String>,
    }

    impl ScopedHome {
        fn set(home: &std::path::Path) -> Self {
            let prev = std::env::var("HOME").ok();
            unsafe { std::env::set_var("HOME", home) };
            Self { prev }
        }
    }

    impl Drop for ScopedHome {
        fn drop(&mut self) {
            unsafe {
                match &self.prev {
                    Some(prev) => std::env::set_var("HOME", prev),
                    None => std::env::remove_var("HOME"),
                }
            }
        }
    }

    #[test]
    fn b64_matches_rfc4648_vectors() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foo"), "Zm9v");
        assert_eq!(b64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn unb64_roundtrips_and_rejects() {
        for raw in [&b""[..], b"f", b"fo", b"foo", b"photo bytes \x00\xff"] {
            assert_eq!(unb64(&b64(raw)).unwrap(), raw);
        }
        assert!(unb64("abc").is_err(), "length");
        assert!(unb64("****").is_err(), "alphabet");
        assert!(unb64("Zg==Zg==").is_err(), "padding mid-stream");
    }

    #[test]
    fn sniff_recognises_four_formats() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0];
        assert_eq!(sniff_mime(&png), Some(("image/png", "png")));
        assert_eq!(
            sniff_mime(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(("image/jpeg", "jpg"))
        );
        assert_eq!(sniff_mime(b"GIF89a...."), Some(("image/gif", "gif")));
        let mut webp = *b"RIFF....WEBPVP8 ";
        webp[4..8].copy_from_slice(&[8, 0, 0, 0]);
        assert_eq!(sniff_mime(&webp), Some(("image/webp", "webp")));
        assert_eq!(sniff_mime(b"not an image"), None);
    }

    #[test]
    fn from_bytes_checks_size_and_format() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let part = ImageFile::from_bytes("a.png", &png).unwrap();
        assert_eq!(part.mime, "image/png");
        assert_eq!(part.size, png.len());
        assert!(part.data_url().starts_with("data:image/png;base64,"));
        assert_eq!(fmt_size(8), "8 B");
        let big = vec![0u8; MAX_IMAGE_BYTES + 1];
        assert!(ImageFile::from_bytes("big.png", &big).is_err());
        assert!(ImageFile::from_bytes("x.txt", b"hello").is_err());
        assert!(ImageFile::from_upload("a.png", &b64(&png)).is_ok());
        assert!(ImageFile::from_upload("a.png", "***").is_err());
    }

    #[test]
    fn store_caches_on_disk_under_scoped_home() {
        let home = std::env::temp_dir().join(format!("kh-tauri-img-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let _scoped = ScopedHome::set(&home);
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
        let mut part = ImageFile::from_bytes("a.png", &png).unwrap();
        part.store().unwrap();
        let file = part.file.clone().unwrap();
        assert!(home.join(IMAGES_DIR).join(&file).exists());
        part.store().unwrap();
        assert_eq!(part.file.as_deref(), Some(file.as_str()), "idempotent");
        std::fs::remove_dir_all(&home).ok();
    }
}
