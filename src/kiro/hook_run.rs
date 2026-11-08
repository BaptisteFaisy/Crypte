//! Moteur d'exécution des hooks Kiro (T3).
//!
//! Sélection (trigger + `matcher` regex via [`super::hook_match`] +
//! `.kiroignore` pour les triggers fichiers), exécution : `command` =
//! shell async `sh -c` (timeout 30 s, sortie tronquée, résultat via
//! `Incoming::HookDone`, jamais bloquant) ; `agent` = prompt mis en
//! file et préfixé au prochain envoi user (transitoire, annoncé).
//! Garde anti-récursion : un hook ne redéclenche jamais (profondeur
//! max 1, voir `firing`). Chargement frais à chaque tir (pas de
//! cache : l'user édite ses hooks sans redémarrer).

use std::collections::VecDeque;
use std::path::Path;

use super::hook_exec::HookOutcome;
use super::hook_match;
use super::hooks::{Hook, Trigger};
use crate::app::App;
use crate::chat::Role;

/// Déclenchements gardés pour `/hooks`.
const RECENT_MAX: usize = 10;

/// Un déclenchement passé (affiché par `/hooks`).
#[derive(Debug, Clone)]
pub struct HookFire {
    pub name: String,
    pub trigger: Trigger,
    pub status: String,
}

/// État hooks de `App` : file agent + historique + gardes.
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
    /// État vide (construit dans `ctors.rs`).
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

impl App {
    /// Tire les hooks du trigger (cible = texte matché : prompt, nom
    /// d'outil, chemin fichier, tâche…). Silencieux si aucun. Sites :
    /// `send.rs`, `flow.rs`, `spec_cmds.rs`, `drain.rs`, `ctors.rs`.
    pub(crate) fn hooks_fire(&mut self, trigger: Trigger, target: &str, chat: usize) {
        // Tests hermétiques : les chemins injectés (`*_in`) seuls tirent,
        // les tests existants n'exécutent jamais les vrais hooks de l'user.
        if cfg!(test) {
            return;
        }
        let project = crate::kiro::project_dir();
        let global = crate::kiro::global_dir();
        self.hooks_fire_in(trigger, target, chat, &project, global.as_deref());
    }

    /// Tir à chemins injectés (cœur testé ; prod passe par `hooks_fire`).
    pub(crate) fn hooks_fire_in(
        &mut self,
        trigger: Trigger,
        target: &str,
        chat: usize,
        project: &Path,
        global: Option<&Path>,
    ) {
        if self.hooks.firing || trigger == Trigger::Unknown {
            return;
        }
        let target = relative_target(project, trigger, target);
        if is_file_trigger(trigger) {
            let ignore = crate::kiro::ignore::load(project);
            if ignore.is_ignored(&target) {
                return;
            }
        }
        let selected = select_hooks(trigger, &target, project, global);
        if selected.is_empty() {
            return;
        }
        self.hooks.firing = true;
        for hook in &selected {
            self.hook_fire_one(hook, trigger, chat, project);
        }
        self.hooks.firing = false;
    }

    /// Exécute un hook : `command` = shell async, `agent` = file +
    /// annonce ; vide ou type opaque = noté `skipped` ou ignoré.
    fn hook_fire_one(&mut self, hook: &Hook, trigger: Trigger, chat: usize, project: &Path) {
        match hook.action.kind.as_str() {
            "command" => match hook.action.command.clone() {
                Some(cmd) if !cmd.trim().is_empty() => {
                    self.hook_spawn_command(chat, &hook.name, &cmd, trigger, project);
                }
                _ => self.hooks.record(HookFire {
                    name: hook.name.clone(),
                    trigger,
                    status: String::from("skipped: empty command"),
                }),
            },
            "agent" => match hook.action.prompt.clone() {
                Some(prompt) if !prompt.trim().is_empty() => {
                    self.hooks.pending.push(prompt);
                    self.hooks.record(HookFire {
                        name: hook.name.clone(),
                        trigger,
                        status: String::from("queued for next send"),
                    });
                    self.push_hook_line(
                        chat,
                        format!("hook {}: prompt queued for next send", hook.name),
                    );
                }
                _ => self.hooks.record(HookFire {
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
    fn hook_spawn_command(
        &mut self,
        chat: usize,
        name: &str,
        cmd: &str,
        trigger: Trigger,
        project: &Path,
    ) {
        if tokio::runtime::Handle::try_current().is_err() {
            self.hooks.record(HookFire {
                name: name.to_string(),
                trigger,
                status: String::from("skipped: no runtime"),
            });
            return;
        }
        super::hook_exec::spawn_command(&self.tx, chat, name, cmd, trigger, project);
    }

    /// Tombée hook : ligne système sur le chat d'origine + historique.
    pub(crate) fn drain_hook_done(&mut self, out: HookOutcome) {
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
        self.push_hook_line(out.chat, line);
        self.hooks.record(HookFire {
            name: out.name,
            trigger: out.trigger,
            status: status.to_string(),
        });
    }

    /// Préfixe l'input des prompts `agent` en file (file drainée).
    /// Vide = passthrough (zéro alloc superflue évitée : clone assumé).
    pub(crate) fn hook_prefixed_input(&mut self, text: &str) -> String {
        if self.hooks.pending.is_empty() {
            return text.to_string();
        }
        let queued = std::mem::take(&mut self.hooks.pending).join("\n\n");
        format!("{queued}\n\n{text}")
    }

    /// `SessionStart` au boot (une seule fois, chat courant, cible
    /// vide : seuls les hooks sans matcher (ou `.*`) partent).
    pub(crate) fn hooks_fire_session_start(&mut self) {
        if cfg!(test) {
            return;
        }
        let project = crate::kiro::project_dir();
        let global = crate::kiro::global_dir();
        self.hooks_fire_session_start_in(&project, global.as_deref());
    }

    /// `SessionStart` injecté (garde `une seule fois` testée ici).
    pub(crate) fn hooks_fire_session_start_in(&mut self, project: &Path, global: Option<&Path>) {
        if self.hooks.session_started {
            return;
        }
        self.hooks.session_started = true;
        let chat = self.sessions.index();
        self.hooks_fire_in(Trigger::SessionStart, "", chat, project, global);
    }

    /// `AgentStop` à la tombée (cible = prompt de l'agent, chat livré).
    pub(crate) fn hooks_fire_agent_stop(&mut self, owner: usize, agent_id: usize, deliver: usize) {
        let target = self.agent_prompt(owner, agent_id);
        self.hooks_fire(Trigger::AgentStop, &target, deliver);
    }

    /// Prompt de l'agent (cible `AgentStop`, vide si inconnu).
    fn agent_prompt(&self, owner: usize, agent_id: usize) -> String {
        self.sessions
            .get(owner)
            .and_then(|sess| sess.agents.iter().find(|agent| agent.id == agent_id))
            .map(|agent| agent.prompt.clone())
            .unwrap_or_default()
    }

    /// `AgentSpawn` à la création (cible = prompt, chat d'origine).
    pub(crate) fn hooks_fire_agent_spawn(&mut self, chat: usize, prompt: &str) {
        self.hooks_fire(Trigger::AgentSpawn, prompt, chat);
    }

    /// Ligne système sur le chat `chat` (pas forcément le courant).
    fn push_hook_line(&mut self, chat: usize, line: String) {
        if let Some(sess) = self.sessions.get_mut(chat) {
            sess.chat.push(Role::System, line);
        }
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
        std::env::temp_dir().join(format!("kh-hook-run-{}-{name}", std::process::id()))
    }

    /// Écrit un fichier hook JSON dans `dir/.kiro/hooks/`.
    fn seed(dir: &Path, file: &str, json: &str) {
        let hooks = dir.join(".kiro/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        std::fs::write(hooks.join(file), json).unwrap();
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
        let mut app = test_app();
        let kept = dir.join("src/main.rs").to_string_lossy().into_owned();
        app.hooks_fire_in(Trigger::PostFileSave, &kept, 0, &dir, None);
        assert_eq!(app.hooks.pending.len(), 1);
        let ignored = dir.join("target/out.rs").to_string_lossy().into_owned();
        app.hooks_fire_in(Trigger::PostFileSave, &ignored, 0, &dir, None);
        assert_eq!(app.hooks.pending.len(), 1);
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
        let mut app = test_app();
        app.hooks_fire_in(Trigger::PromptSubmit, "ajoute un test", 0, &dir, None);
        assert_eq!(app.hooks.pending, vec![String::from("relis le diff")]);
        let prefixed = app.hook_prefixed_input("ajoute un test");
        assert_eq!(prefixed, "relis le diff\n\najoute un test");
        assert!(app.hooks.pending.is_empty());
        assert_eq!(app.hook_prefixed_input("nu"), "nu");
        // Tombée commande : ligne système + historique.
        app.drain_hook_done(HookOutcome {
            chat: 0,
            name: String::from("fmt"),
            trigger: Trigger::PostFileSave,
            ok: true,
            text: String::from("all good"),
            ms: 12,
        });
        let text = app
            .sessions
            .get(0)
            .unwrap()
            .chat
            .messages()
            .last()
            .unwrap()
            .text
            .clone();
        assert!(text.contains("hook fmt: ok, 8 chars, 12 ms"), "{text}");
        assert!(text.contains("all good"), "{text}");
        assert_eq!(app.hooks.recent.len(), 2);
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
        let mut app = test_app();
        app.hooks.firing = true;
        app.hooks_fire_in(Trigger::PromptSubmit, "hello", 0, &dir, None);
        assert!(app.hooks.pending.is_empty());
        assert!(app.hooks.recent.is_empty());
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
        let mut app = test_app();
        app.hooks_fire_session_start_in(&dir, None);
        app.hooks_fire_session_start_in(&dir, None);
        assert_eq!(app.hooks.pending.len(), 1);
        assert!(app.hooks.session_started);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn agent_prompt_rend_le_prompt_ou_vide() {
        let mut app = test_app();
        let exchange = vec![(String::from("user"), String::from("fais ça"))];
        let id = app.sessions.add_agent(0, "fais ça", exchange).unwrap();
        assert_eq!(app.agent_prompt(0, id), "fais ça");
        assert_eq!(app.agent_prompt(0, 999), "");
        assert_eq!(app.agent_prompt(9, id), "");
    }
}
