//! Texte `/kiro` (pur, testable) : steering, specs, hooks, MCP, ignore.

use crate::kiro::hooks::{Action, Hook};
use crate::kiro::spec::{next_task, progress, Spec};
use crate::kiro::steering::{Inclusion, Steering};
use crate::provider::truncate;
use std::path::Path;

/// Statut `.kiro/` projet + global (1 ligne + détails).
pub fn text(project: &Path, home: Option<&Path>) -> String {
    let steering = crate::kiro::steering::load_all_in(project, home);
    let specs = crate::kiro::spec::load_all(&project.join(".kiro/specs"));
    let hooks = crate::kiro::hooks::load_all_in(project, home);
    let hook_count: usize = hooks.iter().map(|named| named.hooks.len()).sum();
    let mcp = crate::kiro::mcp::load_merged_in(project, home);
    let servers = crate::kiro::mcp::enabled(&mcp);
    let ignore = crate::kiro::ignore::load(project);
    if steering.is_empty()
        && specs.is_empty()
        && hook_count == 0
        && servers.is_empty()
        && ignore.is_empty()
    {
        return String::from("kiro: nothing loaded (no .kiro/ here, no ~/.kiro/)");
    }
    let mut out = format!(
        "kiro: steering {} · specs {} · hooks {} · mcp {} · ignore {}",
        steering.len(),
        specs.len(),
        hook_count,
        servers.len(),
        ignore_text(&ignore),
    );
    if !steering.is_empty() {
        out.push_str(&format!("\n  steering: {}", steering_text(&steering)));
    }
    if !specs.is_empty() {
        let specs: Vec<String> = specs.iter().map(spec_text).collect();
        out.push_str(&format!("\n  specs: {}", specs.join(" · ")));
    }
    if hook_count > 0 {
        let hooks: Vec<String> = hooks
            .iter()
            .flat_map(|named| named.hooks.iter().map(|hook| hook_text(&named.file, hook)))
            .collect();
        out.push_str(&format!("\n  hooks: {}", hooks.join(" · ")));
    }
    if !servers.is_empty() {
        let servers: Vec<String> = servers
            .iter()
            .map(|(name, server)| server_text(name, server))
            .collect();
        out.push_str(&format!("\n  mcp: {}", servers.join(" · ")));
    }
    out
}

/// `off` ou `N patterns`.
fn ignore_text(ignore: &crate::kiro::ignore::Ignore) -> String {
    if ignore.is_empty() {
        String::from("off")
    } else {
        format!("{} patterns", ignore.len())
    }
}

/// `a, b (always) · c (manual)` (actifs puis parqués).
fn steering_text(files: &[Steering]) -> String {
    let active: Vec<&str> = files
        .iter()
        .filter(|file| file.inclusion == Inclusion::Always)
        .map(|file| file.name.as_str())
        .collect();
    let mut parked: Vec<String> = files
        .iter()
        .filter(|file| file.inclusion != Inclusion::Always)
        .map(|file| format!("{} ({})", file.name, file.inclusion.as_str()))
        .collect();
    let mut bits = Vec::new();
    if !active.is_empty() {
        bits.push(format!("{} (always)", active.join(", ")));
    }
    bits.append(&mut parked);
    bits.join(" · ")
}

/// `auth (req, design, tasks 2/5, next: …)`.
fn spec_text(spec: &Spec) -> String {
    let mut bits = Vec::new();
    if spec.requirements.is_some() {
        bits.push(String::from("req"));
    }
    if spec.design.is_some() {
        bits.push(String::from("design"));
    }
    let (done, total) = progress(&spec.tasks);
    if total > 0 {
        bits.push(format!("tasks {done}/{total}"));
        if let Some(next) = next_task(&spec.tasks) {
            bits.push(format!("next: {}", truncate(next, 40)));
        }
    }
    format!("{} ({})", spec.name, bits.join(", "))
}

/// `fmt [verify.json] (PostFileSave \.rs$, command: cargo fmt)`.
fn hook_text(file: &str, hook: &Hook) -> String {
    let mut head = hook.trigger.as_str().to_string();
    if let Some(matcher) = &hook.matcher {
        head.push(' ');
        head.push_str(&truncate(matcher, 20));
    }
    format!(
        "{} [{file}] ({head}, {})",
        hook.name,
        action_text(&hook.action)
    )
}

/// `command: cargo test` (ou `agent: …`, `?` si vide).
fn action_text(action: &Action) -> String {
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
    format!("{kind}: {}", truncate(detail, 30))
}

/// `github (stdio: npx …, 2 auto-approve)`.
fn server_text(name: &str, server: &crate::kiro::mcp::Server) -> String {
    let mut bits = Vec::new();
    if let Some(command) = &server.command {
        let mut full = command.clone();
        if !server.args.is_empty() {
            full.push(' ');
            full.push_str(&server.args.join(" "));
        }
        bits.push(format!("stdio: {}", truncate(&full, 40)));
    }
    if let Some(url) = &server.url {
        bits.push(format!("remote: {}", truncate(url, 40)));
    }
    if !server.env.is_empty() {
        bits.push(format!("{} env", server.env.len()));
    }
    if !server.auto_approve.is_empty() {
        bits.push(format!("{} auto-approve", server.auto_approve.len()));
    }
    if bits.is_empty() {
        bits.push(String::from("empty"));
    }
    format!("{name} ({})", bits.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("kh-kiro-status-{}-{name}", std::process::id()))
    }

    #[test]
    fn vide_annonce_rien() {
        let dir = tmp_dir("empty");
        std::fs::create_dir_all(&dir).unwrap();
        let out = text(&dir, Some(&dir.join("home")));
        assert!(out.contains("nothing loaded"), "{out}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Écrit une fixture `.kiro/` complète (projet + global).
    fn seed_full(project: &Path, global: &Path) {
        std::fs::create_dir_all(global.join("steering")).unwrap();
        std::fs::create_dir_all(project.join(".kiro/steering")).unwrap();
        std::fs::create_dir_all(project.join(".kiro/specs/auth")).unwrap();
        std::fs::create_dir_all(project.join(".kiro/hooks")).unwrap();
        std::fs::create_dir_all(project.join(".kiro/settings")).unwrap();
        std::fs::write(global.join("steering/product.md"), "vision").unwrap();
        std::fs::write(
            project.join(".kiro/steering/review.md"),
            "---\ninclusion: manual\n---\nrelis tout",
        )
        .unwrap();
        std::fs::write(project.join(".kiro/specs/auth/requirements.md"), "req").unwrap();
        std::fs::write(
            project.join(".kiro/specs/auth/tasks.md"),
            "- [ ] code\n- [x] test",
        )
        .unwrap();
        std::fs::write(
            project.join(".kiro/hooks/fmt.json"),
            r#"{"hooks":[{"name":"fmt","trigger":"PostFileSave","matcher":"\\.rs$",
               "action":{"type":"command","command":"cargo fmt"}}]}"#,
        )
        .unwrap();
        std::fs::write(
            project.join(".kiro/settings/mcp.json"),
            r#"{"mcpServers":{"github":{"command":"gh-mcp","args":["serve"],"autoApprove":["a","b"]}}}"#,
        )
        .unwrap();
        std::fs::write(project.join(".kiroignore"), "target/\n*.log\n").unwrap();
    }

    #[test]
    fn garni_detaille_tout() {
        let dir = tmp_dir("full");
        let (global, project) = (dir.join("home/.kiro"), dir.join("proj"));
        seed_full(&project, &global);
        let out = text(&project, Some(&global));
        for bit in [
            "steering 2",
            "specs 1",
            "hooks 1",
            "mcp 1",
            "2 patterns",
            "product (always)",
            "review (manual)",
            "auth (req, tasks 1/2, next: code)",
            "fmt [fmt.json] (PostFileSave",
            "command: cargo fmt",
            "github (stdio: gh-mcp serve, 2 auto-approve)",
        ] {
            assert!(out.contains(bit), "manque {bit:?} dans :\n{out}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
