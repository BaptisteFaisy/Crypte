//! Moteur d'exécution des hooks Kiro (T3), version Tauri.
//!
//! Reprend de `src/kiro/hook_run.rs` (TUI) la sélection (trigger +
//! `matcher` + `.kiroignore`), l'exécution (`command` = shell async,
//! `agent` = file + annonce) et la garde anti-récursion. Adaptation :
//! le `impl App` devient [`HooksEngine`] (chemins injectés, sorties
//! [`HookOut`] pour l'intégrateur, tombées via canal interne drainé
//! par [`HooksEngine::drain_outcomes`]).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use super::hook_exec::HookOutcome;
use super::hook_match;
use super::hooks::{Hook, Trigger};

/// Déclenchements gardés pour `/hooks`.
const RECENT_MAX: usize = 10;

/// Un déclenchement passé (affiché par `/hooks`).
#[derive(Debug, Clone)]
pub struct HookFire {
    pub name: String,
    pub trigger: Trigger,
    pub status: String,
}

/// État hooks : file agent + historique + gardes.
#[derive(Debug, Default)]
pub struct HookState {
    /// Prompts `agent` en attente (préfixés au prochain envoi).
    pub pending: Vec<String>,
    /// Derniers déclenchements (capés, voir `RECENT_MAX`).
    pub recent: VecDeque<HookFire>,
    /// `SessionStart` déjà tiré (boot, une seule fois).
    pub session_started: bool,
    /// Tir en cours (garde anti-récursion, profondeur max 1).
    pub firing: bool,
}

impl HookState {
    /// État vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enregistre un déclenchement (file capée, vieux sortis).
    fn record(&mut self, fire: HookFire) {
        self.recent.push_back(fire);
        while self.recent.len() > RECENT_MAX {
            self.recent.pop_front();
        }
    }
}

/// Sortie moteur pour l'intégrateur (ligne chat système).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookOut {
    ChatLine { chat: usize, text: String },
}

/// Moteur hooks : état + chemins + canal des tombées shell.
pub struct HooksEngine {
    state: HookState,
    project: PathBuf,
    global: Option<PathBuf>,
    tx: tokio::sync::mpsc::UnboundedSender<HookOutcome>,
    rx: tokio::sync::mpsc::UnboundedReceiver<HookOutcome>,
}

impl HooksEngine {
    /// Moteur sur chemins injectés (tests hermétiques).
    pub fn new(project: PathBuf, global: Option<PathBuf>) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            state: HookState::new(),
            project,
            global,
            tx,
            rx,
        }
    }

    /// Moteur sur les chemins Kiro réels (prod).
    pub fn live() -> Self {
        Self::new(crate::kiro::project_dir(), crate::kiro::global_dir())
    }

    /// État (file + récents, pour `hooks_list`).
    pub fn state(&self) -> &HookState {
        &self.state
    }

    /// Tire les hooks du trigger (cible = texte matché). Silencieux
    /// si aucun. Chemins injectés à la construction (hermétique).
    pub fn fire(&mut self, trigger: Trigger, target: &str, chat: usize) -> Vec<HookOut> {
        if self.state.firing || trigger == Trigger::Unknown {
            return Vec::new();
        }
        let target = relative_target(&self.project, trigger, target);
        if is_file_trigger(trigger) {
            let ignore = crate::kiro::ignore::load(&self.project);
            if ignore.is_ignored(&target) {
                return Vec::new();
            }
        }
        let selected = select_hooks(trigger, &target, &self.project, self.global.as_deref());
        if selected.is_empty() {
            return Vec::new();
        }
        self.state.firing = true;
        let mut outs = Vec::new();
        for hook in &selected {
            self.fire_one(hook, trigger, chat, &mut outs);
        }
        self.state.firing = false;
        outs
    }

    /// Exécute un hook : `command` = shell async, `agent` = file +
    /// annonce ; vide ou type opaque = noté `skipped` ou ignoré.
    fn fire_one(&mut self, hook: &Hook, trigger: Trigger, chat: usize, outs: &mut Vec<HookOut>) {
        match hook.action.kind.as_str() {
            "command" => match hook.action.command.clone() {
                Some(cmd) if !cmd.trim().is_empty() => {
                    self.spawn_command(chat, &hook.name, &cmd, trigger);
                }
                _ => self.state.record(HookFire {
                    name: hook.name.clone(),
                    trigger,
                    status: String::from("skipped: empty command"),
                }),
            },
            "agent" => match hook.action.prompt.clone() {
                Some(prompt) if !prompt.trim().is_empty() => {
                    self.state.pending.push(prompt);
                    self.state.record(HookFire {
                        name: hook.name.clone(),
                        trigger,
                        status: String::from("queued for next send"),
                    });
                    outs.push(HookOut::ChatLine {
                        chat,
                        text: format!("hook {}: prompt queued for next send", hook.name),
                    });
                }
                _ => self.state.record(HookFire {
                    name: hook.name.clone(),
                    trigger,
                    status: String::from("skipped: empty prompt"),
                }),
            },
            _ => {}
        }
    }

    /// Lance une commande hook (`sh -c`, cwd projet, résultat async).
    /// Hors runtime (tests sync) : noté `skipped`, jamais de panique.
    fn spawn_command(&mut self, chat: usize, name: &str, cmd: &str, trigger: Trigger) {
        if tokio::runtime::Handle::try_current().is_err() {
            self.state.record(HookFire {
                name: name.to_string(),
                trigger,
                status: String::from("skipped: no runtime"),
            });
            return;
        }
        super::hook_exec::spawn_command(&self.tx, chat, name, cmd, trigger, &self.project);
    }

    /// Draine les tombées shell arrivées (lignes + historique).
    pub fn drain_outcomes(&mut self) -> Vec<HookOut> {
        let mut outs = Vec::new();
        while let Ok(out) = self.rx.try_recv() {
            outs.push(self.drain_one(out));
        }
        outs
    }

    /// Tombée hook : ligne système sur le chat d'origine + historique.
    fn drain_one(&mut self, out: HookOutcome) -> HookOut {
        let status = if out.ok { "ok" } else { "FAILED" };
        let mut line = format!(
            "hook {}: {status}, {} chars, {} ms",
            out.name,
            out.text.chars().count(),
            out.ms
        );
        if !out.text.trim().is_empty() {
            line.push_str(&format!(" — {}", crate::provider::truncate(&out.text, 200)));
        }
        self.state.record(HookFire {
            name: out.name,
            trigger: out.trigger,
            status: status.to_string(),
        });
        HookOut::ChatLine {
            chat: out.chat,
            text: line,
        }
    }

    /// Préfixe l'input des prompts `agent` en file (file drainée).
    pub fn prefixed_input(&mut self, text: &str) -> String {
        if self.state.pending.is_empty() {
            return text.to_string();
        }
        let queued = std::mem::take(&mut self.state.pending).join("\n\n");
        format!("{queued}\n\n{text}")
    }

    /// `SessionStart` au boot (une seule fois, cible vide : seuls les
    /// hooks sans matcher (ou `.*`) partent).
    pub fn fire_session_start(&mut self, chat: usize) -> Vec<HookOut> {
        if self.state.session_started {
            return Vec::new();
        }
        self.state.session_started = true;
        self.fire(Trigger::SessionStart, "", chat)
    }

    /// `AgentStop` à la tombée (prompt fourni par l'intégrateur).
    pub fn fire_agent_stop(&mut self, prompt: &str, deliver: usize) -> Vec<HookOut> {
        self.fire(Trigger::AgentStop, prompt, deliver)
    }

    /// `AgentSpawn` à la création (cible = prompt, chat d'origine).
    pub fn fire_agent_spawn(&mut self, chat: usize, prompt: &str) -> Vec<HookOut> {
        self.fire(Trigger::AgentSpawn, prompt, chat)
    }
}

/// Hooks du trigger dont le matcher (absent = toujours) matche `target`.
pub(crate) fn select_hooks(
    trigger: Trigger,
    target: &str,
    project: &Path,
    global: Option<&Path>,
) -> Vec<Hook> {
    super::hooks::load_all_in(project, global)
        .into_iter()
        .flat_map(|named| named.hooks)
        .filter(|hook| hook.trigger == trigger)
        .filter(|hook| {
            hook.matcher
                .as_deref()
                .map(|matcher| hook_match::matches(matcher, target))
                .unwrap_or(true)
        })
        .collect()
}

/// Triggers fichiers (seuls soumis à `.kiroignore`).
fn is_file_trigger(trigger: Trigger) -> bool {
    matches!(
        trigger,
        Trigger::PostFileCreate | Trigger::PostFileSave | Trigger::PostFileDelete
    )
}

/// Cible matchée : relative au projet pour les fichiers (les ancres
/// `^src/` marchent), brute sinon.
fn relative_target(project: &Path, trigger: Trigger, target: &str) -> String {
    if !is_file_trigger(trigger) {
        return target.to_string();
    }
    Path::new(target)
        .strip_prefix(project)
        .map(|rel| rel.to_string_lossy().into_owned())
        .unwrap_or_else(|_| target.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("bt-hook-run-{}-{name}", std::process::id()))
    }

    /// Écrit un fichier hook JSON dans `dir/.kiro/hooks/`.
    fn seed(dir: &Path, file: &str, json: &str) {
        let hooks = dir.join(".kiro/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        std::fs::write(hooks.join(file), json).unwrap();
    }

    #[test]
    fn select_filtre_trigger_et_matcher() {
        let dir = tmp_dir("select");
        seed(
            &dir,
            "mix.json",
            r#"{"hooks":[
              {"name":"fmt","trigger":"PostFileSave","matcher":"\\.rs$",
               "action":{"type":"command","command":"cargo fmt"}},
              {"name":"any","trigger":"PostFileSave",
               "action":{"type":"command","command":"echo"}},
              {"name":"other","trigger":"PromptSubmit",
               "action":{"type":"command","command":"echo"}}]}"#,
        );
        let got = select_hooks(Trigger::PostFileSave, "src/main.rs", &dir, None);
        let names: Vec<&str> = got.iter().map(|hook| hook.name.as_str()).collect();
        assert_eq!(names, vec!["fmt", "any"]);
        let got = select_hooks(Trigger::PostFileSave, "notes.md", &dir, None);
        assert_eq!(got.len(), 1);
        assert!(select_hooks(Trigger::PostFileDelete, "x", &dir, None).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ignore_ecarte_les_fichiers_ignores() {
        let dir = tmp_dir("ignore");
        seed(
            &dir,
            "v.json",
            r#"{"hooks":[{"name":"fmt","trigger":"PostFileSave",
               "action":{"type":"agent","prompt":"relis"}}]}"#,
        );
        std::fs::write(dir.join(".kiroignore"), "target/\n").unwrap();
        let mut engine = HooksEngine::new(dir.clone(), None);
        let kept = dir.join("src/main.rs").to_string_lossy().into_owned();
        let outs = engine.fire(Trigger::PostFileSave, &kept, 0);
        assert_eq!(engine.state().pending.len(), 1);
        assert_eq!(outs.len(), 1);
        let ignored = dir.join("target/out.rs").to_string_lossy().into_owned();
        let outs = engine.fire(Trigger::PostFileSave, &ignored, 0);
        assert_eq!(engine.state().pending.len(), 1);
        assert!(outs.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn agent_mis_en_file_puis_prefixe_au_prochain_envoi() {
        let dir = tmp_dir("agent");
        seed(
            &dir,
            "r.json",
            r#"{"hooks":[{"name":"review","trigger":"PromptSubmit",
               "action":{"type":"agent","prompt":"relis le diff"}}]}"#,
        );
        let mut engine = HooksEngine::new(dir.clone(), None);
        engine.fire(Trigger::PromptSubmit, "ajoute un test", 0);
        assert_eq!(engine.state().pending, vec![String::from("relis le diff")]);
        let prefixed = engine.prefixed_input("ajoute un test");
        assert_eq!(prefixed, "relis le diff\n\najoute un test");
        assert!(engine.state().pending.is_empty());
        assert_eq!(engine.prefixed_input("nu"), "nu");
        let out = engine.drain_one(HookOutcome {
            chat: 0,
            name: String::from("fmt"),
            trigger: Trigger::PostFileSave,
            ok: true,
            text: String::from("all good"),
            ms: 12,
        });
        let HookOut::ChatLine { chat, text } = out;
        assert_eq!(chat, 0);
        assert!(text.contains("hook fmt: ok, 8 chars, 12 ms"), "{text}");
        assert!(text.contains("all good"), "{text}");
        assert_eq!(engine.state().recent.len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn garde_anti_recursion_bloque_le_tir_imbrique() {
        let dir = tmp_dir("guard");
        seed(
            &dir,
            "r.json",
            r#"{"hooks":[{"name":"review","trigger":"PromptSubmit",
               "action":{"type":"agent","prompt":"relis"}}]}"#,
        );
        let mut engine = HooksEngine::new(dir.clone(), None);
        engine.state.firing = true;
        engine.fire(Trigger::PromptSubmit, "hello", 0);
        assert!(engine.state.pending.is_empty());
        assert!(engine.state.recent.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn session_start_une_seule_fois() {
        let dir = tmp_dir("session");
        seed(
            &dir,
            "s.json",
            r#"{"hooks":[{"name":"boot","trigger":"SessionStart",
               "action":{"type":"agent","prompt":"contexte"}}]}"#,
        );
        let mut engine = HooksEngine::new(dir.clone(), None);
        engine.fire_session_start(0);
        engine.fire_session_start(0);
        assert_eq!(engine.state.pending.len(), 1);
        assert!(engine.state.session_started);
        std::fs::remove_dir_all(&dir).ok();
    }
}
