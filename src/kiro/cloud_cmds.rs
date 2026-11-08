//! Handlers `/cloud` (sessions cloud Kiro via `kiro-cli` headless).
//!
//! `/cloud run` lance un agent dans un sandbox cloud (réponse via
//! `Incoming::Cloud`, streamée `[cloud#n]`) ; `list`/`delete` sont des
//! requêtes one-shot (job 0) ; `attach` imprime la reprise manuelle
//! (la session vit dans le cloud, hors Crypte) ; `stop` tue le
//! processus local (la session cloud, elle, continue).

use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

use crate::app::{App, Incoming};
use crate::chat::Role;
use crate::kiro::cloud::{self, CloudLine, CloudMsg, CloudOutcome};

/// Aide `/cloud` (aussi sur sous-commande inconnue).
const CLOUD_USAGE: &str = "usage: /cloud [status] | run [--repo o/r] <task> | list | attach <id> | delete <id> | stop [#n] — kiro cloud sessions (kiro-cli 2.17+)";
/// Timeout requêtes one-shot (`list`, `delete`).
const ONE_SHOT_SECS: u64 = 60;
/// Job des requêtes one-shot (jamais dans le registre).
const QUERY_JOB: usize = 0;

impl App {
    /// `/cloud ...` : `status` (défaut), `run`, `list`, `attach`,
    /// `delete`, `stop`, `help`.
    pub(crate) fn cmd_cloud(&mut self, args: Vec<String>) {
        let mut words = args.iter().map(String::as_str);
        match words.next() {
            None | Some("status") => self.cloud_status(),
            Some("run") => self.cloud_run(words.collect()),
            Some("list") => self.cloud_query(cloud::list_argv(), "sessions"),
            Some("attach") => match words.next() {
                Some(id) => self.push_system(cloud::attach_text(id)),
                None => self.push_system(String::from(
                    "usage: /cloud attach <id> — resume outside Crypte",
                )),
            },
            Some("delete") => match words.next() {
                Some(id) => self.cloud_query(cloud::delete_argv(id), "delete"),
                None => self.push_system(String::from("usage: /cloud delete <id>")),
            },
            Some("stop") => self.cloud_stop(words.next()),
            Some("help") => self.push_system(String::from(CLOUD_USAGE)),
            Some(other) => self.push_system(format!("unknown /cloud {other} — {CLOUD_USAGE}")),
        }
    }

    /// `/cloud status` : CLI, dépôt résolu, git, runs en vol (synchrone).
    fn cloud_status(&mut self) {
        let cli = match self.cloud.probe() {
            Ok(cli) => cli,
            Err(hint) => {
                self.push_system(format!("cloud: {hint}"));
                return;
            }
        };
        let project = crate::kiro::project_dir();
        let repo = cloud::resolve_repo(None, cloud::git_remote(&project).as_deref());
        let git = cloud::git_state(&project).map(|state| {
            let dirty = if state.dirty { "dirty" } else { "clean" };
            match state.unpushed {
                Some(0) => format!("{dirty}, pushed"),
                Some(n) => format!("{dirty}, {n} ahead (invisible in cloud)"),
                None => format!("{dirty}, no upstream"),
            }
        });
        let runs = if self.cloud.running.is_empty() {
            String::from("none")
        } else {
            let mut ids: Vec<usize> = self.cloud.running.keys().copied().collect();
            ids.sort_unstable();
            ids.iter()
                .map(|id| {
                    let repo = self
                        .cloud
                        .running
                        .get(id)
                        .map(|job| job.repo.as_str())
                        .unwrap_or("?");
                    format!("#{id} ({repo})")
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let (major, minor, patch) = cli.version;
        self.push_system(format!(
            "cloud: kiro-cli {major}.{minor}.{patch} ({})\n  repo: {}\n  git: {}\n  runs: {runs}\n  web: https://app.kiro.dev",
            cli.path.display(),
            repo.unwrap_or_else(|hint| hint.to_string()),
            git.as_deref().unwrap_or("unreadable (git missing?)")
        ));
    }

    /// `/cloud run [--repo o/r] <tâche...>` : pré-contrôles sync, spawn,
    /// stream async (`[cloud#n]`). Le sandbox clone le dépôt distant :
    /// le non-poussé est averti, pas bloqué.
    fn cloud_run(&mut self, words: Vec<&str>) {
        let cli = match self.cloud.probe() {
            Ok(cli) => cli,
            Err(hint) => {
                self.push_system(format!("cloud: {hint}"));
                return;
            }
        };
        let (explicit, task) = split_run_args(&words);
        if task.is_empty() {
            self.push_system(String::from("usage: /cloud run [--repo o/r] <task>"));
            return;
        }
        let project = crate::kiro::project_dir();
        let repo = match cloud::resolve_repo(
            explicit.as_deref(),
            cloud::git_remote(&project).as_deref(),
        ) {
            Ok(repo) => repo,
            Err(hint) => {
                self.push_system(format!("cloud: {hint}"));
                return;
            }
        };
        self.cloud_warn_git(&project);
        let child = match tokio::process::Command::new(&cli.path)
            .args(cloud::run_argv(&repo, &task))
            .current_dir(&project)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => child,
            Err(err) => {
                self.push_system(format!("cloud: spawn failed: {err}"));
                return;
            }
        };
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let id = self.cloud.register(stop_tx, repo.clone());
        let chat = self.sessions.index();
        let tx = self.tx.clone();
        self.pool.spawn(pump_run(tx, chat, id, child, stop_rx));
        self.push_system(format!("cloud#{id} on {repo} …"));
    }

    /// Avertit si le cloud ne verra pas le travail local (sale/non poussé).
    fn cloud_warn_git(&mut self, project: &std::path::Path) {
        match cloud::git_state(project) {
            None => self.push_system(String::from(
                "cloud: git unreadable here — the sandbox clones the remote as-is",
            )),
            Some(state) if state.dirty => self.push_system(String::from(
                "cloud: dirty tree — uncommitted work is invisible in the cloud",
            )),
            Some(state) if state.unpushed.is_some_and(|n| n > 0) => {
                self.push_system(String::from(
                    "cloud: unpushed commits — the sandbox clones the remote as-is",
                ));
            }
            Some(_) => {}
        }
    }

    /// `/cloud list|delete` : requête one-shot async (job 0, 60 s max).
    fn cloud_query(&mut self, argv: Vec<String>, what: &'static str) {
        let cli = match self.cloud.probe() {
            Ok(cli) => cli,
            Err(hint) => {
                self.push_system(format!("cloud: {hint}"));
                return;
            }
        };
        let chat = self.sessions.index();
        let tx = self.tx.clone();
        let project = crate::kiro::project_dir();
        self.pool.spawn(async move {
            let (ok, text) = run_once(&cli.path, &argv, &project).await;
            let outcome = CloudOutcome {
                session: None,
                ok,
                detail: format!("{what}:\n{text}"),
            };
            let _ = tx
                .send(Incoming::Cloud(chat, QUERY_JOB, CloudMsg::Done(outcome)))
                .await;
        });
        self.push_system(format!("cloud {what} …"));
    }

    /// `/cloud stop [#n]` : tue le processus local (la session cloud
    /// continue). Sans arg : le seul run en vol, s'il est unique.
    fn cloud_stop(&mut self, target: Option<&str>) {
        let id = match target.map(|t| t.trim_start_matches('#').parse::<usize>()) {
            None if self.cloud.running.len() == 1 => {
                *self.cloud.running.keys().next().unwrap_or(&0)
            }
            None => {
                self.push_system(String::from("usage: /cloud stop #<n>"));
                return;
            }
            Some(Ok(id)) => id,
            Some(Err(_)) => {
                self.push_system(String::from("usage: /cloud stop #<n>"));
                return;
            }
        };
        match self.cloud.take(id) {
            Some(job) => {
                let _ = job.stop.send(());
                let session = job
                    .session
                    .map(|s| format!(" (cloud session {s} continues)"))
                    .unwrap_or_else(|| String::from(" (cloud session continues)"));
                self.push_system(format!("cloud#{id} stopped{session}"));
            }
            None => self.push_system(format!("cloud: no run #{id}")),
        }
    }

    /// `Incoming::Cloud` : ligne streamée (`[cloud#n]`, chat seul, jamais
    /// l'historique) ou fin (résumé session + reprise + web).
    pub(crate) fn drain_cloud(&mut self, chat: usize, job: usize, msg: CloudMsg) {
        match msg {
            CloudMsg::Line(line) => self.drain_cloud_line(chat, job, line),
            CloudMsg::Done(outcome) => self.drain_cloud_done(chat, job, outcome),
        }
    }

    /// Ligne streamée : taggée, chat seul (job inconnu = stoppé, ignoré).
    fn drain_cloud_line(&mut self, chat: usize, job: usize, line: CloudLine) {
        if job != QUERY_JOB && !self.cloud.running.contains_key(&job) {
            return;
        }
        if let Some(session) = &line.session {
            self.cloud.note_session(job, session.clone());
        }
        if let Some(sess) = self.sessions.get_mut(chat) {
            sess.chat
                .push(Role::Assistant, format!("[cloud#{job}] {}", line.text));
        }
    }

    /// Fin : registre nettoyé (runs), résumé affichable (job 0 = brut).
    fn drain_cloud_done(&mut self, chat: usize, job: usize, outcome: CloudOutcome) {
        let known = self.cloud.take(job).is_some() || job == QUERY_JOB;
        let session = outcome.session.clone().filter(|s| !s.is_empty());
        let text = if job == QUERY_JOB {
            outcome.detail.clone()
        } else if !known {
            match session {
                Some(id) => cloud::attach_text(&id),
                None => return,
            }
        } else if !outcome.ok {
            match session {
                Some(id) => format!("[cloud#{job}] failed: {}\n  session: {id}\n  resume: kiro-cli --resume-id {id}", outcome.detail),
                None => format!("[cloud#{job}] failed: {}", outcome.detail),
            }
        } else {
            match session {
                Some(id) => format!("[cloud#{job}] done — session {id}\n  resume: kiro-cli --resume-id {id}\n  web: https://app.kiro.dev"),
                None => format!("[cloud#{job}] done (no session id captured)"),
            }
        };
        if let Some(sess) = self.sessions.get_mut(chat) {
            sess.chat.push(Role::System, text);
        }
    }
}

/// Découpe `/cloud run` : `--repo R` optionnel + tâche (reste joint).
fn split_run_args(words: &[&str]) -> (Option<String>, String) {
    let mut repo = None;
    let mut task = Vec::new();
    let mut rest = words.iter().peekable();
    while let Some(word) = rest.next() {
        if *word == "--repo" {
            if let Some(value) = rest.next() {
                repo.get_or_insert_with(|| (*value).to_string());
            }
        } else {
            task.push((*word).to_string());
        }
    }
    (repo, task.join(" "))
}

/// Exécute le CLI et attend la sortie (60 s max) : `(ok, texte)`.
async fn run_once(cli: &std::path::Path, argv: &[String], cwd: &std::path::Path) -> (bool, String) {
    let child = match tokio::process::Command::new(cli)
        .args(argv)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(err) => return (false, format!("spawn failed: {err}")),
    };
    let out =
        match tokio::time::timeout(Duration::from_secs(ONE_SHOT_SECS), child.wait_with_output())
            .await
        {
            Err(_) => return (false, format!("timed out after {ONE_SHOT_SECS}s")),
            Ok(Err(err)) => return (false, format!("wait failed: {err}")),
            Ok(Ok(out)) => out,
        };
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&err);
    }
    let text = text.trim().to_string();
    let text = crate::provider::truncate(&text, 2000);
    (out.status.success(), text)
}

/// Pompe un run cloud : lignes streamées (`Line`), puis `Done` (session
/// captée). `stop` reçu ou canal mort → enfant tué, `Done` quand même.
async fn pump_run(
    tx: tokio::sync::mpsc::Sender<Incoming>,
    chat: usize,
    id: usize,
    mut child: tokio::process::Child,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) {
    let err_task = drain_stderr(child.stderr.take());
    let mut lines = child.stdout.take().map(|out| BufReader::new(out).lines());
    let mut session = None;
    let drained = loop {
        tokio::select! {
            biased;
            _ = &mut stop => break false,
            next = async {
                match lines.as_mut() {
                    Some(l) => l.next_line().await.ok().flatten(),
                    None => None,
                }
            } => {
                match next {
                    Some(raw) => {
                        if let Some(ev) = cloud::parse_line(&raw) {
                            if session.is_none() {
                                session = ev.session.clone();
                            }
                            if tx.send(Incoming::Cloud(chat, id, CloudMsg::Line(ev))).await.is_err() {
                                break false;
                            }
                        }
                    }
                    None => break true,
                }
            }
        }
    };
    if !drained {
        let _ = child.start_kill();
    }
    let code = child.wait().await.ok().and_then(|s| s.code());
    let err_buf = err_task.await.unwrap_or_default();
    let ok = drained && code == Some(0);
    let mut detail = String::from_utf8_lossy(&err_buf).trim().to_string();
    if detail.is_empty() && !ok {
        detail = code
            .map(|c| format!("exit {c}"))
            .unwrap_or_else(|| String::from("killed"));
    }
    let detail = crate::provider::truncate(&detail, 500);
    let outcome = CloudOutcome {
        session,
        ok,
        detail,
    };
    let _ = tx
        .send(Incoming::Cloud(chat, id, CloudMsg::Done(outcome)))
        .await;
}

/// Vidange stderr en tâche de fond (tuyau plein = enfant bloqué).
fn drain_stderr(stderr: Option<tokio::process::ChildStderr>) -> tokio::task::JoinHandle<Vec<u8>> {
    tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(mut err) = stderr {
            let _ = err.read_to_end(&mut buf).await;
        }
        buf
    })
}

#[cfg(test)]
mod tests;
