//! Handler `/permissions` + porte des outils (`impl App` déporté).
//!
//! Séparé de [`crate::permissions`] (plafond 500 lignes) : le module parent
//! porte les types purs (mode, règles, persistance), celui-ci l'état `App`
//! (handler + porte `allow`/`ask`/`deny`, voir `tool_gate`).

use std::path::PathBuf;

use crate::app::App;
use crate::chat::Role;
use crate::permissions::{save_rules_to, save_to, Decision, Mode, Rule, Rules};

/// Rappel d'usage (`/permissions` mal formé).
const PERMISSIONS_USAGE: &str =
    "usage: /permissions [yolo|auto|<tool> <allow|ask|deny>|list|reset]";
/// Valeurs de règle acceptées, pour les messages d'erreur.
const RULE_VALUES: &str = "allow, ask, deny";

impl App {
    /// `/permissions` : mode seul (affiche), mode (`yolo|auto`), règle
    /// (`<outil> <allow|ask|deny>`), `list` (règles), `reset` (vide les
    /// règles) ; mode et règles persistés (en anglais).
    pub(crate) fn cmd_permissions(&mut self, args: Vec<String>) {
        let words: Vec<&str> = args.iter().map(String::as_str).collect();
        match words.as_slice() {
            [] => self.push_system(permissions_line(self.permissions, &self.t5.rules)),
            ["list"] => self.push_system(rules_text(&self.t5.rules)),
            ["reset"] => {
                self.t5.rules = Rules::default();
                let mut msg = String::from("tool rules cleared");
                msg.push_str(&save_rules_note(&self.t5.rules_path, &self.t5.rules));
                self.push_system(msg);
            }
            [one] => match Mode::parse(one) {
                Some(mode) => {
                    self.permissions = mode;
                    let mut msg = permissions_line(mode, &self.t5.rules);
                    match &self.permissions_path {
                        Some(p) => {
                            if let Err(err) = save_to(p, mode) {
                                msg.push_str(&format!(" — could not save: {err}"));
                            }
                        }
                        None => msg.push_str(" — not persisted (HOME missing)"),
                    }
                    self.push_system(msg);
                }
                None => self.push_system(format!("invalid mode: {one} — values: yolo, auto")),
            },
            [tool, rule] => match Rule::parse(rule) {
                Some(rule) => {
                    self.t5.rules.set(tool, rule);
                    let mut msg = format!("{tool} → {}", rule.label());
                    msg.push_str(&save_rules_note(&self.t5.rules_path, &self.t5.rules));
                    self.push_system(msg);
                }
                None => self.push_system(format!("invalid rule: {rule} — values: {RULE_VALUES}")),
            },
            _ => self.push_system(String::from(PERMISSIONS_USAGE)),
        }
    }

    /// Porte des appels d'outils : `allow` → exécute, `ask` (ou défaut du
    /// mode) → confirme sauf `autoApprove` MCP, `deny` → refuse sans y/n
    /// (le modèle est informé, la boucle avance).
    pub(crate) fn tool_gate(&mut self, call: crate::tools::ToolCall) {
        match self.t5.rules.decide(self.permissions, &call.name) {
            Decision::Ask if !self.mcp.is_auto(&call.name) => self.tool_ask(&call),
            Decision::Deny => {
                if let Some(chat) = self.tool_pop_front() {
                    if let Some(sess) = self.sessions.get_mut(chat) {
                        sess.chat
                            .push(Role::System, format!("denied {} (rule)", call.name));
                    }
                    self.tool_delivered(vec![crate::tools::flow::Delivery {
                        call,
                        ok: false,
                        text: String::from("denied by permissions rule"),
                        ms: 0,
                    }]);
                }
            }
            Decision::Ask | Decision::Allow => {
                if let Some(chat) = self.tool_pop_front() {
                    self.tool_spawn_exec(chat, call);
                }
            }
        }
    }

    /// Dépile l'appel en tête de la boucle outils (rend le chat d'origine).
    fn tool_pop_front(&mut self) -> Option<usize> {
        self.tool_run.as_mut().map(|run| {
            run.queue.pop_front();
            run.chat
        })
    }
}

/// Ligne d'affichage du mode : `permissions: auto (asks …)` (+ note règles).
fn permissions_line(mode: Mode, rules: &Rules) -> String {
    let mut line = format!("permissions: {} ({})", mode.label(), mode.describe());
    if !rules.entries.is_empty() {
        line.push_str(" — tool rules set (/permissions list)");
    }
    line
}

/// Liste des règles (`no tool rules` + aide si vide, en anglais).
fn rules_text(rules: &Rules) -> String {
    if rules.entries.is_empty() {
        return String::from("no tool rules — /permissions <tool> <allow|ask|deny>");
    }
    let mut lines = vec![String::from("tool rules:")];
    lines.extend(
        rules
            .entries
            .iter()
            .map(|(pattern, rule)| format!("  {pattern} → {}", rule.label())),
    );
    lines.join("\n")
}

/// Persiste les règles, rend la note d'échec éventuelle (vide si OK).
fn save_rules_note(path: &Option<PathBuf>, rules: &Rules) -> String {
    match path {
        Some(p) => match save_rules_to(p, rules) {
            Ok(()) => String::new(),
            Err(err) => format!(" — could not save: {err}"),
        },
        None => String::from(" — not persisted (HOME missing)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::{load_rules_from, Decision};
    use std::collections::VecDeque;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kh-perms-cmds-{name}"))
    }

    /// App de test (provider fictif : `localhost:9` refuse tout).
    fn test_app() -> App {
        let cfg = crate::provider::ProviderConfig {
            base_url: String::from("http://localhost:9/"),
            api_key: String::from("factice"),
            model: String::from("factice"),
        };
        App::from_config(cfg).unwrap()
    }

    #[tokio::test]
    async fn gate_deny_refuse_sans_prompt_et_previent_le_modele() {
        let mut app = test_app();
        app.permissions = Mode::Auto;
        app.t5.rules.set("shell_run", Rule::Deny);
        let call = crate::tools::ToolCall {
            id: String::from("c1"),
            name: String::from("shell_run"),
            args: String::from("{}"),
        };
        app.tool_run = Some(crate::tools::flow::ToolRun {
            chat: 0,
            wire: Vec::new(),
            queue: VecDeque::from([call.clone()]),
            confirming: false,
        });
        app.tool_gate(call);
        let run = app.tool_run.as_ref().expect("boucle conservée");
        assert!(run.queue.is_empty());
        assert!(!run.confirming);
        let texts: Vec<String> = app
            .sessions
            .get(0)
            .unwrap()
            .chat
            .messages()
            .iter()
            .map(|msg| msg.text.clone())
            .collect();
        assert!(
            texts.iter().any(|t| t.contains("denied shell_run (rule)")),
            "{texts:?}"
        );
        assert!(texts.iter().any(|t| t.contains("FAILED")), "{texts:?}");
        assert!(run
            .wire
            .iter()
            .any(|msg| msg.content.as_deref() == Some("denied by permissions rule")));
    }

    #[test]
    fn commande_regle_list_reset_et_rejets() {
        let mut app = test_app();
        let dir = tmp_dir("cmd");
        std::fs::remove_dir_all(&dir).ok();
        app.t5.rules_path = Some(dir.join("permissions-rules"));
        app.run_command("/permissions shell_run deny");
        assert_eq!(last_text(&app), "shell_run → deny");
        assert_eq!(
            load_rules_from(&dir.join("permissions-rules")).decide(Mode::Yolo, "shell_run"),
            Decision::Deny
        );
        app.run_command("/permissions list");
        assert!(last_text(&app).contains("shell_run → deny"));
        app.run_command("/permissions shell_run maybe");
        assert!(last_text(&app).starts_with("invalid rule: maybe"));
        app.run_command("/permissions a b c");
        assert!(last_text(&app).starts_with("usage: /permissions"));
        app.run_command("/permissions reset");
        assert_eq!(last_text(&app), "tool rules cleared");
        assert!(app.t5.rules.entries.is_empty());
        app.run_command("/permissions list");
        assert!(last_text(&app).starts_with("no tool rules"));
        std::fs::remove_dir_all(&dir).ok();
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
}
