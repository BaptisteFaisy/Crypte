//! `/vps` : lancer ce TUI à distance + écran de pilotage.
//!
//! `impl App` déporté (limite 500 lignes) : handlers, tâches ssh de
//! fond (`Incoming::Vps`), rendu écran, touches écran. Le ssh tourne
//! en tâche bornée (`pool`), le TUI reste fluide pendant ce temps.

use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::Frame;

use crate::app::{App, Incoming, Screen};
use crate::vps::{self, VpsOutcome, VpsProfile};

impl App {
    /// `/vps ...` : écran (sans arg), launch, off, status, add, del, install.
    pub(crate) fn cmd_vps(&mut self, args: Vec<String>) {
        let mut words = args.iter().map(String::as_str);
        match words.next() {
            None => self.open_vps(),
            Some("off") => self.vps_stop(words.next()),
            Some("status") => self.vps_status(words.next()),
            Some("add") => self.vps_add(words),
            Some("del") => self.vps_del(words.next()),
            Some("install") => self.vps_install(words.next()),
            Some("help") => self.push_system(vps_usage()),
            Some(target) => self.vps_launch(target),
        }
    }

    /// Ouvre l'écran VPS (liste des profils + statuts).
    fn open_vps(&mut self) {
        self.vps.clamp_selection();
        self.screen = Screen::Vps;
    }

    /// `/vps <ip|nom>` : (re)lance le TUI distant en tmux.
    fn vps_launch(&mut self, target: &str) {
        let profile = match vps::resolve(target, &self.vps.profiles) {
            Ok(profile) => profile,
            Err(error) => {
                self.push_system(format!("vps: {error}"));
                return;
            }
        };
        self.push_system(format!("vps: launching on {} …", profile.dst()));
        self.vps.current = Some(profile.name.clone());
        self.spawn_vps_op(profile, "launch");
    }

    /// `/vps off [cible]` : tue la session distante (défaut : dernière).
    fn vps_stop(&mut self, target: Option<&str>) {
        let target = match target.or(self.vps.current.as_deref()) {
            Some(target) => target.to_string(),
            None => {
                self.push_system(String::from("vps: nothing launched here (give a target)"));
                return;
            }
        };
        let profile = match vps::resolve(&target, &self.vps.profiles) {
            Ok(profile) => profile,
            Err(error) => {
                self.push_system(format!("vps: {error}"));
                return;
            }
        };
        self.spawn_vps_op(profile, "stop");
    }

    /// `/vps status [cible]` : session up/down (défaut : tous profils).
    fn vps_status(&mut self, target: Option<&str>) {
        match target {
            Some(target) => match vps::resolve(target, &self.vps.profiles) {
                Ok(profile) => self.spawn_vps_op(profile, "status"),
                Err(error) => self.push_system(format!("vps: {error}")),
            },
            None => {
                if self.vps.profiles.is_empty() {
                    self.push_system(String::from(
                        "vps: no profiles — /vps add <name> <user@host>",
                    ));
                    return;
                }
                for profile in self.vps.profiles.clone() {
                    self.spawn_vps_op(profile, "status");
                }
            }
        }
    }

    /// `/vps install <cible>` : clone/pull + build release distant.
    fn vps_install(&mut self, target: Option<&str>) {
        let Some(target) = target else {
            self.push_system(String::from("vps: usage: /vps install <ip|name>"));
            return;
        };
        let profile = match vps::resolve(target, &self.vps.profiles) {
            Ok(profile) => profile,
            Err(error) => {
                self.push_system(format!("vps: {error}"));
                return;
            }
        };
        self.push_system(format!("vps: installing on {} (minutes) …", profile.dst()));
        self.spawn_vps_op(profile, "install");
    }

    /// `/vps add <nom> <user@host> [--port P] [--key F] [--dir D]`.
    fn vps_add<'a>(&mut self, words: impl Iterator<Item = &'a str>) {
        let rest: Vec<&str> = words.collect();
        if rest.len() < 2 {
            self.push_system(String::from(
                "vps: usage: /vps add <name> <user@host> [--port P] [--key F] [--dir D]",
            ));
            return;
        }
        let (name, dst) = (rest[0], rest[1]);
        let mut profile = match vps::resolve(dst, &[]) {
            Ok(profile) => profile,
            Err(error) => {
                self.push_system(format!("vps: {error}"));
                return;
            }
        };
        profile.name = name.to_string();
        let mut flags = rest[2..].iter().peekable();
        while let Some(flag) = flags.next() {
            let value = flags.next();
            match (*flag, value) {
                ("--port", Some(port)) => match port.parse::<u16>() {
                    Ok(port) => profile.port = port,
                    Err(_) => {
                        self.push_system(format!("vps: bad port {port:?}"));
                        return;
                    }
                },
                ("--key", Some(key)) => profile.key = Some(key.into()),
                ("--dir", Some(dir)) => profile.dir = dir.to_string(),
                _ => {
                    self.push_system(format!("vps: bad flag {flag:?} (want --port/--key/--dir)"));
                    return;
                }
            }
        }
        let updated = self.vps.find(name).is_some();
        if updated {
            self.vps.profiles.retain(|p| p.name != name);
        }
        self.vps.profiles.push(profile.clone());
        match vps::save(&self.vps.profiles) {
            Ok(()) => self.push_system(format!(
                "vps: profile {name:?} {} ({})",
                if updated { "updated" } else { "saved" },
                profile.dst(),
            )),
            Err(error) => self.push_system(format!("vps: save failed: {error}")),
        }
    }

    /// `/vps del <nom>` : oublie un profil.
    fn vps_del(&mut self, name: Option<&str>) {
        let Some(name) = name else {
            self.push_system(String::from("vps: usage: /vps del <name>"));
            return;
        };
        if self.vps.find(name).is_none() {
            self.push_system(format!("vps: unknown profile {name:?}"));
            return;
        }
        self.vps.profiles.retain(|p| p.name != name);
        self.vps.clamp_selection();
        match vps::save(&self.vps.profiles) {
            Ok(()) => self.push_system(format!("vps: profile {name:?} deleted")),
            Err(error) => self.push_system(format!("vps: save failed: {error}")),
        }
    }

    /// Lance `op` (`launch|stop|status|install`) en tâche de fond.
    fn spawn_vps_op(&mut self, profile: VpsProfile, op: &'static str) {
        let cmd = match op {
            "launch" => profile.launch_cmd(),
            "stop" => profile.stop_cmd(),
            "status" => profile.status_cmd(),
            _ => profile.install_cmd(),
        };
        self.vps.set(&profile.name, vps::VpsStatus::Working);
        let tx = self.tx.clone();
        self.pool.spawn(async move {
            let outcome = run_ssh(&profile, op, &cmd).await;
            let _ = tx.send(Incoming::Vps(outcome)).await;
        });
    }

    /// Aboutissement ssh : statut mémorisé + feedback dans le chat.
    pub(crate) fn drain_vps(&mut self, outcome: VpsOutcome) {
        match outcome {
            VpsOutcome::Launched { target } => {
                self.vps.set(&target, vps::VpsStatus::Up);
                self.vps.current = Some(target.clone());
                let dst = self.ssh_prefix(&target);
                self.push_system(format!(
                    "vps: {target} UP — attach: {dst} -t 'tmux attach -t crypte' · \
                     events: {dst} 'tail -F ~/.crypte-log/events.jsonl'"
                ));
            }
            VpsOutcome::Stopped { target } => {
                self.vps.set(&target, vps::VpsStatus::Down);
                self.push_system(format!("vps: {target} stopped"));
            }
            VpsOutcome::Status { target, up } => {
                self.vps.set(
                    &target,
                    if up {
                        vps::VpsStatus::Up
                    } else {
                        vps::VpsStatus::Down
                    },
                );
                self.push_system(format!("vps: {target} {}", if up { "UP" } else { "DOWN" }));
            }
            VpsOutcome::Installed { target } => {
                self.vps.set(&target, vps::VpsStatus::Down);
                self.push_system(format!("vps: {target} installed — /vps {target} to launch"));
            }
            VpsOutcome::Failed { target, op, error } => {
                self.vps.set(&target, vps::VpsStatus::Failed(error.clone()));
                self.push_system(format!("vps: {op} {target}: {error}"));
            }
        }
    }

    /// Préfixe ssh affichable (`ssh [-p P] [-i K] user@host`).
    fn ssh_prefix(&self, target: &str) -> String {
        match self.vps.find(target) {
            Some(profile) => {
                let mut argv = profile.ssh_argv();
                argv.pop();
                format!("ssh {}", argv.join(" "))
            }
            None => format!("ssh {target}"),
        }
    }

    /// Écran VPS : profils + statuts + aide (↑/↓, Entrée, Échap).
    pub(crate) fn draw_vps(&mut self, frame: &mut Frame, area: Rect) {
        use ratatui::style::Style;
        use ratatui::widgets::Paragraph;

        use crate::theme::BONE;
        let mut lines = vec![
            String::from("VPS — launch this TUI remotely  (/vps <ip|name>)"),
            String::new(),
        ];
        if self.vps.profiles.is_empty() {
            lines.push(String::from("  no profiles — /vps add <name> <user@host>"));
        }
        for (i, profile) in self.vps.profiles.iter().enumerate() {
            let mark = if i == self.vps.selected { "▸" } else { " " };
            let status = match self.vps.status_of(&profile.name) {
                vps::VpsStatus::Up => "UP",
                vps::VpsStatus::Down => "DOWN",
                vps::VpsStatus::Working => "…",
                vps::VpsStatus::Failed(_) => "FAIL",
                vps::VpsStatus::Unknown => "?",
            };
            lines.push(format!(
                "{mark} {:<12} {:<28} [{}]",
                profile.name,
                profile.dst(),
                status
            ));
        }
        lines.push(String::new());
        lines.push(String::from(
            "keys: ↑/↓ select · Enter launch · Esc back · q quit",
        ));
        lines.push(String::from(
            "cmds: /vps <ip|name> · /vps off|status|install · /vps add|del · /vps help",
        ));
        let paragraph = Paragraph::new(lines.join("\n")).style(Style::new().fg(BONE));
        frame.render_widget(paragraph, area);
    }

    /// Touches écran VPS : ↑/↓ (sélection), Entrée (lance), Échap (chat).
    /// `q` quitte l'app (comme sur le hub).
    pub(crate) fn on_vps_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char('q') => false,
            KeyCode::Up => self.cmd(|s| s.vps_move(-1)),
            KeyCode::Down => self.cmd(|s| s.vps_move(1)),
            KeyCode::Enter => self.cmd(|s| s.vps_launch_selected()),
            KeyCode::Esc => self.cmd(|s| s.screen = Screen::Chat),
            _ => true,
        }
    }

    /// Déplace la sélection écran (cyclique).
    fn vps_move(&mut self, delta: isize) {
        let n = self.vps.profiles.len() as isize;
        self.vps.selected = (self.vps.selected as isize + delta).rem_euclid(n.max(1)) as usize;
    }

    /// Lance le profil sélectionné (depuis l'écran VPS).
    fn vps_launch_selected(&mut self) {
        let Some(name) = self
            .vps
            .profiles
            .get(self.vps.selected)
            .map(|p| p.name.clone())
        else {
            self.push_system(String::from(
                "vps: no profiles — /vps add <name> <user@host>",
            ));
            return;
        };
        self.screen = Screen::Chat;
        self.vps_launch(&name);
    }
}

/// `ssh … -- <cmd>` puis traduction de la sortie en [`VpsOutcome`].
async fn run_ssh(profile: &VpsProfile, op: &'static str, cmd: &str) -> VpsOutcome {
    let target = profile.name.clone();
    let fail = |error: String| VpsOutcome::Failed {
        target: target.clone(),
        op: op.to_string(),
        error,
    };
    let mut argv = profile.ssh_argv();
    argv.push(cmd.to_string());
    // stdin coupé : sinon ssh le dévore et le TUI devient sourd.
    let out = match tokio::process::Command::new("ssh")
        .args(&argv)
        .stdin(std::process::Stdio::null())
        .output()
        .await
    {
        Ok(out) => out,
        Err(error) => return fail(format!("ssh: {error}")),
    };
    match op {
        "status" => match vps::parse_status(&out) {
            Ok(up) => VpsOutcome::Status { target, up },
            Err(error) => fail(error),
        },
        "launch" => match vps::parse_marker(&out, op, "UP") {
            Ok(()) => VpsOutcome::Launched { target },
            Err(error) => fail(error),
        },
        "stop" => match vps::parse_marker(&out, op, "STOPPED") {
            Ok(()) => VpsOutcome::Stopped { target },
            Err(error) => fail(error),
        },
        _ => match vps::parse_marker(&out, op, "INSTALLED") {
            Ok(()) => VpsOutcome::Installed { target },
            Err(error) => fail(error),
        },
    }
}

/// Aide `/vps help` (aussi résumée sur l'écran VPS).
fn vps_usage() -> String {
    String::from(
        "vps: launch this TUI on a remote machine (SSH key only)\n\
         /vps — open this screen\n\
         /vps <ip|name> — launch there (name = saved profile, else [user@]host[:port])\n\
         /vps off [target] — stop the remote session (default: last launched)\n\
         /vps status [target] — refresh (default: all profiles)\n\
         /vps install <target> — clone/pull + release build there\n\
         /vps add <name> <user@host> [--port P] [--key F] [--dir D] — save a profile\n\
         /vps del <name> — forget a profile",
    )
}
