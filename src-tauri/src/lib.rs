//! Crypte (Tauri rebuild): image slice backend.
//!
//! Staged pictures + one-shot imaged send (`chat`), media validation
//! (`media`), OpenAI-compatible client (`provider`). The `greet` demo
//! is gone — every command below is the real slice.

mod chat;
mod media;
mod provider;

use std::sync::Mutex;

use chat::Staged;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(Staged::default()))
        .invoke_handler(tauri::generate_handler![
            chat::attach_image_bytes,
            chat::attach_image_path,
            chat::remove_image,
            chat::clear_images,
            chat::staged_images,
            chat::provider_status,
            chat::send_message,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
