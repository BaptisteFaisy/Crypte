//! VPS : lancer ce TUI à distance (`/vps <ip|nom>`).
//!
//! Profils nommés dans `~/.config/kiro-horror-tui/vps.toml` (`[[vps]]`).
//! Une cible inconnue est lue comme `[user@]host[:port]` (ad hoc).
//! Opérations via `ssh` (clé uniquement, `BatchMode`) en tâche de fond,
//! résultats dans [`VpsOutcome`] (routés par `drain_vps`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Output;

use serde::{Deserialize, Serialize};

/// Nom de session tmux distante (émission et lecture doivent matcher).
pub const SESSION: &str = "crypte";
/// Dépôt cloné par `/vps install`.
pub const REPO_URL: &str = "https://github.com/BaptisteFaisy/Crypte.git";
/// Dossier config (même base que chats.json, voir `persist`).
const STATE_DIR: &str = ".config/kiro-horror-tui";
/// Fichier des profils nommés.
const VPS_FILE: &str = "vps.toml";

/// Profil VPS (nommé ou ad hoc).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpsProfile {
    /// Nom du profil (ou cible tapée, en ad hoc).
    pub name: String,
    /// Hôte : IP ou nom DNS (pas d'IPv6).
    pub host: String,
    /// Utilisateur SSH.
    pub user: String,
    /// Port SSH (22 par défaut).
    #[serde(default = "default_port")]
    pub port: u16,
    /// Clé SSH (`None` = clés par défaut / agent).
    #[serde(default)]
    pub key: Option<PathBuf>,
    /// Checkout distant (`~/crypte` par défaut).
    #[serde(default = "default_dir")]
    pub dir: String,
}

/// Port SSH par défaut (désérialisation).
fn default_port() -> u16 {
    22
}

/// Checkout distant par défaut (désérialisation).
fn default_dir() -> String {
    String::from("~/crypte")
}

impl VpsProfile {
    /// `user@host[:port]` (affichage + ssh).
    pub fn dst(&self) -> String {
        if self.port == 22 {
            format!("{}@{}", self.user, self.host)
        } else {
            format!("{}@{}:{}", self.user, self.host, self.port)
        }
    }

    /// argv ssh jusqu'à la destination (commande distante ensuite).
    /// Clé uniquement : `BatchMode` refuse tout prompt de mot de passe.
    /// `-n` : ssh ne touche pas au stdin (le TUI resterait sourd).
    /// Clé d'hôte inconnue acceptée (TOFU, 1re connexion seulement).
    pub fn ssh_argv(&self) -> Vec<String> {
        let mut argv = vec![
            String::from("-n"),
            String::from("-o"),
            String::from("BatchMode=yes"),
            String::from("-o"),
            String::from("StrictHostKeyChecking=accept-new"),
            String::from("-o"),
            String::from("ConnectTimeout=10"),
            String::from("-p"),
            self.port.to_string(),
        ];
        if let Some(key) = &self.key {
            argv.push(String::from("-i"));
            argv.push(key.display().to_string());
        }
        argv.push(format!("{}@{}", self.user, self.host));
        argv.push(String::from("--"));
        argv
    }

    /// Binaire release distant.
    fn bin(&self) -> String {
        format!("{}/target/release/kiro-horror-tui", self.dir)
    }

    /// Présence de la session tmux (code 0 = up, 1 = down).
    pub fn status_cmd(&self) -> String {
        format!("tmux has-session -t {} 2>/dev/null", SESSION)
    }

    /// (Re)lance le TUI en tmux + journal events. `NOBIN` si absent.
    pub fn launch_cmd(&self) -> String {
        format!(
            "BIN={}; [ -x \"$BIN\" ] || {{ echo NOBIN; exit 3; }}; \
             mkdir -p ~/.crypte-log; tmux kill-session -t {s} 2>/dev/null; \
             tmux new-session -d -s {s} -c {d} \
             \"$BIN --events-log ~/.crypte-log/events.jsonl\" && echo UP",
            self.bin(),
            s = SESSION,
            d = self.dir,
        )
    }

    /// Tue la session tmux (toujours `STOPPED` si ssh passe).
    pub fn stop_cmd(&self) -> String {
        format!("tmux kill-session -t {} 2>/dev/null; echo STOPPED", SESSION)
    }

    /// Clone/pull + build release. `~/.cargo/env` sourcé : un ssh
    /// non interactif n'a pas forcément cargo dans son `PATH`.
    /// `NOCARGO` si rustup absent, `INSTALLED` si build OK.
    pub fn install_cmd(&self) -> String {
        format!(
            ". ~/.cargo/env 2>/dev/null; \
             if [ -d {d}/.git ]; then git -C {d} pull --ff-only || exit 5; \
             else git clone {repo} {d} || exit 5; fi; \
             command -v cargo >/dev/null || {{ echo NOCARGO; exit 4; }}; \
             cd {d} && cargo build --release && echo INSTALLED",
            d = self.dir,
            repo = REPO_URL,
        )
    }
}

/// `target` : nom de profil (exact) ou `[user@]host[:port]` ad hoc
/// (`user` défaut = `$USER`). Erreur si vide ou invalide.
pub fn resolve(target: &str, profiles: &[VpsProfile]) -> Result<VpsProfile, String> {
    if let Some(found) = profiles.iter().find(|p| p.name == target) {
        return Ok(found.clone());
    }
    let target = target.trim();
    if target.is_empty() {
        return Err(String::from(
            "empty target (want a name or [user@]host[:port])",
        ));
    }
    let (user, rest) = match target.split_once('@') {
        Some((user, rest)) => (user.to_string(), rest),
        None => (
            std::env::var("USER").unwrap_or_else(|_| String::from("root")),
            target,
        ),
    };
    let (host, port) = match rest.rsplit_once(':') {
        Some((host, port)) => {
            let port = port
                .parse::<u16>()
                .map_err(|_| format!("bad port in {target:?}"))?;
            (host, port)
        }
        None => (rest, 22),
    };
    if user.is_empty() || host.is_empty() {
        return Err(format!("bad target {target:?} (want [user@]host[:port])"));
    }
    Ok(VpsProfile {
        name: target.to_string(),
        host: host.to_string(),
        user,
        port,
        key: None,
        dir: default_dir(),
    })
}

/// Chemin du fichier des profils (`None` si `HOME` manquant).
pub fn path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        let mut path = PathBuf::from(home);
        path.push(STATE_DIR);
        path.push(VPS_FILE);
        path
    })
}

/// Racine TOML (`[[vps]]`).
#[derive(Debug, Default, Serialize, Deserialize)]
struct VpsFile {
    #[serde(default)]
    vps: Vec<VpsProfile>,
}

/// Profils depuis `path` (absent/corrompu = vide ; corrompu conservé
/// en `.corrupt`, comme chats.json).
pub fn load_from(path: &Path) -> Vec<VpsProfile> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    match toml::from_str::<VpsFile>(&text) {
        Ok(file) => file.vps,
        Err(_) => {
            let corrupt = path.with_extension("toml.corrupt");
            let _ = std::fs::rename(path, corrupt);
            Vec::new()
        }
    }
}

/// Écrit les profils (crée les dossiers parents).
pub fn save_to(path: &Path, profiles: &[VpsProfile]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    let file = VpsFile {
        vps: profiles.to_vec(),
    };
    let text = toml::to_string(&file).map_err(|e| format!("toml: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("write: {e}"))
}

/// Profils du chemin par défaut (vide si `HOME` manquant).
pub fn load() -> Vec<VpsProfile> {
    path().map(|p| load_from(&p)).unwrap_or_default()
}

/// Sauve au chemin par défaut.
pub fn save(profiles: &[VpsProfile]) -> Result<(), String> {
    let Some(path) = path() else {
        return Err(String::from("no HOME, cannot save profiles"));
    };
    save_to(&path, profiles)
}

/// État distant connu d'une cible.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum VpsStatus {
    /// Jamais interrogé.
    #[default]
    Unknown,
    /// Requête en vol (`status`, `launch`…).
    Working,
    /// Session tmux présente.
    Up,
    /// Session tmux absente.
    Down,
    /// Échec (cause affichable).
    Failed(String),
}

/// Résultat d'une opération distante (tâche de fond → `drain_vps`).
#[derive(Debug, Clone)]
pub enum VpsOutcome {
    Launched {
        target: String,
    },
    Stopped {
        target: String,
    },
    Status {
        target: String,
        up: bool,
    },
    Installed {
        target: String,
    },
    Failed {
        target: String,
        op: String,
        error: String,
    },
}

/// Sortie ssh inexploitable (binaire absent, 255 = ssh lui-même).
fn ssh_failed(out: &Output) -> Option<String> {
    if out.status.code() == Some(255) {
        let tail = String::from_utf8_lossy(&out.stderr);
        return Some(format!(
            "ssh failed: {}",
            tail.lines().next().unwrap_or("?")
        ));
    }
    None
}

/// `status` : code 0 = up, 1 = down, 255 = ssh en erreur.
pub fn parse_status(out: &Output) -> Result<bool, String> {
    if let Some(error) = ssh_failed(out) {
        return Err(error);
    }
    Ok(out.status.success())
}

/// Marqueur attendu sur stdout (`UP`, `STOPPED`, `INSTALLED`…).
/// Cas spéciaux (`NOBIN`, `NOCARGO`) traduits en conseil.
pub fn parse_marker(out: &Output, op: &str, marker: &str) -> Result<(), String> {
    if let Some(error) = ssh_failed(out) {
        return Err(error);
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    if stdout.contains("NOBIN") {
        return Err(String::from(
            "not installed there — /vps install <target> first",
        ));
    }
    if stdout.contains("NOCARGO") {
        return Err(String::from(
            "cargo missing there — install rustup on the VPS first",
        ));
    }
    if out.status.success() && stdout.contains(marker) {
        return Ok(());
    }
    let tail = stdout.lines().next_back().unwrap_or("?");
    Err(format!(
        "{op} failed (code {:?}): {tail}",
        out.status.code()
    ))
}

/// État runtime : profils + statuts + sélection écran + dernière cible.
#[derive(Debug, Default)]
pub struct VpsRuntime {
    pub profiles: Vec<VpsProfile>,
    pub status: HashMap<String, VpsStatus>,
    pub selected: usize,
    pub current: Option<String>,
}

impl VpsRuntime {
    /// Charge les profils disque (vide si aucun).
    pub fn load() -> Self {
        Self {
            profiles: load(),
            ..Self::default()
        }
    }

    /// Statut connu (`Unknown` si jamais interrogé).
    pub fn status_of(&self, name: &str) -> VpsStatus {
        self.status.get(name).cloned().unwrap_or_default()
    }

    /// Mémorise un statut.
    pub fn set(&mut self, name: &str, status: VpsStatus) {
        self.status.insert(name.to_string(), status);
    }

    /// Profil par nom exact.
    pub fn find(&self, name: &str) -> Option<&VpsProfile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    /// Borne la sélection écran (liste peut rétrécir).
    pub fn clamp_selection(&mut self) {
        let max = self.profiles.len().saturating_sub(1);
        self.selected = self.selected.min(max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    /// `Output` factice (code + stdout).
    fn out(code: i32, stdout: &str) -> Output {
        Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    #[test]
    fn resolve_prefere_le_profil_puis_parse_ad_hoc() {
        let profiles = vec![VpsProfile {
            name: String::from("r630"),
            host: String::from("10.0.0.2"),
            user: String::from("u"),
            port: 22,
            key: None,
            dir: default_dir(),
        }];
        assert_eq!(resolve("r630", &profiles).unwrap().host, "10.0.0.2");
        let adhoc = resolve("b@1.2.3.4:2222", &profiles).unwrap();
        assert_eq!((adhoc.user, adhoc.port), (String::from("b"), 2222));
        let bare = resolve("1.2.3.4", &profiles).unwrap();
        assert_eq!((bare.host, bare.port), (String::from("1.2.3.4"), 22));
        assert!(resolve("", &profiles).is_err());
        assert!(resolve("h:notaport", &profiles).is_err());
    }

    #[test]
    fn ssh_argv_passe_port_cle_et_batch() {
        let mut profile = resolve("b@h", &[]).unwrap();
        profile.port = 2222;
        profile.key = Some(PathBuf::from("/tmp/k"));
        let argv = profile.ssh_argv();
        assert!(argv.contains(&String::from("-n")));
        assert!(argv.contains(&String::from("BatchMode=yes")));
        assert!(argv.contains(&String::from("StrictHostKeyChecking=accept-new")));
        assert!(argv.contains(&String::from("2222")));
        assert!(argv.contains(&String::from("/tmp/k")));
        assert_eq!(argv.last(), Some(&String::from("--")));
    }

    #[test]
    fn parse_status_distingue_up_down_et_ssh() {
        assert_eq!(parse_status(&out(0, "")), Ok(true));
        assert_eq!(parse_status(&out(1, "")), Ok(false));
        assert!(parse_status(&out(255, "")).is_err());
    }

    #[test]
    fn parse_marker_traduit_nobin_et_nocargo() {
        assert!(parse_marker(&out(0, "x\nUP\n"), "launch", "UP").is_ok());
        let nobin = parse_marker(&out(3, "NOBIN\n"), "launch", "UP").unwrap_err();
        assert!(nobin.contains("install"));
        let nocargo = parse_marker(&out(4, "NOCARGO\n"), "install", "INSTALLED").unwrap_err();
        assert!(nocargo.contains("cargo"));
        assert!(parse_marker(&out(255, ""), "launch", "UP").is_err());
    }

    #[test]
    fn profils_roundtrip_toml() {
        let path = std::env::temp_dir().join(format!("crypte-vps-{}.toml", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let profiles = vec![resolve("b@h:2222", &[]).unwrap()];
        save_to(&path, &profiles).unwrap();
        let back = load_from(&path);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].port, 2222);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn statut_defaut_et_borne_selection() {
        let mut runtime = VpsRuntime::default();
        assert_eq!(runtime.status_of("x"), VpsStatus::Unknown);
        runtime.set("x", VpsStatus::Up);
        assert_eq!(runtime.status_of("x"), VpsStatus::Up);
        runtime.selected = 9;
        runtime.clamp_selection();
        assert_eq!(runtime.selected, 0);
    }
}
