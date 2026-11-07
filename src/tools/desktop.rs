//! Outils bureau réel (`:0`, XFCE/X11) : fenêtres, clavier, capture.
//!
//! Visible par l'utilisateur (chaque focus/saisie vole l'écran) : lister
//! et capturer sont libres, le reste demande confirmation en mode `auto`.
//! Constructeurs d'argv purs (testés), exécutés par `exec` (`DISPLAY=:0`).

use serde_json::json;

use super::wire::ToolDef;

/// Affichage réel de l'utilisateur (jamais le virtuel `:99`).
pub const DISPLAY: &str = ":0";

/// Binaires bureau (résolus via `PATH`, user-space, sans sudo).
pub const WMCTRL_BIN: &str = "wmctrl";
pub const XDOTOOL_BIN: &str = "xdotool";
pub const SHOOTER_BIN: &str = "xfce4-screenshooter";

/// `wmctrl -l` : fenêtres (id, bureau, titre).
pub fn windows_argv() -> Vec<String> {
    vec![String::from("-l")]
}

/// `wmctrl -a <query>` : amène la fenêtre au premier plan.
pub fn focus_argv(query: &str) -> Vec<String> {
    vec![String::from("-a"), query.to_string()]
}

/// `xdotool key --clearmodifiers …` : frappe des touches.
pub fn key_argv(keys: &[String]) -> Vec<String> {
    let mut argv = vec![String::from("key"), String::from("--clearmodifiers")];
    argv.extend(keys.iter().cloned());
    argv
}

/// `xdotool type --delay 12 …` : saisit du texte (jamais de mot de passe).
pub fn type_argv(text: &str) -> Vec<String> {
    vec![
        String::from("type"),
        String::from("--delay"),
        String::from("12"),
        text.to_string(),
    ]
}

/// `xfce4-screenshooter -f|-w -s <path>` : capture sans presse-papier ni
/// notification (contrairement au script `shot`, trop bavard pour un agent).
pub fn shot_argv(window: bool, path: &str) -> Vec<String> {
    vec![
        if window {
            String::from("-w")
        } else {
            String::from("-f")
        },
        String::from("-s"),
        path.to_string(),
    ]
}

/// Schémas des outils bureau (2 lecteurs + 3 actions).
pub fn defs() -> Vec<ToolDef> {
    vec![
        ToolDef::new(
            "desktop_windows",
            "List open windows on the user's real screen (id, title).",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        ToolDef::new(
            "desktop_shot",
            "Screenshot the real screen to a file. Returns the file path.",
            json!({"type":"object","properties":{
                "window":{"type":"boolean","description":"active window only (default: full screen)"}
            },"additionalProperties":false}),
        ),
        ToolDef::new(
            "desktop_focus",
            "Bring a window to the front (visible to the user).",
            json!({"type":"object","properties":{
                "query":{"type":"string","description":"window title substring"}
            },"required":["query"],"additionalProperties":false}),
        ),
        ToolDef::new(
            "desktop_key",
            "Press keys on the real desktop (space-separated, e.g. 'ctrl+l').",
            json!({"type":"object","properties":{
                "keys":{"type":"string","description":"space-separated key names"}
            },"required":["keys"],"additionalProperties":false}),
        ),
        ToolDef::new(
            "desktop_type",
            "Type text on the real desktop. Never passwords or 2FA codes.",
            json!({"type":"object","properties":{
                "text":{"type":"string","description":"text to type"}
            },"required":["text"],"additionalProperties":false}),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_suivent_les_clis() {
        assert_eq!(windows_argv(), ["-l"]);
        assert_eq!(focus_argv("Gmail"), ["-a", "Gmail"]);
        assert_eq!(
            key_argv(&[String::from("ctrl+l")]),
            ["key", "--clearmodifiers", "ctrl+l"]
        );
        assert_eq!(type_argv("hi"), ["type", "--delay", "12", "hi"]);
        assert_eq!(shot_argv(false, "/tmp/s.png"), ["-f", "-s", "/tmp/s.png"]);
        assert_eq!(shot_argv(true, "/tmp/s.png"), ["-w", "-s", "/tmp/s.png"]);
    }

    #[test]
    fn cinq_outils_bureau() {
        assert_eq!(defs().len(), 5);
    }
}
