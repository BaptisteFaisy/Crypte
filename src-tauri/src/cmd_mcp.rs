//! Commandes §6 (MCP) : `mcp_status` (cache, jamais bloquant) +
//! `mcp_refresh` (reconnecte tout, event `mcp_refreshed`).

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, State};

use crate::state::{lock, ToolsBackend};
use crate::views::{McpServerView, McpStatusView, UiEvent, UI_CHANNEL};

/// `mcp_status` : serveurs configurés (cache seul, comme `/mcp`).
#[tauri::command]
pub fn mcp_status(backend: State<Mutex<ToolsBackend>>) -> Result<McpStatusView, String> {
    let backend = lock(&backend)?;
    Ok(mcp_view(&backend))
}

/// `mcp_refresh` : arrêt + relecture config + reconnexion, puis event.
#[tauri::command]
pub async fn mcp_refresh(
    app: AppHandle,
    backend: State<'_, Mutex<ToolsBackend>>,
) -> Result<McpStatusView, String> {
    let mcp = lock(&backend)?.mcp.clone();
    mcp.refresh().await;
    let backend = lock(&backend)?;
    let view = mcp_view(&backend);
    let _ = app.emit(
        UI_CHANNEL,
        UiEvent::McpRefreshed {
            status: view.clone(),
        },
    );
    Ok(view)
}

/// Vue contrat depuis le cache du parc.
fn mcp_view(backend: &ToolsBackend) -> McpStatusView {
    McpStatusView {
        servers: backend
            .mcp
            .status_struct()
            .into_iter()
            .map(|(name, enabled, tools)| McpServerView {
                name,
                enabled,
                tools,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statut_vide_sans_config() {
        let dir = std::env::temp_dir().join(format!("bt-cmd-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let backend = ToolsBackend::new_in(&dir);
        let view = mcp_view(&backend);
        assert!(view.servers.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
