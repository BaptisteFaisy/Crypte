//! Handler `/hooks` (statut moteur hooks, `impl App` déporté).
//!
//! `/hooks` liste les hooks chargés par trigger + la file `agent` +
//! les derniers déclenchements (voir [`super::hook_run::HookState`]).

use super::hook_run::HookState;
use super::hooks::{Action, Hook, NamedHooks};
use crate::app::App;

impl App {
    /// `/hooks` : hooks chargés par trigger + derniers déclenchements.
    pub(crate) fn cmd_hooks(&mut self) {
        let project = crate::kiro::project_dir();
        let global = crate::kiro::global_dir();
        self.cmd_hooks_in(&project, global.as_deref());
    }

    /// `/hooks` à chemins injectés (testé, hermétique).
    fn cmd_hooks_in(&mut self, project: &std::path::Path, global: Option<&std::path::Path>) {
        let loaded = super::hooks::load_all_in(project, global);
        self.push_system(hooks_status(&loaded, &self.hooks));
    }
}

/// Statut `/hooks` (pur, testable) : compteur par trigger + file +
/// détails + récents.
fn hooks_status(loaded: &[NamedHooks], state: &HookState) -> String {
    let count: usize = loaded.iter().map(|named| named.hooks.len()).sum();
    if count == 0 && state.recent.is_empty() {
        return format!(
            "hooks: none loaded · queued prompts {}",
            state.pending.len()
        );
    }
    let mut out = format!(
        "hooks: {} ({}) · queued prompts {}",
        count,
        per_trigger(loaded),
        state.pending.len()
    );
    for named in loaded {
        for hook in &named.hooks {
            out.push_str(&format!("\n  {}", hook_line(&named.file, hook)));
        }
    }
    if !state.recent.is_empty() {
        let recent: Vec<String> = state
            .recent
            .iter()
            .map(|fire| format!("{} {} {}", fire.name, fire.trigger.as_str(), fire.status))
            .collect();
        out.push_str(&format!("\n  recent: {}", recent.join(" · ")));
    }
    out
}

/// Compteurs par trigger (`PostFileSave 2 · PromptSubmit 1`, ordre vu).
fn per_trigger(loaded: &[NamedHooks]) -> String {
    let mut kinds: Vec<(&str, usize)> = Vec::new();
    for named in loaded {
        for hook in &named.hooks {
            let name = hook.trigger.as_str();
            match kinds.iter_mut().find(|(seen, _)| *seen == name) {
                Some((_, count)) => *count += 1,
                None => kinds.push((name, 1)),
            }
        }
    }
    kinds
        .iter()
        .map(|(name, count)| format!("{name} {count}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// `fmt [verify.json] (PostFileSave \.rs$, command: cargo fmt)`.
fn hook_line(file: &str, hook: &Hook) -> String {
    let mut head = hook.trigger.as_str().to_string();
    if let Some(matcher) = &hook.matcher {
        head.push(' ');
        head.push_str(&crate::provider::truncate(matcher, 20));
    }
    format!(
        "{} [{file}] ({head}, {})",
        hook.name,
        action_short(&hook.action)
    )
}

/// `command: cargo test` (ou `agent: …`, `?` si vide).
fn action_short(action: &Action) -> String {
    let detail = action
        .command
        .as_deref()
        .or(action.prompt.as_deref())
        .unwrap_or("?");
    let kind = if action.kind.is_empty() {
        "?"
    } else {
        action.kind.as_str()
    };
    format!("{kind}: {}", crate::provider::truncate(detail, 30))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("kh-hook-cmds-{}-{name}", std::process::id()))
    }

    /// App de test (provider fictif, aucun appel réseau ici).
    fn test_app() -> App {
        let cfg = crate::provider::ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        App::from_config(cfg).unwrap()
    }

    /// Dernière ligne affichée du chat courant.
    fn last_text(app: &App) -> String {
        app.sessions
            .get(0)
            .unwrap()
            .chat
            .messages()
            .last()
            .map(|msg| msg.text.clone())
            .unwrap_or_default()
    }

    #[test]
    fn vide_annonce_rien() {
        let dir = tmp_dir("empty");
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = test_app();
        app.cmd_hooks_in(&dir, None);
        let text = last_text(&app);
        assert!(text.contains("hooks: none loaded"), "{text}");
        assert!(text.contains("queued prompts 0"), "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn garni_liste_triggers_file_et_recents() {
        let dir = tmp_dir("full");
        let hooks = dir.join(".kiro/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        std::fs::write(
            hooks.join("verify.json"),
            r#"{"hooks":[
              {"name":"fmt","trigger":"PostFileSave","matcher":"\\.rs$",
               "action":{"type":"command","command":"cargo fmt"}},
              {"name":"review","trigger":"PromptSubmit",
               "action":{"type":"agent","prompt":"relis le diff"}}]}"#,
        )
        .unwrap();
        let mut app = test_app();
        app.hooks_fire_in(
            crate::kiro::hooks::Trigger::PromptSubmit,
            "hello",
            0,
            &dir,
            None,
        );
        app.cmd_hooks_in(&dir, None);
        let text = last_text(&app);
        for bit in [
            "hooks: 2 (PostFileSave 1 · PromptSubmit 1)",
            "queued prompts 1",
            "fmt [verify.json] (PostFileSave",
            "command: cargo fmt",
            "review [verify.json] (PromptSubmit, agent: relis le diff)",
            "recent: review PromptSubmit queued for next send",
        ] {
            assert!(text.contains(bit), "manque {bit:?} dans :\n{text}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
