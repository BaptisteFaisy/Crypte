//! Sessions cloud Kiro (`/cloud`) : lance `kiro-cli` en headless.
//!
//! Un run = `kiro-cli chat --v3 --no-interactive --cloud --repo
//! owner/repo --trust-all-tools --output-format stream-json <tâche>`
//! (voir kiro.dev/docs/cli/headless). Le sandbox clone le dépôt côté
//! serveur : le travail non poussé est invisible du cloud (averti, pas
//! bloqué). Sessions listées/reprises via `--list-sessions` /
//! `--resume-id` (voir kiro.dev/docs/cloud-sessions).
//!
//! Tests hermétiques : `KH_KIRO_CLI` remplace le binaire (stub shell),
//! `KH_KIRO_PATH` remplace le `PATH` balayé (le global reste intact).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Version CLI minimale pour le cloud (docs : 2.17+).
const MIN_CLI_MAJOR: u64 = 2;
/// Mineur minimal (majeur 2).
const MIN_CLI_MINOR: u64 = 17;

/// CLI détectée (mise en cache après le premier `probe` réussi).
#[derive(Debug, Clone)]
pub struct CliStatus {
    /// Chemin du binaire (`KH_KIRO_CLI` ou trouvé dans `PATH`).
    pub path: PathBuf,
    /// Version parsée (`--version`).
    pub version: (u64, u64, u64),
}

/// État git du dossier projet (avertissements `/cloud run`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitState {
    /// Faux si git absent ou dossier hors dépôt.
    pub readable: bool,
    /// `git status --porcelain` non vide.
    pub dirty: bool,
    /// Commits non poussés (`None` : pas de upstream).
    pub unpushed: Option<usize>,
}

/// Une ligne d'évènement (`stream-json` ou texte brut).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudLine {
    /// Texte affiché (`[cloud#n]` préfixé au drain).
    pub text: String,
    /// Id de session si la ligne le révèle.
    pub session: Option<String>,
}

/// Fin d'un run cloud (ou d'une requête `list`/`delete`, job 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudOutcome {
    /// Id de session capté pendant le run (`None` si inconnu).
    pub session: Option<String>,
    /// Sortie 0 du CLI.
    pub ok: bool,
    /// Détail (erreur, ou sortie brute pour `list`/`delete`).
    pub detail: String,
}

/// Message async d'un run cloud : ligne streamée ou fin.
#[derive(Debug, Clone)]
pub enum CloudMsg {
    /// Ligne à afficher aussitôt.
    Line(CloudLine),
    /// Fin : registre nettoyé + résumé (session, reprise, web).
    Done(CloudOutcome),
}

/// Run cloud en vol (arrêtable par `/cloud stop` : la pompe tue
/// l'enfant à la réception, `kill_on_drop` en backstop).
pub(crate) struct Job {
    /// Signaler `()` arrête la pompe (enfant tué par elle).
    pub stop: tokio::sync::oneshot::Sender<()>,
    /// Dépôt `--repo` du run.
    pub repo: String,
    /// Id de session capté (annoncé dans le résumé final).
    pub session: Option<String>,
}

/// Registre des runs en vol + cache CLI (un champ `App`).
pub(crate) struct CloudJobs {
    /// Jobs indexés (`take` à la fin, `stop` pour tuer).
    pub running: HashMap<usize, Job>,
    next: usize,
    cli: Option<CliStatus>,
}

impl CloudJobs {
    /// Registre vide (compteur jobs dès 1, 0 = requêtes `list`/`delete`).
    pub fn new() -> Self {
        Self {
            running: HashMap::new(),
            next: 1,
            cli: None,
        }
    }

    /// Détecte `kiro-cli` (bloquant ~100 ms, une seule fois : caché).
    /// `Err` = message affichable (absent ou trop vieux pour le cloud).
    pub fn probe(&mut self) -> Result<CliStatus, String> {
        if let Some(cached) = &self.cli {
            return Ok(cached.clone());
        }
        let path = cli_path().ok_or_else(|| {
            String::from(
                "kiro-cli introuvable — installe : curl -fsSL https://cli.kiro.dev/install | bash",
            )
        })?;
        let out = std::process::Command::new(&path)
            .arg("--version")
            .output()
            .map_err(|err| format!("kiro-cli --version : {err}"))?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let version = parse_version(&text)
            .ok_or_else(|| format!("kiro-cli : version illisible ({text:?})"))?;
        if !cloud_supported(version) {
            let (major, minor, patch) = version;
            return Err(format!(
                "kiro-cli {major}.{minor}.{patch} trop vieux — cloud dès 2.17 (mets à jour)"
            ));
        }
        let status = CliStatus { path, version };
        self.cli = Some(status.clone());
        Ok(status)
    }

    /// Enregistre un run lancé, rend son id (`[cloud#n]`).
    pub fn register(&mut self, stop: tokio::sync::oneshot::Sender<()>, repo: String) -> usize {
        let id = self.next;
        self.next += 1;
        self.running.insert(
            id,
            Job {
                stop,
                repo,
                session: None,
            },
        );
        id
    }

    /// Retire un job (fin ou `stop`), rend le job s'il existait.
    pub fn take(&mut self, id: usize) -> Option<Job> {
        self.running.remove(&id)
    }

    /// Mémorise l'id de session capté dans le stream.
    pub fn note_session(&mut self, id: usize, session: String) {
        if let Some(job) = self.running.get_mut(&id) {
            job.session.get_or_insert(session);
        }
    }
}

/// Chemin `kiro-cli` : `KH_KIRO_CLI` prime (tests), sinon balayage
/// `KH_KIRO_PATH` si posée (tests hermétiques, `PATH` global intact),
/// sinon `PATH`.
pub fn cli_path() -> Option<PathBuf> {
    if let Some(over) = crate::kiro::read_env("KH_KIRO_CLI") {
        return Some(PathBuf::from(over));
    }
    let path = match crate::kiro::read_env("KH_KIRO_PATH") {
        Some(search) => std::ffi::OsString::from(search),
        None => std::env::var_os("PATH")?,
    };
    std::env::split_paths(&path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join("kiro-cli"))
        .find(|candidate| candidate.is_file())
}

/// Premier triplet `N.N.N` trouvé (`kiro-cli --version`, `None` sinon).
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut token = String::new();
    for ch in text.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_digit() || ch == '.' {
            token.push(ch);
        } else {
            if let Some(found) = triplet(&token) {
                return Some(found);
            }
            token.clear();
        }
    }
    None
}

/// Un jeton `chiffres.points` vaut triplet s'il a 3 parts numériques.
fn triplet(token: &str) -> Option<(u64, u64, u64)> {
    let mut parts = token.split('.');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(a), Some(b), Some(c), None) => match (a.parse(), b.parse(), c.parse()) {
            (Ok(x), Ok(y), Ok(z)) => Some((x, y, z)),
            _ => None,
        },
        _ => None,
    }
}

/// Cloud dispo : majeur > 2, ou 2.x avec x ≥ 17 (docs CLI 2.17+).
pub fn cloud_supported(version: (u64, u64, u64)) -> bool {
    let (major, minor, _) = version;
    major > MIN_CLI_MAJOR || (major == MIN_CLI_MAJOR && minor >= MIN_CLI_MINOR)
}

/// `argv` d'un run cloud headless (contrat doc headless + `--trust-all-tools`,
/// le cloud tourne sans approbation par changement de toute façon).
pub fn run_argv(repo: &str, task: &str) -> Vec<String> {
    [
        "chat",
        "--v3",
        "--no-interactive",
        "--cloud",
        "--repo",
        repo,
        "--trust-all-tools",
        "--output-format",
        "stream-json",
        task,
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// `argv` liste des sessions (locales + cloud, colonnes env/status du CLI).
pub fn list_argv() -> Vec<String> {
    ["chat", "--list-sessions"]
        .into_iter()
        .map(String::from)
        .collect()
}

/// `argv` suppression d'une session (`--delete-session <id>`).
pub fn delete_argv(id: &str) -> Vec<String> {
    ["chat", "--delete-session", id]
        .into_iter()
        .map(String::from)
        .collect()
}

/// Reprise manuelle : la session vit dans le cloud, on la reprend hors
/// Crypte (terminal, web, mobile) — pas d'URL session inventée.
pub fn attach_text(id: &str) -> String {
    format!(
        "cloud session {id} — resume outside Crypte:\n  kiro-cli --resume-id {id}\n  web: https://app.kiro.dev (same session everywhere)"
    )
}

/// Une ligne de sortie CLI → évènement affiché (`None` si blanche).
/// JSON défensif (schéma `stream-json` non documenté) : id de session
/// cherché sous `session_id`/`sessionId`/`session.id`, texte sous
/// `text`/`message`/`content`/`delta`/`error`, sinon ligne brute.
pub fn parse_line(line: &str) -> Option<CloudLine> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        let session = session_of(&value);
        let text = text_of(&value).unwrap_or_else(|| trimmed.to_string());
        let text = crate::provider::truncate(&text, 500);
        return Some(CloudLine { text, session });
    }
    let text = crate::provider::truncate(trimmed, 500);
    Some(CloudLine {
        text,
        session: None,
    })
}

/// Id de session dans un évènement JSON (`None` si absent).
fn session_of(value: &serde_json::Value) -> Option<String> {
    for key in ["session_id", "sessionId"] {
        if let Some(id) = value.get(key).and_then(|v| v.as_str()) {
            return Some(id.to_string());
        }
    }
    value
        .get("session")
        .and_then(|s| s.get("id"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Texte d'un évènement JSON (`error` préfixé, `None` si inconnu).
fn text_of(value: &serde_json::Value) -> Option<String> {
    if let Some(err) = value.get("error").and_then(|v| v.as_str()) {
        return Some(format!("error: {err}"));
    }
    ["text", "message", "content", "delta"]
        .into_iter()
        .filter_map(|key| value.get(key).and_then(|v| v.as_str()))
        .map(str::to_string)
        .next()
}

/// Dépôt du run : `--repo` explicite prime (attendu `owner/repo`),
/// sinon `origin` du dossier projet.
pub fn resolve_repo(explicit: Option<&str>, remote: Option<&str>) -> Result<String, &'static str> {
    if let Some(want) = explicit {
        let want = want.trim();
        let ok = want.contains('/')
            && !want.split_whitespace().nth(1).is_some()
            && want.split('/').all(|p| !p.is_empty());
        return ok
            .then(|| want.to_string())
            .ok_or("invalid repo (want owner/repo)");
    }
    remote
        .and_then(repo_from_remote)
        .ok_or("no repo: run from a git clone or /cloud run --repo owner/repo")
}

/// `owner/repo` depuis une URL `origin` GitHub (https, ssh, scp).
fn repo_from_remote(remote: &str) -> Option<String> {
    let remote = remote.trim().trim_end_matches('/').trim_end_matches(".git");
    let path = remote
        .strip_prefix("https://github.com/")
        .or_else(|| remote.strip_prefix("http://github.com/"))
        .or_else(|| remote.strip_prefix("git@github.com:"))
        .or_else(|| remote.strip_prefix("ssh://git@github.com/"))?;
    let mut parts = path.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    (parts.next().is_none() && !owner.is_empty() && !repo.is_empty())
        .then(|| format!("{owner}/{repo}"))
}

/// URL `origin` du dossier (`None` : git absent ou hors dépôt).
pub fn git_remote(dir: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// État git (bloquant ~ms) : `None` si git absent ou hors dépôt.
pub fn git_state(dir: &Path) -> Option<GitState> {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["status", "--porcelain"])
        .output()
        .ok()?;
    if !status.status.success() {
        return None;
    }
    let dirty = !String::from_utf8_lossy(&status.stdout).trim().is_empty();
    let ahead = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-list", "--count", "@{u}..HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| {
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse::<usize>()
                .ok()
        });
    Some(GitState {
        readable: true,
        dirty,
        unpushed: ahead,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `parse_version` trouve le triplet, `None` sans triplet.
    #[test]
    fn version_parse_trouve_triplet() {
        assert_eq!(parse_version("kiro-cli 2.17.3"), Some((2, 17, 3)));
        assert_eq!(parse_version("v3.2.0 (v3)"), Some((3, 2, 0)));
        assert_eq!(parse_version("nope"), None);
        assert_eq!(parse_version("2.17"), None);
    }

    /// Seuil cloud : 2.17+, majeur supérieur toujours OK.
    #[test]
    fn version_seuil_cloud_2_17() {
        assert!(!cloud_supported((2, 16, 9)));
        assert!(cloud_supported((2, 17, 0)));
        assert!(cloud_supported((3, 0, 0)));
    }

    /// Contrat `argv` : flags doc headless, dans l'ordre.
    #[test]
    fn run_argv_contrat_doc() {
        assert_eq!(
            run_argv("o/r", "do it"),
            vec![
                "chat",
                "--v3",
                "--no-interactive",
                "--cloud",
                "--repo",
                "o/r",
                "--trust-all-tools",
                "--output-format",
                "stream-json",
                "do it",
            ]
        );
    }

    /// `argv` liste + suppression.
    #[test]
    fn list_delete_argv() {
        assert_eq!(list_argv(), vec!["chat", "--list-sessions"]);
        assert_eq!(delete_argv("abc"), vec!["chat", "--delete-session", "abc"]);
    }

    /// JSON : session + texte extraits, `error` préfixé.
    #[test]
    fn parse_line_json_session_texte() {
        let line = parse_line(r#"{"session_id":"s1","text":"hi"}"#).unwrap();
        assert_eq!(line.text, "hi");
        assert_eq!(line.session.as_deref(), Some("s1"));
        let nested = parse_line(r#"{"session":{"id":"s2"},"message":"yo"}"#).unwrap();
        assert_eq!(nested.session.as_deref(), Some("s2"));
        let err = parse_line(r#"{"error":"boom"}"#).unwrap();
        assert_eq!(err.text, "error: boom");
        assert!(err.session.is_none());
    }

    /// Ligne brute gardée telle quelle, blanche ignorée.
    #[test]
    fn parse_line_brute_et_vide() {
        assert!(parse_line("   ").is_none());
        let raw = parse_line("  hello  ").unwrap();
        assert_eq!(raw.text, "hello");
        assert!(raw.session.is_none());
    }

    /// Résolution dépôt : explicite prime, sinon `origin` (https/ssh/scp).
    #[test]
    fn resolve_repo_priorites() {
        assert_eq!(resolve_repo(Some("o/r"), None).as_deref(), Ok("o/r"));
        assert!(resolve_repo(Some("nope"), None).is_err());
        assert_eq!(
            resolve_repo(None, Some("https://github.com/BaptisteFaisy/Crypte.git")).as_deref(),
            Ok("BaptisteFaisy/Crypte")
        );
        assert_eq!(
            resolve_repo(None, Some("git@github.com:o/r.git")).as_deref(),
            Ok("o/r")
        );
        assert_eq!(
            resolve_repo(None, Some("ssh://git@github.com/o/r")).as_deref(),
            Ok("o/r")
        );
        assert!(resolve_repo(None, Some("https://gitlab.com/o/r")).is_err());
        assert!(resolve_repo(None, None).is_err());
    }

    /// Hors dépôt : pas de remote, état git illisible (vrai avec ou sans git).
    #[test]
    fn git_hors_depot_illisible() {
        let dir = std::env::temp_dir().join(format!("kh-cloud-norepo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(git_remote(&dir).is_none());
        assert!(git_state(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Binaire absent : message installable (recherche vide, hermétique).
    #[test]
    fn probe_binaire_absent_message() {
        let scope = crate::test_env::EnvScope::new(&["KH_KIRO_PATH", "KH_KIRO_CLI"]);
        scope.set("KH_KIRO_PATH", "/nonexistent");
        scope.unset("KH_KIRO_CLI");
        let mut jobs = CloudJobs::new();
        let err = jobs.probe().unwrap_err();
        assert!(err.contains("introuvable"), "{err}");
        assert!(err.contains("cli.kiro.dev/install"), "{err}");
    }
}
