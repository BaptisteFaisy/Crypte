//! Handlers `/spec` T2 (workflow specs Kiro) : `new` (génération 3
//! étapes chaînées), statut, `run` (tâches via sous-agents, une par
//! une, `y/n` en `auto`, direct en `yolo`).
//!
//! `impl App` déporté : lit/écrit l'état via les accès `pub(crate)` de [`App`].

use std::path::Path;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Incoming};
use crate::chat::Role;
use crate::connect::NOT_CONNECTED;
use crate::events::Event;
use crate::kiro::spec_run::{SpecGen, SpecRun};
use crate::permissions::Mode;
use crate::provider::Completion;

impl App {
    /// `/spec new <nom> <idée...>` : génère requirements + design +
    /// tasks (3 appels chaînés, voir `drain_spec_new`). Refuse si la
    /// spec existe déjà (aucun écrasement).
    pub(crate) fn cmd_spec_new(&mut self, name: String, idea: String) {
        if !crate::kiro::spec::valid_name(&name) {
            self.push_system(format!(
                "invalid spec name: {name} — use letters, digits, - and _"
            ));
            return;
        }
        let dir = crate::kiro::spec::spec_dir(&crate::kiro::project_dir(), &name);
        if ["requirements.md", "design.md", "tasks.md"]
            .iter()
            .any(|file| dir.join(file).is_file())
        {
            self.push_system(format!("spec {name} already exists — kept as is"));
            return;
        }
        if self.provider.is_none() {
            self.push_system(String::from(NOT_CONNECTED));
            return;
        }
        // Premier envoi réel : lecture unique d'agents.md (pin + annonce).
        self.ensure_agents_loaded();
        let chat = self.sessions.index();
        let gen = SpecGen::new(&name, &idea);
        let used = crate::tokens::context_estimate(&gen.messages());
        if used > self.context_limit() {
            self.push_system(format!(
                "spec context full ({}), generation not started",
                crate::tokens::fmt_context(used, self.context_limit())
            ));
            return;
        }
        self.push_system(format!("spec {name}: generating requirements… (0/3)"));
        self.spawn_spec_gen(chat, gen);
    }

    /// `/spec <nom>` : statut (docs présentes, tâches x/y, prochaine).
    pub(crate) fn cmd_spec_show(&mut self, name: String) {
        if !crate::kiro::spec::safe_name(&name) {
            self.push_system(format!("invalid spec name: {name}"));
            return;
        }
        let dir = crate::kiro::spec::spec_dir(&crate::kiro::project_dir(), &name);
        match crate::kiro::spec::load(&dir) {
            None => self.push_system(format!(
                "unknown spec: {name} — /spec new {name} <idea> to create it"
            )),
            Some(spec) => {
                self.push_system(format!("spec {}", crate::kiro::status::spec_text(&spec)));
            }
        }
    }

    /// `/spec <nom> run` : exécute les tâches non faites via des
    /// sous-agents (mécanisme `/agent`), une par une. Un seul run par
    /// spec, et une seule attente `y/n` à la fois.
    pub(crate) fn cmd_spec_run(&mut self, name: String) {
        if !crate::kiro::spec::safe_name(&name) {
            self.push_system(format!("invalid spec name: {name}"));
            return;
        }
        if self.provider.is_none() {
            self.push_system(String::from(NOT_CONNECTED));
            return;
        }
        if self.spec_runs.values().any(|run| run.name == name)
            || self
                .spec_waiting
                .as_ref()
                .is_some_and(|run| run.name == name)
        {
            self.push_system(format!("spec {name} is already running"));
            return;
        }
        if let Some(waiting) = &self.spec_waiting {
            self.push_system(format!(
                "spec {} waits for y/n — answer first",
                waiting.name
            ));
            return;
        }
        let chat = self.sessions.index();
        self.launch_spec_task(&name, chat);
    }

    /// Étape de génération reçue : comptée au ledger, stockée puis
    /// écrite (voir `spec_gen_store`) ; échec ou vide = stop (gardés).
    pub(crate) fn drain_spec_new(&mut self, chat: usize, gen: SpecGen, reply: Result<Completion>) {
        let done = match reply {
            Ok(done) if !done.text.trim().is_empty() => done,
            Ok(_) => {
                let what = format!("empty {}", gen.file_name());
                return self.spec_gen_failed(chat, &gen, &what);
            }
            Err(err) => {
                let what = format!("{} failed: {err:#}", gen.step_name());
                return self.spec_gen_failed(chat, &gen, &what);
            }
        };
        if let Some(sess) = self.sessions.get_mut(chat) {
            sess.ledger
                .record(done.prompt_tokens, done.completion_tokens, 0.0);
        }
        self.spec_gen_store(chat, gen, done);
    }

    /// Échec d'étape : note + fichiers déjà écrits gardés (en anglais).
    fn spec_gen_failed(&mut self, chat: usize, gen: &SpecGen, what: &str) {
        self.push_spec_line(
            chat,
            format!("spec {}: {what} — kept: {}", gen.name, gen.kept_text()),
        );
    }

    /// Étape reçue : écrit le fichier, affiche la progression (x/3),
    /// enchaîne l'étape suivante ou conclut (résumé final).
    fn spec_gen_store(&mut self, chat: usize, gen: SpecGen, done: Completion) {
        let dir = crate::kiro::spec::spec_dir(&crate::kiro::project_dir(), &gen.name);
        let file = gen.file_name();
        let mut gen = gen;
        gen.store(done.text.clone());
        if let Err(err) = write_doc(self.t5.checkpoint_dir.as_deref(), &dir, file, &done.text) {
            self.push_spec_line(
                chat,
                format!("spec {}: cannot write {file}: {err}", gen.name),
            );
            return;
        }
        let created = dir.join(file).to_string_lossy().into_owned();
        self.hooks_fire(crate::kiro::hooks::Trigger::PostFileCreate, &created, chat);
        if gen.advance() {
            self.push_spec_line(
                chat,
                format!(
                    "spec {}: {file} written ({}/3) — generating {}…",
                    gen.name,
                    gen.step,
                    gen.step_name()
                ),
            );
            self.spawn_spec_gen(chat, gen);
        } else {
            let total = crate::kiro::spec::parse_tasks(&done.text).len();
            self.push_spec_line(
                chat,
                format!(
                    "spec {} ready: requirements + design + {total} tasks (3/3)",
                    gen.name
                ),
            );
        }
    }

    /// Enchaîne l'appel LLM de l'étape courante (`Incoming::SpecNew`,
    /// état repassé par le canal : rien à garder dans `App`).
    fn spawn_spec_gen(&mut self, chat: usize, gen: SpecGen) {
        let Some(provider) = self.provider.clone() else {
            self.push_spec_line(
                chat,
                format!(
                    "spec {}: disconnected — kept: {}",
                    gen.name,
                    gen.kept_text()
                ),
            );
            return;
        };
        let messages = gen.messages();
        let model = self.effective_model_for(chat);
        let effort = self.thinking.clone();
        let tx = self.tx.clone();
        self.pool.spawn(async move {
            let reply = provider
                .complete_full(&messages, &model, effort.as_deref())
                .await;
            let _ = tx.send(Incoming::SpecNew(chat, gen, reply)).await;
        });
    }

    /// Lance la prochaine tâche non faite (relue sur disque à chaque
    /// appel : robuste aux éditions entre deux tâches).
    fn launch_spec_task(&mut self, name: &str, chat: usize) {
        // Premier envoi réel : lecture unique d'agents.md (pin + annonce).
        self.ensure_agents_loaded();
        let dir = crate::kiro::spec::spec_dir(&crate::kiro::project_dir(), name);
        let Some(spec) = crate::kiro::spec::load(&dir) else {
            self.push_spec_line(
                chat,
                format!("unknown spec: {name} — /spec new {name} <idea> to create it"),
            );
            return;
        };
        let total = spec.tasks.len();
        let Some(index) = spec.tasks.iter().position(|task| !task.done) else {
            if total == 0 {
                self.push_spec_line(chat, format!("spec {name}: no tasks yet"));
            } else {
                self.push_spec_line(chat, format!("spec {name} complete ({total}/{total})"));
            }
            return;
        };
        let prompt = crate::kiro::spec_prompts::task_user(
            spec.requirements.as_deref(),
            spec.design.as_deref(),
            &spec.tasks[index].text,
        );
        if let Some(line) = self.spec_prompt_over(name, &prompt) {
            self.push_spec_line(chat, line);
            return;
        }
        self.spawn_spec_agent(name, chat, &spec, index, prompt);
    }

    /// Ligne d'erreur si agents.md + prompt dépassent la fenêtre (`None` = OK).
    fn spec_prompt_over(&self, name: &str, prompt: &str) -> Option<String> {
        let mut messages = Vec::with_capacity(2);
        if let Some(content) = &self.agents_md {
            messages.push((String::from("system"), content.clone()));
        }
        messages.push((String::from("user"), prompt.to_string()));
        let used = crate::tokens::context_estimate(&messages);
        (used > self.context_limit()).then(|| {
            format!(
                "spec {name}: context full ({}), run stopped",
                crate::tokens::fmt_context(used, self.context_limit())
            )
        })
    }

    /// Crée le sous-agent de la tâche (mécanisme `/agent` : workspace,
    /// annonce, lancement, réponse livrée sur `chat`).
    fn spawn_spec_agent(
        &mut self,
        name: &str,
        chat: usize,
        spec: &crate::kiro::spec::Spec,
        index: usize,
        prompt: String,
    ) {
        let total = spec.tasks.len();
        let exchange = vec![(String::from("user"), prompt.clone())];
        let Some(agent_id) = self.sessions.add_agent(chat, &prompt, exchange) else {
            return;
        };
        self.sessions
            .attach_workspace(agent_id, crate::workspace::base().as_deref());
        self.events
            .emit(&Event::agent_spawned(agent_id, chat, chat, &prompt));
        let run = SpecRun::new(name, chat, index, total).with_agent(agent_id);
        if let Some((_, agent)) = self.sessions.find_agent_mut(agent_id) {
            agent.spec = Some(crate::sessions::SpecTag {
                name: name.to_string(),
                index,
                total,
            });
        }
        self.push_spec_line(
            chat,
            crate::agent_view::spec_parent_line(name, &run.label(), &spec.tasks[index].text),
        );
        self.push_spec_line(chat, crate::agent_view::spec_child_line(agent_id, name));
        self.hooks_fire_agent_spawn(chat, &prompt);
        self.hooks_fire(
            crate::kiro::hooks::Trigger::PreTaskExecution,
            &spec.tasks[index].text,
            chat,
        );
        self.spec_runs.insert(agent_id, run);
        let ask = vec![(String::from("user"), prompt)];
        self.launch_agent(chat, agent_id, chat, ask, None);
    }

    /// Tâche finie (réponse agent) : coche sur disque, affiche,
    /// enchaîne (`yolo`) ou demande (`auto`). Hors spec : rien.
    pub(crate) fn spec_agent_done(&mut self, agent_id: usize) {
        let Some(run) = self.spec_runs.remove(&agent_id) else {
            return;
        };
        let Some(tasks) = self.spec_check_off(&run) else {
            return;
        };
        let done_text = tasks
            .get(run.index)
            .map(|task| task.text.clone())
            .unwrap_or_default();
        self.hooks_fire(
            crate::kiro::hooks::Trigger::PostTaskExecution,
            &done_text,
            run.chat,
        );
        let (done, total) = crate::kiro::spec::progress(&tasks);
        self.push_spec_line(
            run.chat,
            format!(
                "spec {}: task {}/{} done ({done}/{total})",
                run.name,
                run.index + 1,
                total
            ),
        );
        let Some(next) = tasks.iter().position(|task| !task.done) else {
            self.push_spec_line(
                run.chat,
                format!("spec {} complete ({total}/{total})", run.name),
            );
            return;
        };
        if self.permissions == Mode::Yolo {
            self.launch_spec_task(&run.name, run.chat);
        } else if self.spec_waiting.is_some() {
            self.push_spec_line(
                run.chat,
                format!(
                    "spec {}: another confirmation is pending — run stopped, /spec {} run to resume",
                    run.name, run.name
                ),
            );
        } else {
            let text = tasks[next].text.clone();
            self.spec_ask(SpecRun::new(&run.name, run.chat, next, total), &text);
        }
    }

    /// Coche la tâche du run dans `tasks.md` (réécriture par index,
    /// reste préservé) ; rend les tâches fraîches (`None` = stoppé).
    fn spec_check_off(&mut self, run: &SpecRun) -> Option<Vec<crate::kiro::spec::Task>> {
        let dir = crate::kiro::spec::spec_dir(&crate::kiro::project_dir(), &run.name);
        let path = dir.join("tasks.md");
        let raw = std::fs::read_to_string(&path).unwrap_or_default();
        let Some(updated) = crate::kiro::spec::check_task(&raw, run.index) else {
            self.push_spec_line(
                run.chat,
                format!(
                    "spec {}: tasks.md changed underfoot — run stopped",
                    run.name
                ),
            );
            return None;
        };
        let tracked =
            crate::kiro::checkpoint::write_via(self.t5.checkpoint_dir.as_deref(), &path, &updated);
        if tracked.is_err() {
            self.push_spec_line(
                run.chat,
                format!("spec {}: cannot write tasks.md — run stopped", run.name),
            );
            return None;
        }
        let saved = path.to_string_lossy().into_owned();
        self.hooks_fire(crate::kiro::hooks::Trigger::PostFileSave, &saved, run.chat);
        Some(
            crate::kiro::spec::load(&dir)
                .map(|spec| spec.tasks)
                .unwrap_or_default(),
        )
    }

    /// Tâche échouée (agent en erreur) : run stoppé, tâche NON cochée.
    /// Hors spec : rien.
    pub(crate) fn spec_agent_failed(&mut self, agent_id: usize) {
        let Some(run) = self.spec_runs.remove(&agent_id) else {
            return;
        };
        let failed = format!("{} {}", run.name, run.label());
        self.hooks_fire(
            crate::kiro::hooks::Trigger::PostTaskExecution,
            &failed,
            run.chat,
        );
        self.push_spec_line(
            run.chat,
            format!(
                "spec {}: task {} failed — run stopped",
                run.name,
                run.label()
            ),
        );
    }

    /// Touches pendant une confirmation spec : y = lance, n/Échap =
    /// stoppe (le reste est ignoré, jamais de quit — comme les outils).
    pub(crate) fn on_spec_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('y' | 'Y') => self.spec_resolve(true),
            KeyCode::Char('n' | 'N') | KeyCode::Esc => self.spec_resolve(false),
            _ => true,
        }
    }

    /// Résolution y/n : lance la tâche suivante ou stoppe le run (x/y).
    fn spec_resolve(&mut self, allow: bool) -> bool {
        let Some(run) = self.spec_waiting.take() else {
            return true;
        };
        if let Some(sess) = self.sessions.get_mut(run.chat) {
            sess.chat.set_prompt(None, false);
            sess.chat.set_input_preview(None);
        }
        if allow {
            self.launch_spec_task(&run.name, run.chat);
        } else {
            let dir = crate::kiro::spec::spec_dir(&crate::kiro::project_dir(), &run.name);
            let (done, total) = crate::kiro::spec::load(&dir)
                .map(|spec| crate::kiro::spec::progress(&spec.tasks))
                .unwrap_or((0, 0));
            self.push_spec_line(
                run.chat,
                format!("spec {} run stopped ({done}/{total} done)", run.name),
            );
        }
        true
    }

    /// Demande confirmation pour la tâche suivante (mode `auto`) :
    /// saute au chat, affiche la tâche, prompt y/n (touches capturées).
    fn spec_ask(&mut self, run: SpecRun, task: &str) {
        let chat = run.chat;
        let line = format!(
            "spec {}: run task {} — {}? (y/n)",
            run.name,
            run.label(),
            crate::provider::truncate(task, 60)
        );
        let preview = format!(
            "task {}: {}",
            run.label(),
            crate::provider::truncate(task, 60)
        );
        self.sessions.select(chat);
        self.spec_waiting = Some(run);
        if let Some(sess) = self.sessions.get_mut(chat) {
            sess.chat.push(Role::System, line);
            sess.chat
                .set_prompt(Some(String::from("run it? (y/n)")), false);
            sess.chat.set_input_preview(Some(preview));
        }
    }

    /// Ligne système sur le chat `chat` (pas forcément le courant ;
    /// jumeau de `push_tool_line`, `tools/` étant une zone T3).
    fn push_spec_line(&mut self, chat: usize, line: String) {
        if let Some(sess) = self.sessions.get_mut(chat) {
            sess.chat.push(Role::System, line);
        }
    }
}

/// Écrit un doc de spec (dossier créé, `\n` final garanti, snapshot
/// si `ckpt`).
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
