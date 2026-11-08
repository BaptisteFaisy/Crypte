//! Orchestration des specs Kiro (`/spec new|run`), version Tauri.
//!
//! Reprend de `src/kiro/spec_cmds.rs` (TUI) la génération chaînée en 3
//! étapes (requirements → design → tasks, écrites + snapshotées) et
//! l'exécution tâche par tâche via sous-agents (`yolo` enchaîne,
//! `auto` demande y/n). Adaptation : `impl App` → [`SpecRunner`]
//! (état isolé, sorties [`SpecOut`] pour l'intégrateur qui exécute les
//! appels LLM, spawn les agents réels et émet `spec_progress`).
//!
//! Textes anglais repris à l'identique, y compris les lignes
//! `agent_view` (`◈ spec …`, `✦ agent#N …`) et `NOT_CONNECTED`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::kiro::hook_run::{HookOut, HooksEngine};
use crate::kiro::hooks::Trigger;
use crate::kiro::spec_run::{SpecGen, SpecRun};
use crate::permissions::Mode;
use crate::provider::truncate;

/// Message déconnecté (identique à `connect.rs` du TUI).
pub const NOT_CONNECTED: &str = "not connected: /connect <provider> or /settings";

/// Sortie du runner pour l'intégrateur.
#[derive(Debug)]
pub enum SpecOut {
    /// Ligne système sur le chat.
    ChatLine { chat: usize, text: String },
    /// Tokens d'une étape de génération (ledger du chat).
    LedgerAdd {
        chat: usize,
        prompt: u64,
        completion: u64,
    },
    /// Compléter `gen.messages()` (modèle/effort de l'intégrateur),
    /// puis `gen_reply`.
    SpawnGen { chat: usize, gen: SpecGen },
    /// Spawner un agent réel sur `prompt`, puis `agent_started`.
    /// (`name`/`index`/`total`/`task` : contexte d'enregistrement —
    /// l'intégrateur les repasse à `agent_started`, cf. `bridge`.)
    SpawnAgent {
        chat: usize,
        name: String,
        index: usize,
        total: usize,
        label: String,
        prompt: String,
        task: String,
    },
    /// Confirmation y/n (modale/CLI frontend), puis `respond`.
    AskConfirm {
        chat: usize,
        name: String,
        label: String,
        task: String,
    },
    /// Progression (event `spec_progress`).
    Progress {
        name: String,
        label: String,
        done: usize,
        total: usize,
    },
}

mod gen;

/// Runner specs : runs + attente y/n + chemins (LLM/agents dehors).
pub struct SpecRunner {
    project: PathBuf,
    checkpoint_dir: Option<PathBuf>,
    mode: Mode,
    connected: bool,
    runs: HashMap<usize, SpecRun>,
    waiting: Option<SpecRun>,
}

impl SpecRunner {
    /// Runner sur chemins injectés (tests hermétiques).
    pub fn new(project: PathBuf, checkpoint_dir: Option<PathBuf>) -> Self {
        Self {
            project,
            checkpoint_dir,
            mode: Mode::Auto,
            connected: true,
            runs: HashMap::new(),
            waiting: None,
        }
    }

    /// Runner sur les chemins Kiro réels (prod).
    pub fn live() -> Self {
        Self::new(crate::kiro::project_dir(), crate::kiro::checkpoint::dir())
    }

    /// Suit le mode de permissions (yolo enchaîne, auto demande).
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    /// Suit la connexion (l'intégrateur met à jour à chaque changement).
    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
    }

    /// Attente y/n en cours, le cas échéant.
    pub fn waiting(&self) -> Option<&SpecRun> {
        self.waiting.as_ref()
    }

    /// `/spec <nom> run` : exécute les tâches non faites via des
    /// sous-agents, une par une. Un seul run par spec, et une seule
    /// attente `y/n` à la fois.
    pub fn run_start(&mut self, name: &str, chat: usize) -> Vec<SpecOut> {
        if !crate::kiro::spec::safe_name(name) {
            return vec![SpecOut::ChatLine {
                chat,
                text: format!("invalid spec name: {name}"),
            }];
        }
        if !self.connected {
            return vec![SpecOut::ChatLine {
                chat,
                text: NOT_CONNECTED.to_string(),
            }];
        }
        if self.runs.values().any(|run| run.name == name)
            || self.waiting.as_ref().is_some_and(|run| run.name == name)
        {
            return vec![SpecOut::ChatLine {
                chat,
                text: format!("spec {name} is already running"),
            }];
        }
        if let Some(waiting) = &self.waiting {
            return vec![SpecOut::ChatLine {
                chat,
                text: format!("spec {} waits for y/n — answer first", waiting.name),
            }];
        }
        self.launch_task(name, chat)
    }

    /// Lance la prochaine tâche non faite (relue sur disque à chaque
    /// appel : robuste aux éditions entre deux tâches).
    fn launch_task(&self, name: &str, chat: usize) -> Vec<SpecOut> {
        let dir = crate::kiro::spec::spec_dir(&self.project, name);
        let Some(spec) = crate::kiro::spec::load(&dir) else {
            return vec![SpecOut::ChatLine {
                chat,
                text: format!("unknown spec: {name} — /spec new {name} <idea> to create it"),
            }];
        };
        let total = spec.tasks.len();
        let Some(index) = spec.tasks.iter().position(|task| !task.done) else {
            let text = if total == 0 {
                format!("spec {name}: no tasks yet")
            } else {
                format!("spec {name} complete ({total}/{total})")
            };
            return vec![SpecOut::ChatLine { chat, text }];
        };
        let task = spec.tasks[index].text.clone();
        let prompt = crate::kiro::spec_prompts::task_user(
            spec.requirements.as_deref(),
            spec.design.as_deref(),
            &task,
        );
        vec![SpecOut::SpawnAgent {
            chat,
            name: name.to_string(),
            index,
            total,
            label: format!("{}/{}", index + 1, total),
            prompt,
            task,
        }]
    }

    /// Agent de tâche spawné par l'intégrateur : enregistre le run,
    /// annonce (parent + enfant), hooks, progression.
    #[allow(clippy::too_many_arguments)] // Miroir de l'appel TUI (id + index + tâche explicites).
    pub fn agent_started(
        &mut self,
        name: &str,
        chat: usize,
        index: usize,
        total: usize,
        agent_id: usize,
        task: &str,
        hooks: &mut HooksEngine,
    ) -> Vec<SpecOut> {
        let mut outs = Vec::new();
        let run = SpecRun::new(name, chat, index, total).with_agent(agent_id);
        outs.push(SpecOut::ChatLine {
            chat,
            text: spec_parent_line(name, &run.label(), task),
        });
        outs.push(SpecOut::ChatLine {
            chat,
            text: spec_child_line(agent_id, name),
        });
        let prompt = format!("spec {name} task {}", run.label());
        push_hook_lines(hooks.fire_agent_spawn(chat, &prompt), &mut outs);
        push_hook_lines(hooks.fire(Trigger::PreTaskExecution, task, chat), &mut outs);
        self.runs.insert(agent_id, run.clone());
        outs.push(SpecOut::Progress {
            name: name.to_string(),
            label: run.label(),
            done: index,
            total,
        });
        outs
    }

    /// Tâche finie (réponse agent) : coche sur disque, affiche,
    /// enchaîne (`yolo`) ou demande (`auto`). Hors spec : rien.
    pub fn agent_done(&mut self, agent_id: usize, hooks: &mut HooksEngine) -> Vec<SpecOut> {
        let mut outs = Vec::new();
        let Some(run) = self.runs.remove(&agent_id) else {
            return outs;
        };
        let Some(tasks) = self.check_off(&run, hooks, &mut outs) else {
            return outs;
        };
        let done_text = tasks
            .get(run.index)
            .map(|task| task.text.clone())
            .unwrap_or_default();
        push_hook_lines(
            hooks.fire(Trigger::PostTaskExecution, &done_text, run.chat),
            &mut outs,
        );
        let (done, total) = crate::kiro::spec::progress(&tasks);
        outs.push(SpecOut::ChatLine {
            chat: run.chat,
            text: format!(
                "spec {}: task {}/{} done ({done}/{total})",
                run.name,
                run.index + 1,
                total
            ),
        });
        outs.push(SpecOut::Progress {
            name: run.name.clone(),
            label: run.label(),
            done,
            total,
        });
        self.schedule_next(&run, &tasks, &mut outs);
        outs
    }

    /// Après une tâche : termine, enchaîne (`yolo`) ou demande (`auto`).
    fn schedule_next(
        &mut self,
        run: &SpecRun,
        tasks: &[crate::kiro::spec::Task],
        outs: &mut Vec<SpecOut>,
    ) {
        let total = tasks.len();
        let Some(next) = tasks.iter().position(|task| !task.done) else {
            outs.push(SpecOut::ChatLine {
                chat: run.chat,
                text: format!("spec {} complete ({total}/{total})", run.name),
            });
            return;
        };
        if self.mode == Mode::Yolo {
            outs.extend(self.launch_task(&run.name, run.chat));
        } else if self.waiting.is_some() {
            outs.push(SpecOut::ChatLine {
                chat: run.chat,
                text: format!(
                    "spec {}: another confirmation is pending — run stopped, /spec {} run to resume",
                    run.name, run.name
                ),
            });
        } else {
            self.ask_next(run, tasks, next, total, outs);
        }
    }

    /// Demande y/n pour la prochaine tâche (attente `spec_waiting`).
    fn ask_next(
        &mut self,
        run: &SpecRun,
        tasks: &[crate::kiro::spec::Task],
        next: usize,
        total: usize,
        outs: &mut Vec<SpecOut>,
    ) {
        let text = tasks[next].text.clone();
        let ask = SpecRun::new(&run.name, run.chat, next, total);
        outs.push(SpecOut::ChatLine {
            chat: run.chat,
            text: format!(
                "spec {}: run task {} — {}? (y/n)",
                run.name,
                ask.label(),
                truncate(&text, 60)
            ),
        });
        outs.push(SpecOut::AskConfirm {
            chat: run.chat,
            name: run.name.clone(),
            label: ask.label(),
            task: text,
        });
        self.waiting = Some(ask);
    }

    /// Coche la tâche du run dans `tasks.md` (réécriture par index,
    /// reste préservé) ; rend les tâches fraîches (`None` = stoppé).
    fn check_off(
        &self,
        run: &SpecRun,
        hooks: &mut HooksEngine,
        outs: &mut Vec<SpecOut>,
    ) -> Option<Vec<crate::kiro::spec::Task>> {
        let dir = crate::kiro::spec::spec_dir(&self.project, &run.name);
        let path = dir.join("tasks.md");
        let raw = std::fs::read_to_string(&path).unwrap_or_default();
        let Some(updated) = crate::kiro::spec::check_task(&raw, run.index) else {
            outs.push(SpecOut::ChatLine {
                chat: run.chat,
                text: format!(
                    "spec {}: tasks.md changed underfoot — run stopped",
                    run.name
                ),
            });
            return None;
        };
        let tracked =
            crate::kiro::checkpoint::write_via(self.checkpoint_dir.as_deref(), &path, &updated);
        if tracked.is_err() {
            outs.push(SpecOut::ChatLine {
                chat: run.chat,
                text: format!("spec {}: cannot write tasks.md — run stopped", run.name),
            });
            return None;
        }
        let saved = path.to_string_lossy().into_owned();
        push_hook_lines(hooks.fire(Trigger::PostFileSave, &saved, run.chat), outs);
        Some(
            crate::kiro::spec::load(&dir)
                .map(|spec| spec.tasks)
                .unwrap_or_default(),
        )
    }

    /// Tâche échouée (agent en erreur) : run stoppé, tâche NON cochée.
    /// Hors spec : rien.
    pub fn agent_failed(&mut self, agent_id: usize, hooks: &mut HooksEngine) -> Vec<SpecOut> {
        let mut outs = Vec::new();
        let Some(run) = self.runs.remove(&agent_id) else {
            return outs;
        };
        let failed = format!("{} {}", run.name, run.label());
        push_hook_lines(
            hooks.fire(Trigger::PostTaskExecution, &failed, run.chat),
            &mut outs,
        );
        outs.push(SpecOut::ChatLine {
            chat: run.chat,
            text: format!(
                "spec {}: task {} failed — run stopped",
                run.name,
                run.label()
            ),
        });
        outs
    }

    /// Réponse à l'attente y/n (`spec_respond`) : `y…` lance, `n…`
    /// stoppe, le reste garde l'attente (le TUI ignorait la touche ;
    /// ici une aide d'une ligne, affordance Tauri sans modale).
    pub fn respond(&mut self, text: &str) -> Vec<SpecOut> {
        let Some(run) = self.waiting.take() else {
            return Vec::new();
        };
        let answer = text.trim().to_lowercase();
        if answer.starts_with('y') {
            return self.launch_task(&run.name, run.chat);
        }
        if !answer.starts_with('n') {
            self.waiting = Some(run);
            return Vec::new();
        }
        let dir = crate::kiro::spec::spec_dir(&self.project, &run.name);
        let (done, total) = crate::kiro::spec::load(&dir)
            .map(|spec| crate::kiro::spec::progress(&spec.tasks))
            .unwrap_or((0, 0));
        vec![SpecOut::ChatLine {
            chat: run.chat,
            text: format!("spec {} run stopped ({done}/{total} done)", run.name),
        }]
    }
}

/// Lignes hooks → sorties runner (même texte, même chat).
fn push_hook_lines(fired: Vec<HookOut>, outs: &mut Vec<SpecOut>) {
    for out in fired {
        let HookOut::ChatLine { chat, text } = out;
        outs.push(SpecOut::ChatLine { chat, text });
    }
}

/// Parent `/spec run` : `◈ spec <nom> — task <i/n> : <tâche>` (TUI).
fn spec_parent_line(name: &str, label: &str, task: &str) -> String {
    format!("◈ spec {name} — task {label}: {}", truncate(task, 40))
}

/// Enfant `/spec run` : `  ✦ agent#N — spec <nom>` (TUI).
fn spec_child_line(id: usize, name: &str) -> String {
    format!("  ✦ agent#{id} — spec {name}")
}

/// Écrit un doc de spec (dossier créé, `\n` final garanti, snapshot
/// si `ckpt`). Identique au TUI.
fn write_doc(ckpt: Option<&Path>, dir: &Path, file: &str, text: &str) -> std::io::Result<()> {
    let owned: String;
    let final_text = if text.ends_with('\n') {
        text
    } else {
        owned = format!("{text}\n");
        &owned
    };
    crate::kiro::checkpoint::write_via(ckpt, &dir.join(file), final_text)
}

#[cfg(test)]
mod tests;
