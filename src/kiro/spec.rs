//! Specs Kiro (`.kiro/specs/<nom>/{requirements,design,tasks}.md`).
//!
//! Lecture seule en T1 (workflow d'exécution = T2) : liste les specs,
//! parse les cases `tasks.md` (`- [ ]` / `- [x]`).

use std::path::Path;

/// Tâche (`- [ ] texte` / `- [x] texte`, `-` ou `*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub done: bool,
    pub text: String,
}

/// Parse les cases d'un `tasks.md` (autres lignes ignorées).
pub fn parse_tasks(md: &str) -> Vec<Task> {
    md.lines().filter_map(parse_task_line).collect()
}

/// Une ligne de case (`- [ ] …`, `- [x] …`, `*` accepté).
fn parse_task_line(line: &str) -> Option<Task> {
    let rest = line.trim_start();
    let rest = rest
        .strip_prefix('-')
        .or_else(|| rest.strip_prefix('*'))?
        .trim_start();
    let inner = rest.strip_prefix('[')?;
    let mark = inner.chars().next()?;
    let after = inner.strip_prefix(mark)?.strip_prefix(']')?;
    if mark != ' ' && mark != 'x' && mark != 'X' {
        return None;
    }
    Some(Task {
        done: mark == 'x' || mark == 'X',
        text: after.trim().to_string(),
    })
}

/// `(faites, total)` des tâches.
pub fn progress(tasks: &[Task]) -> (usize, usize) {
    (tasks.iter().filter(|task| task.done).count(), tasks.len())
}

/// Première tâche non faite (statut `/kiro`).
pub fn next_task(tasks: &[Task]) -> Option<&str> {
    tasks
        .iter()
        .find(|task| !task.done)
        .map(|task| task.text.as_str())
}

/// Spec lue sur disque (docs brutes + tâches parsées).
#[derive(Debug, Clone)]
pub struct Spec {
    pub name: String,
    pub requirements: Option<String>,
    pub design: Option<String>,
    pub tasks: Vec<Task>,
}

/// Charge une spec (`dir` = `.kiro/specs/<nom>`).
pub fn load(dir: &Path) -> Option<Spec> {
    let name = dir.file_name()?.to_string_lossy().into_owned();
    let requirements = crate::agents::read_non_empty(&dir.join("requirements.md"));
    let design = crate::agents::read_non_empty(&dir.join("design.md"));
    let tasks = crate::agents::read_non_empty(&dir.join("tasks.md"))
        .map(|md| parse_tasks(&md))
        .unwrap_or_default();
    if requirements.is_none() && design.is_none() && tasks.is_empty() {
        return None;
    }
    Some(Spec {
        name,
        requirements,
        design,
        tasks,
    })
}

/// Charge toutes les specs d'un dossier (triées par nom).
pub fn load_all(specs_dir: &Path) -> Vec<Spec> {
    let mut dirs = Vec::new();
    let Ok(entries) = std::fs::read_dir(specs_dir) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            dirs.push(entry.path());
        }
    }
    dirs.sort();
    dirs.into_iter().filter_map(|dir| load(&dir)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("kh-kiro-spec-{}-{name}", std::process::id()))
    }

    #[test]
    fn cases_cochees_et_progression() {
        let tasks = parse_tasks(
            "# Tasks\n- [ ] 1. Write code\n- [x] 2. Review it\n* [X] 3. Ship\n- nope\n- [?] 4. Unknown",
        );
        assert_eq!(tasks.len(), 3);
        assert!(!tasks[0].done);
        assert!(tasks[1].done);
        assert_eq!(tasks[0].text, "1. Write code");
        assert_eq!(progress(&tasks), (2, 3));
        assert_eq!(next_task(&tasks), Some("1. Write code"));
        assert_eq!(next_task(&tasks[1..]), None);
    }

    #[test]
    fn load_all_ne_garde_que_les_specs_valides() {
        let dir = tmp_dir("load");
        let specs = dir.join(".kiro/specs");
        std::fs::create_dir_all(specs.join("auth")).unwrap();
        std::fs::create_dir_all(specs.join("vide")).unwrap();
        std::fs::write(specs.join("auth/requirements.md"), "req").unwrap();
        std::fs::write(specs.join("auth/tasks.md"), "- [ ] code\n- [x] test").unwrap();
        let all = load_all(&specs);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "auth");
        assert_eq!(all[0].requirements.as_deref(), Some("req"));
        assert!(all[0].design.is_none());
        assert_eq!(progress(&all[0].tasks), (1, 2));
        assert!(load_all(&dir.join("absent")).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
