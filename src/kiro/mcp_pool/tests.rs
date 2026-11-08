use super::*;
use std::collections::HashMap;

/// Config à deux serveurs (`old` désactivé, donc exclu).
fn cfg_two() -> Config {
    let live = Server {
        command: Some(String::from("kh-mcp-no-such-bin")),
        auto_approve: vec![String::from("search")],
        ..Default::default()
    };
    let dead = Server {
        disabled: true,
        ..Default::default()
    };
    Config {
        mcp_servers: HashMap::from([(String::from("gh"), live), (String::from("old"), dead)]),
    }
}

/// Config vide (aucun serveur).
fn cfg_empty() -> Config {
    Config {
        mcp_servers: HashMap::new(),
    }
}

#[test]
fn vide_sans_config() {
    let state = McpState::for_config(cfg_empty());
    assert!(state.status().contains("no servers"), "{}", state.status());
    assert!(!state.is_auto("mcp_gh_search"));
    assert!(!state.is_auto("shell_run"));
}

#[tokio::test]
async fn defs_natifs_seuls_et_appel_inconnu() {
    let state = McpState::for_config(cfg_empty());
    assert_eq!(state.tooled_defs().await.len(), 14);
    let out = state.call(0, "mcp_gh_search", "{}").await;
    assert!(!out.ok);
    assert!(out.text.contains("unknown MCP tool"), "{}", out.text);
    let bad = state.call(0, "mcp_gh_search", "nope").await;
    assert!(!bad.ok);
    assert!(bad.text.contains("invalid JSON"), "{}", bad.text);
}

#[test]
fn statut_liste_enabled_seuls() {
    let state = McpState::for_config(cfg_two());
    let text = state.status();
    assert!(text.contains("1 server"), "{text}");
    assert!(text.contains("gh (stdio, not connected)"), "{text}");
    assert!(!text.contains("old"), "{text}");
}

#[tokio::test]
async fn refresh_reconnecte_et_arret_propre() {
    let state = McpState::for_config(cfg_empty());
    state.shutdown().await;
    let text = state.refresh_with(cfg_two()).await;
    assert!(text.contains("1 server"), "{text}");
    assert!(text.contains("gh (stdio, error: "), "{text}");
    let back = state.refresh_with(cfg_empty()).await;
    assert!(back.contains("no servers"), "{back}");
}

/// Stub MCP stdio partagé (voir `mcp_lists`, un dossier par test).
fn stub_server(tag: &str) -> (McpState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("kh-mcp-stub-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let stub = crate::kiro::mcp_lists::write_stub(&dir, tag);
    let server = Server {
        command: Some(stub.to_string_lossy().into_owned()),
        ..Default::default()
    };
    let state = McpState::for_config(Config {
        mcp_servers: HashMap::from([(String::from("stub"), server)]),
    });
    (state, dir)
}

/// Listes via stub : resource + prompt schématisés.
#[tokio::test]
async fn stub_lists_resources_prompts() {
    let (state, dir) = stub_server("listes");
    let resources = state.resources().await;
    assert_eq!(resources.len(), 1);
    let found = resources[0].1.as_ref().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].uri, "stub://doc");
    let prompts = state.prompts().await;
    let listed = prompts[0].1.as_ref().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "revue");
    assert!(listed[0].arguments[0].required);
    state.shutdown().await;
    std::fs::remove_dir_all(&dir).ok();
}

/// Lecture + prompt via stub : contenu et messages rendus.
#[tokio::test]
async fn stub_read_prompt() {
    let (state, dir) = stub_server("lire");
    let content = state.read("stub", "stub://doc").await.unwrap();
    assert_eq!(content.text, "hello stub");
    assert_eq!(content.mime.as_deref(), Some("text/plain"));
    let text = state
        .prompt("stub", "revue", serde_json::json!({"fichier": "a"}))
        .await
        .unwrap();
    assert_eq!(text, "user: Relis.");
    state.shutdown().await;
    std::fs::remove_dir_all(&dir).ok();
}

/// Serveur inconnu ou mort : erreurs claires, jamais de panic.
#[tokio::test]
async fn read_prompt_erreurs_claires() {
    let (state, dir) = stub_server("erreurs");
    assert!(state
        .read("nope", "x")
        .await
        .unwrap_err()
        .contains("unknown MCP server"));
    assert!(state
        .prompt("nope", "x", serde_json::json!({}))
        .await
        .unwrap_err()
        .contains("unknown MCP server"));
    let dead = McpState::for_config(Config {
        mcp_servers: HashMap::from([(
            String::from("mort"),
            Server {
                command: Some(String::from("kh-mcp-no-such-bin")),
                ..Default::default()
            },
        )]),
    });
    let resources = dead.resources().await;
    assert!(resources[0].1.is_err());
    assert!(dead
        .read("mort", "x")
        .await
        .unwrap_err()
        .contains("not connected"));
    state.shutdown().await;
    std::fs::remove_dir_all(&dir).ok();
}
