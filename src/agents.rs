//! Contexte système agents.md : chargement, annonce, compaction.
//!
//! Lu une seule fois, au premier envoi (jamais relu ensuite), injecté
//! comme message `system` en tête de l'historique de chaque chat (flag
//! `pinned_system`) et préfixé aux appels `/agent`. Exclu du résumé
//! `/compact` (méta) et de `--once` (brut).

use std::path::Path;

/// Candidats cherchés dans le cwd, dans l'ordre (`--agents-file` prime).
const CANDIDATES: [&str; 2] = ["AGENTS.md", "AGENT.md"];
/// Messages récents conservés tels quels après un compactage.
const COMPACT_KEPT: usize = 4;

/// Charge le contexte : `--agents-file` prime, sinon `./AGENTS.md` puis
/// `./AGENT.md` dans le cwd. `None` si absent, illisible ou vide.
pub fn load(explicit: Option<&str>) -> Option<String> {
    if let Some(path) = explicit {
        return read_non_empty(Path::new(path));
    }
    let cwd = std::env::current_dir().ok()?;
    load_from_dir(&cwd)
}

/// Variante testable : cherche les candidats dans `dir` (dans l'ordre).
pub fn load_from_dir(dir: &Path) -> Option<String> {
    CANDIDATES
        .iter()
        .find_map(|name| read_non_empty(&dir.join(name)))
}

/// Lit un fichier, `None` si illisible ou blanc (évite un system vide).
fn read_non_empty(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// `(lignes, ~tokens)` d'un contenu (tokens via [`crate::tokens::estimate`]).
pub fn stats(content: &str) -> (usize, usize) {
    (content.lines().count(), crate::tokens::estimate(content))
}

/// Annonce de chargement : `AGENTS.md loaded (N lines, ~M tokens)`.
pub fn loaded_message(content: &str) -> String {
    let (lines, toks) = stats(content);
    format!("AGENTS.md loaded ({lines} lines, ~{toks} tokens)")
}

/// Ligne `/usage` : chargé (~tokens, lignes) ou absent.
pub fn usage_line(agents_md: Option<&str>) -> String {
    match agents_md {
        Some(content) => {
            let (lines, toks) = stats(content);
            format!("agents.md: loaded (~{toks} tokens, {lines} lines)")
        }
        None => String::from("agents.md: missing"),
    }
}

/// Historique sans l'entrée pinnée (requête de résumé méta `/compact`).
pub fn without_pinned(history: &[(String, String)], pinned: bool) -> &[(String, String)] {
    if pinned {
        history.get(1..).unwrap_or(&[])
    } else {
        history
    }
}

/// Historique compacté : `[pinné?]` + `[résumé]` + 4 derniers (hors pinné).
pub fn compacted_history(
    history: &[(String, String)],
    summary: &str,
    pinned: bool,
) -> Vec<(String, String)> {
    let mut out = Vec::with_capacity(COMPACT_KEPT + 2);
    if pinned {
        if let Some(first) = history.first() {
            out.push(first.clone());
        }
    }
    out.push((
        String::from("system"),
        format!("Compacted summary: {summary}"),
    ));
    out.extend(
        without_pinned(history, pinned)
            .iter()
            .rev()
            .take(COMPACT_KEPT)
            .rev()
            .cloned(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Entrée d'historique courte (tests).
    fn msg(role: &str, text: &str) -> (String, String) {
        (role.to_string(), text.to_string())
    }

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("kh-agents-{}-{name}", std::process::id()))
    }

    #[test]
    fn compacted_keeps_pinned_head_summary_and_last_four() {
        let history = vec![
            msg("system", "AGENTS"),
            msg("user", "u1"),
            msg("assistant", "a1"),
            msg("user", "u2"),
            msg("assistant", "a2"),
            msg("user", "u3"),
            msg("assistant", "a3"),
        ];
        let out = compacted_history(&history, "tout va", true);
        assert_eq!(out.len(), 6);
        assert_eq!(out[0], msg("system", "AGENTS"));
        assert_eq!(out[1], msg("system", "Compacted summary: tout va"));
        assert_eq!(
            out[2..],
            [
                msg("user", "u2"),
                msg("assistant", "a2"),
                msg("user", "u3"),
                msg("assistant", "a3"),
            ]
        );
    }

    #[test]
    fn compacted_without_pin_is_summary_plus_last_four() {
        let history = vec![
            msg("user", "u1"),
            msg("assistant", "a1"),
            msg("user", "u2"),
            msg("assistant", "a2"),
            msg("user", "u3"),
            msg("assistant", "a3"),
        ];
        let out = compacted_history(&history, "tout va", false);
        assert_eq!(out.len(), 5);
        assert_eq!(out[0], msg("system", "Compacted summary: tout va"));
        assert_eq!(
            out[1..],
            [
                msg("user", "u2"),
                msg("assistant", "a2"),
                msg("user", "u3"),
                msg("assistant", "a3"),
            ]
        );
    }

    #[test]
    fn loader_prefers_agents_md_then_agent_md_then_none() {
        let dir = tmp_dir("order");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("AGENT.md"), "vieux").unwrap();
        std::fs::write(dir.join("AGENTS.md"), "canon").unwrap();
        assert_eq!(load_from_dir(&dir).as_deref(), Some("canon"));
        std::fs::remove_file(dir.join("AGENTS.md")).unwrap();
        assert_eq!(load_from_dir(&dir).as_deref(), Some("vieux"));
        std::fs::remove_file(dir.join("AGENT.md")).unwrap();
        assert_eq!(load_from_dir(&dir), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn loader_explicit_primes_and_blank_is_none() {
        let dir = tmp_dir("explicit");
        std::fs::create_dir_all(&dir).unwrap();
        let custom = dir.join("custom.md");
        std::fs::write(&custom, "sur mesure").unwrap();
        std::fs::write(dir.join("AGENTS.md"), "canon").unwrap();
        assert_eq!(load(custom.to_str()).as_deref(), Some("sur mesure"));
        assert_eq!(load(Some(dir.join("absent.md").to_str().unwrap())), None);
        std::fs::write(&custom, "  \n\t\n").unwrap();
        assert_eq!(load(custom.to_str()), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn messages_show_lines_and_tokens() {
        let text = "ligne un\nligne deux";
        let (lines, toks) = stats(text);
        assert_eq!(lines, 2);
        assert!(toks > 0);
        assert_eq!(
            loaded_message(text),
            format!("AGENTS.md loaded (2 lines, ~{toks} tokens)")
        );
        assert!(usage_line(Some(text)).starts_with("agents.md: loaded"));
        assert_eq!(usage_line(None), "agents.md: missing");
    }
}
