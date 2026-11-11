//! Outils navigateur : CLI `agent-browser` (Chromium headed sur `:99`).
//!
//! Le wrapper force l'affichage virtuel (aucun vol de focus) ; chaque
//! crypte tourne dans sa session dédiée ([`session`], jamais la session
//! partagée). Constructeurs d'argv purs (testés), exécutés par `exec`.

use serde_json::json;

use super::wire::ToolDef;

/// Préfixe de la session navigateur (`crypte-<pid>`, dédiée au processus).
pub const SESSION_PREFIX: &str = "crypte";

/// Nom de la variable de session lue par `agent-browser`.
pub const SESSION_ENV: &str = "AGENT_BROWSER_SESSION";

/// Binaire navigateur (résolu via `PATH`, wrapper headed `:99`).
pub const BIN: &str = "agent-browser";

/// Session dédiée au processus (jamais la session partagée par défaut).
pub fn session() -> String {
    format!("{SESSION_PREFIX}-{}", std::process::id())
}

/// `open <url>` : navigue vers l'URL (http/https uniquement).
pub fn open_argv(url: &str) -> Vec<String> {
    vec![String::from("open"), url.to_string()]
}

/// `snapshot -i -c` : éléments interactifs, compact (refs `@eN`).
pub fn snapshot_argv() -> Vec<String> {
    ["snapshot", "-i", "-c"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// `read [url] [--filter f]` : texte agent de l'onglet ou d'une URL.
pub fn read_argv(url: Option<&str>, filter: Option<&str>) -> Vec<String> {
    let mut argv = vec![String::from("read")];
    if let Some(url) = url {
        argv.push(url.to_string());
    }
    if let Some(filter) = filter {
        argv.push(String::from("--filter"));
        argv.push(filter.to_string());
    }
    argv
}

/// `click <ref>` : clique la ref du snapshot (`@eN` ou sélecteur CSS).
pub fn click_argv(target: &str) -> Vec<String> {
    vec![String::from("click"), target.to_string()]
}

/// `fill <ref> <text>` : efface puis saisit dans le champ.
pub fn fill_argv(target: &str, text: &str) -> Vec<String> {
    vec![String::from("fill"), target.to_string(), text.to_string()]
}

/// `type <ref> <text>` : saisit sans effacer (ajout au contenu).
pub fn type_argv(target: &str, text: &str) -> Vec<String> {
    vec![String::from("type"), target.to_string(), text.to_string()]
}

/// `press <key>` : touche (`Enter`, `Tab`, `Escape`, flèches…).
pub fn press_argv(key: &str) -> Vec<String> {
    vec![String::from("press"), key.to_string()]
}

/// `screenshot <path>` : capture du viewport vers le fichier.
pub fn shot_argv(path: &str) -> Vec<String> {
    vec![String::from("screenshot"), path.to_string()]
}

/// Schémas des outils navigateur (lectures + actions).
pub fn defs() -> Vec<ToolDef> {
    let mut defs = read_defs();
    defs.extend(act_defs());
    defs
}

/// Lecteurs navigateur (sans effet : snapshot, lecture, capture).
fn read_defs() -> Vec<ToolDef> {
    vec![
        ToolDef::new(
            "browser_snapshot",
            "List interactive page elements with @eN refs. Call after every navigation before clicking.",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        ToolDef::new(
            "browser_read",
            "Read page text (current tab, or fetch a URL as markdown).",
            json!({"type":"object","properties":{
                "url":{"type":"string","description":"optional URL to fetch (else current tab)"},
                "filter":{"type":"string","description":"optional heading filter"}
            },"additionalProperties":false}),
        ),
        ToolDef::new(
            "browser_shot",
            "Screenshot the viewport to a file. Returns the file path.",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
    ]
}

/// Actions navigateur (naviguent ou modifient la page).
fn act_defs() -> Vec<ToolDef> {
    vec![
        ToolDef::new(
            "browser_open",
            "Navigate the agent browser to an http(s) URL.",
            json!({"type":"object","properties":{
                "url":{"type":"string","description":"http(s) URL to open"}
            },"required":["url"],"additionalProperties":false}),
        ),
        ToolDef::new(
            "browser_click",
            "Click an element by @eN ref from browser_snapshot (or CSS selector).",
            json!({"type":"object","properties":{
                "target":{"type":"string","description":"@eN ref or CSS selector"}
            },"required":["target"],"additionalProperties":false}),
        ),
        ToolDef::new(
            "browser_fill",
            "Clear a field and type text into it (@eN ref). Never passwords or 2FA codes.",
            json!({"type":"object","properties":{
                "target":{"type":"string","description":"@eN ref of the field"},
                "text":{"type":"string","description":"text to type"}
            },"required":["target","text"],"additionalProperties":false}),
        ),
        ToolDef::new(
            "browser_type",
            "Type text without clearing (@eN ref). Never passwords or 2FA codes.",
            json!({"type":"object","properties":{
                "target":{"type":"string","description":"@eN ref of the field"},
                "text":{"type":"string","description":"text to append"}
            },"required":["target","text"],"additionalProperties":false}),
        ),
        ToolDef::new(
            "browser_press",
            "Press a key: Enter, Tab, Escape, Backspace, arrows…",
            json!({"type":"object","properties":{
                "key":{"type":"string","description":"key name"}
            },"required":["key"],"additionalProperties":false}),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_est_dediee_au_processus() {
        assert_eq!(session(), format!("crypte-{}", std::process::id()));
    }

    #[test]
    fn argv_suivent_la_cli() {
        assert_eq!(open_argv("https://x.fr"), ["open", "https://x.fr"]);
        assert_eq!(snapshot_argv(), ["snapshot", "-i", "-c"]);
        assert_eq!(read_argv(None, None), ["read"]);
        assert_eq!(
            read_argv(Some("https://x.fr"), Some("auth")),
            ["read", "https://x.fr", "--filter", "auth"]
        );
        assert_eq!(click_argv("@e3"), ["click", "@e3"]);
        assert_eq!(fill_argv("@e1", "hi"), ["fill", "@e1", "hi"]);
        assert_eq!(press_argv("Enter"), ["press", "Enter"]);
        assert_eq!(shot_argv("/tmp/s.png"), ["screenshot", "/tmp/s.png"]);
    }

    #[test]
    fn huit_outils_navigateur() {
        let defs = defs();
        assert_eq!(defs.len(), 8);
        assert!(defs.iter().any(|d| d.function.name == "browser_snapshot"));
        assert!(defs.iter().any(|d| d.function.name == "browser_open"));
    }
}
