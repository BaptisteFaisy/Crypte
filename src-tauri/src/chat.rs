//! Staged images + message send (Tauri commands).
//!
//! The frontend stages pictures ([`attach_image_bytes`] from a file
//! picker, [`attach_image_path`] from a pasted path), previews them
//! ([`staged_images`], `dataUrl` thumbnails), then [`send_message`]
//! ships one imaged turn to the provider and consumes the staged set
//! (same stage → send → clear flow as the TUI branch `/image`).

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tauri::State;

use crate::media::{ImageFile, MAX_IMAGES_PER_MSG};
use crate::provider::{self, ProviderConfig, ProviderStatus, WireMsg};

/// Staged images of the current message (ids + bytes + data URLs).
#[derive(Debug, Default)]
pub struct Staged {
    next_id: u64,
    images: HashMap<u64, StagedImage>,
}

/// One staged image: file bytes (the map key is the id).
#[derive(Debug, Clone)]
struct StagedImage {
    file: ImageFile,
}

/// Staged image for the frontend (id + label + preview URL).
#[derive(Debug, Clone, Serialize)]
pub struct StagedView {
    id: u64,
    name: String,
    mime: String,
    size: usize,
    #[serde(rename = "dataUrl")]
    data_url: String,
}

/// Send result for the frontend: reply + billed usage + images used.
#[derive(Debug, Clone, Serialize)]
pub struct SendResult {
    reply: String,
    prompt: u64,
    completion: u64,
    #[serde(rename = "imagesUsed")]
    images_used: usize,
}

/// Provider availability for the frontend (key presence, no secret).
#[derive(Debug, Clone, Serialize)]
pub struct ProviderState {
    configured: bool,
    #[serde(flatten)]
    status: Option<ProviderStatus>,
}

impl Staged {
    /// Stages a validated file (quota checked, cached on disk).
    fn stage(&mut self, mut file: ImageFile) -> anyhow::Result<StagedView> {
        if self.images.len() >= MAX_IMAGES_PER_MSG {
            anyhow::bail!("{MAX_IMAGES_PER_MSG} images max per message — send first");
        }
        file.store()?;
        self.next_id += 1;
        let id = self.next_id;
        let view = StagedView {
            id,
            name: file.name.clone(),
            mime: file.mime.clone(),
            size: file.size,
            data_url: file.data_url(),
        };
        self.images.insert(id, StagedImage { file });
        Ok(view)
    }

    /// Takes all staged images (send consumes the set).
    fn take_all(&mut self) -> Vec<ImageFile> {
        let mut ids: Vec<u64> = self.images.keys().copied().collect();
        ids.sort_unstable();
        ids.into_iter()
            .filter_map(|id| self.images.remove(&id).map(|staged| staged.file))
            .collect()
    }
}

/// Stages a file-picker upload (name + base64 bytes, validated + cached).
#[tauri::command]
pub fn attach_image_bytes(
    staged: State<'_, Mutex<Staged>>,
    name: String,
    b64: String,
) -> Result<StagedView, String> {
    let file = ImageFile::from_upload(&name, &b64).map_err(|err| err.to_string())?;
    stage_file(&staged, file)
}

/// Stages a pasted disk path (validated + cached, automation-friendly).
#[tauri::command]
pub fn attach_image_path(
    staged: State<'_, Mutex<Staged>>,
    path: String,
) -> Result<StagedView, String> {
    let file = ImageFile::load(&path).map_err(|err| err.to_string())?;
    stage_file(&staged, file)
}

/// Stages under the state lock (single choke point, quota enforced).
fn stage_file(staged: &State<'_, Mutex<Staged>>, file: ImageFile) -> Result<StagedView, String> {
    staged
        .lock()
        .map_err(|_| String::from("staged state poisoned"))?
        .stage(file)
        .map_err(|err| err.to_string())
}

/// Removes one staged image (`true` when it was there).
#[tauri::command]
pub fn remove_image(staged: State<'_, Mutex<Staged>>, id: u64) -> Result<bool, String> {
    let mut staged = staged
        .lock()
        .map_err(|_| String::from("staged state poisoned"))?;
    Ok(staged.images.remove(&id).is_some())
}

/// Clears the staged set (unsent bytes stay cached on disk).
#[tauri::command]
pub fn clear_images(staged: State<'_, Mutex<Staged>>) -> Result<(), String> {
    let mut staged = staged
        .lock()
        .map_err(|_| String::from("staged state poisoned"))?;
    staged.images.clear();
    Ok(())
}

/// Lists the staged images (id order, preview URLs included).
#[tauri::command]
pub fn staged_images(staged: State<'_, Mutex<Staged>>) -> Result<Vec<StagedView>, String> {
    let staged = staged
        .lock()
        .map_err(|_| String::from("staged state poisoned"))?;
    let mut ids: Vec<u64> = staged.images.keys().copied().collect();
    ids.sort_unstable();
    Ok(ids
        .into_iter()
        .filter_map(|id| {
            staged.images.get(&id).map(|staged| StagedView {
                id,
                name: staged.file.name.clone(),
                mime: staged.file.mime.clone(),
                size: staged.file.size,
                data_url: staged.file.data_url(),
            })
        })
        .collect())
}

/// Provider availability (key presence + base/model, never the key).
#[tauri::command]
pub fn provider_status() -> ProviderState {
    match ProviderConfig::from_env() {
        Ok(config) => ProviderState {
            configured: true,
            status: Some(config.status()),
        },
        Err(_) => ProviderState {
            configured: false,
            status: None,
        },
    }
}

/// Builds the single user turn to send (plain text, or parts with the
/// staged data URLs — same JSON as the TUI branch).
pub fn build_turn(text: &str, images: &[ImageFile]) -> WireMsg {
    if images.is_empty() {
        WireMsg::text("user", text)
    } else {
        let urls: Vec<String> = images.iter().map(ImageFile::data_url).collect();
        WireMsg::user_rich(text, &urls)
    }
}

/// Sends the staged set + text as one imaged turn (non-streaming),
/// consumes the staged set, returns reply + billed usage.
#[tauri::command]
pub async fn send_message(
    staged: State<'_, Mutex<Staged>>,
    text: String,
) -> Result<SendResult, String> {
    let config = ProviderConfig::from_env().map_err(|err| err.to_string())?;
    let images = staged
        .lock()
        .map_err(|_| String::from("staged state poisoned"))?
        .take_all();
    if text.trim().is_empty() && images.is_empty() {
        return Err(String::from("nothing to send (text or image required)"));
    }
    let turn = build_turn(&text, &images);
    let done = provider::complete(&config, std::slice::from_ref(&turn))
        .await
        .map_err(|err| err.to_string())?;
    Ok(SendResult {
        reply: done.text,
        prompt: done.prompt_tokens,
        completion: done.completion_tokens,
        images_used: images.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Staged PNG bytes (magic only — `stage` never decodes).
    fn png() -> Vec<u8> {
        vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3]
    }

    #[test]
    fn stage_enforces_quota_and_take_all_consumes() {
        let home = std::env::temp_dir().join(format!("kh-tauri-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let prev = std::env::var("HOME").ok();
        unsafe { std::env::set_var("HOME", &home) };
        let mut staged = Staged::default();
        for i in 0..MAX_IMAGES_PER_MSG {
            let file = ImageFile::from_bytes(&format!("a{i}.png"), &png()).unwrap();
            let view = staged.stage(file).unwrap();
            assert_eq!(view.id as usize, i + 1);
            assert!(view.data_url.starts_with("data:image/png;base64,"));
        }
        let extra = ImageFile::from_bytes("full.png", &png()).unwrap();
        assert!(staged.stage(extra).is_err(), "quota enforced");
        let taken = staged.take_all();
        assert_eq!(taken.len(), MAX_IMAGES_PER_MSG);
        assert!(staged.images.is_empty(), "send consumes the set");
        assert!(staged.take_all().is_empty());
        unsafe {
            match prev {
                Some(prev) => std::env::set_var("HOME", prev),
                None => std::env::remove_var("HOME"),
            }
        }
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn build_turn_is_plain_or_parts() {
        let plain = build_turn("hi", &[]);
        let raw = serde_json::to_value(&plain).unwrap();
        assert_eq!(raw["content"], serde_json::json!("hi"));
        let file = ImageFile::from_bytes("a.png", &png()).unwrap();
        let rich = build_turn("look", std::slice::from_ref(&file));
        let raw = serde_json::to_value(&rich).unwrap();
        assert_eq!(raw["content"][0]["type"], "text");
        assert_eq!(raw["content"][1]["type"], "image_url");
    }
}
