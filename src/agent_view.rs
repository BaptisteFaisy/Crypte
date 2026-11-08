//! Pastilles sous-agents inline : marqueur scintillant, sélection ↑↓,
//! échange déplié (Entrée), agents de workflow nichés sous leur spec.
//!
//! Les annonces (`✦ agent#N …`, `  ✦ agent#N …` sous `◈ spec …`) sont
//! des messages système ; ce module les décore au rendu (post-traitement
//! des lignes visibles, hors cache de wrap) et fournit les libellés.

use std::borrow::Cow;

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::sessions::{AgentStatus, SubAgent};
use crate::theme;

/// Marqueur d'un widget agent (annonce de lancement).
pub const WIDGET: &str = "✦";
/// Marqueur d'un parent de workflow `/spec`.
pub const SPEC_PARENT: &str = "◈";
/// Retrait du bloc d'échange déplié (doublé si agent de workflow).
const INDENT: &str = "  ";

/// Contexte agents du rendu chat (construit par `draw_main`).
#[derive(Clone, Copy, Debug)]
pub struct AgentCtx<'a> {
    pub list: &'a [SubAgent],
    pub cursor: Option<usize>,
    pub expanded: &'a [usize],
}

impl<'a> AgentCtx<'a> {
    /// Contexte vide (tests).
    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            list: &[],
            cursor: None,
            expanded: &[],
        }
    }
}

/// Annonce `/agent` : `✦ agent#N started[ → Chat k] — prompt`.
pub fn spawn_line(id: usize, target: Option<usize>, prompt: &str) -> String {
    let short = crate::provider::truncate(prompt, 60);
    match target {
        None => format!("{WIDGET} agent#{id} started — {short}"),
        Some(n) => format!("{WIDGET} agent#{id} started → Chat {n} — {short}"),
    }
}

/// Parent `/spec run` : `◈ spec <nom> — task i/n: <tâche>`.
pub fn spec_parent_line(name: &str, label: &str, task: &str) -> String {
    let short = crate::provider::truncate(task, 40);
    format!("{SPEC_PARENT} spec {name} — task {label}: {short}")
}

/// Enfant `/spec run` : `  ✦ agent#N — spec <nom>` (ancre du widget).
pub fn spec_child_line(id: usize, name: &str) -> String {
    format!("  {WIDGET} agent#{id} — spec {name}")
}

/// Id du widget si la ligne (texte brut) est une annonce `✦ agent#N`.
pub fn widget_id(plain: &str) -> Option<usize> {
    let rest = plain.trim_start().strip_prefix(WIDGET)?.trim_start();
    let num = rest.strip_prefix("agent#")?;
    let digits: String = num.chars().take_while(|c| c.is_ascii_digit()).collect();
    match digits.parse::<usize>().ok() {
        Some(id) if id >= 1 => Some(id),
        _ => None,
    }
}

/// Style du marqueur : scintille (tick) si en cours, fixe sinon.
pub fn marker_style(status: AgentStatus, tick: u64) -> Style {
    match status {
        AgentStatus::Running => {
            let lit = tick / 4 % 2 == 0;
            Style::new()
                .fg(if lit { theme::ECTO } else { theme::CANDLE })
                .add_modifier(Modifier::BOLD)
        }
        AgentStatus::Done => Style::new().fg(theme::ECTO),
        AgentStatus::Failed => Style::new().fg(theme::BLOOD),
    }
}

/// Décore les lignes visibles : marqueur du statut + surlignage du
/// curseur + blocs d'échange dépliés (wrappés à `inner_w`).
pub fn decorate(visible: &mut Vec<Line<'static>>, ctx: &AgentCtx, tick: u64, inner_w: usize) {
    if ctx.list.is_empty() {
        return;
    }
    for line in visible.iter_mut() {
        let plain = crate::clip::plain_text(line);
        let Some(id) = widget_id(&plain) else {
            continue;
        };
        let Some(agent) = ctx.list.iter().find(|a| a.id == id) else {
            continue;
        };
        restyle_marker(line, marker_style(agent.status, tick));
        if ctx.cursor == Some(id) {
            line.style = line.style.bg(theme::lerp(theme::VOID, theme::DRIED, 0.55));
        }
    }
    let anchors: Vec<(usize, &SubAgent)> = visible
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            let id = widget_id(&crate::clip::plain_text(line))?;
            if !ctx.expanded.contains(&id) {
                return None;
            }
            ctx.list.iter().find(|a| a.id == id).map(|a| (i, a))
        })
        .collect();
    for (i, agent) in anchors.into_iter().rev() {
        visible.splice(i + 1..i + 1, transcript_block(agent, inner_w));
    }
}

/// Bloc d'échange déplié : transcrit wrappé, indenté (×2 si workflow).
fn transcript_block(agent: &SubAgent, inner_w: usize) -> Vec<Line<'static>> {
    let indent = if agent.spec.is_some() { "    " } else { INDENT };
    let body_w = inner_w.saturating_sub(indent.chars().count()).max(1);
    let style = Style::new().fg(theme::SAND);
    let mut out = Vec::new();
    for raw in agent.transcript().lines() {
        for row in crate::chat::wrap_text(raw, body_w) {
            out.push(Line::from(vec![
                Span::styled(indent, style),
                Span::styled(row, style),
            ]));
        }
    }
    out
}

/// Recolore le seul glyphe `✦` de la ligne (découpe son span en 3).
fn restyle_marker(line: &mut Line<'static>, style: Style) {
    let found = line
        .spans
        .iter()
        .enumerate()
        .find_map(|(n, s)| s.content.find(WIDGET).map(|at| (n, at)));
    let Some((n, at)) = found else {
        return;
    };
    let base = line.spans[n].style;
    let text = std::mem::replace(&mut line.spans[n].content, Cow::Borrowed("")).into_owned();
    let (before, rest) = text.split_at(at);
    let after = &rest[WIDGET.len()..];
    let mut all: Vec<Span<'static>> = std::mem::take(&mut line.spans);
    let mut out = Vec::with_capacity(all.len() + 2);
    out.extend(all.drain(..n));
    if !before.is_empty() {
        out.push(Span::styled(before.to_string(), base));
    }
    out.push(Span::styled(WIDGET, style));
    if !after.is_empty() {
        out.push(Span::styled(after.to_string(), base));
    }
    out.extend(all.drain(1..));
    line.spans = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Agent de test (échange user seul, statut `Running`).
    fn agent(id: usize, prompt: &str) -> SubAgent {
        SubAgent::new(id, prompt, vec![(String::from("user"), String::from("u"))])
    }

    /// Ligne visible façon `history_lines` (gouttière + corps).
    fn body(text: &str) -> Line<'static> {
        Line::from(vec![
            Span::raw(crate::chat::GUTTER),
            Span::raw(text.to_string()),
        ])
    }

    #[test]
    fn widget_id_ne_matche_que_les_annonces() {
        assert_eq!(widget_id("✦ agent#3 started"), Some(3));
        assert_eq!(widget_id("  ✦ agent#12 — spec foo"), Some(12));
        assert_eq!(widget_id("[agent#1] réponse"), None);
        assert_eq!(widget_id("◈ spec foo — task 1/2"), None);
        assert_eq!(widget_id("✦ agent#0 x"), None);
        assert_eq!(widget_id("✦ coucou"), None);
        assert_eq!(widget_id("simple texte"), None);
    }

    #[test]
    fn libelles_portent_les_marqueurs() {
        assert!(spawn_line(1, None, "fais ça").starts_with("✦ agent#1 started"));
        assert!(spawn_line(2, Some(3), "x").contains("→ Chat 3"));
        assert!(spec_parent_line("foo", "1/2", "tâche").starts_with("◈ spec foo"));
        assert_eq!(spec_child_line(7, "foo"), "  ✦ agent#7 — spec foo");
        assert_eq!(widget_id(&spawn_line(1, None, "x")), Some(1));
        assert_eq!(widget_id(&spec_child_line(7, "foo")), Some(7));
    }

    #[test]
    fn marqueur_scintille_en_cours_fixe_sinon() {
        let run0 = marker_style(AgentStatus::Running, 0);
        let run4 = marker_style(AgentStatus::Running, 4);
        assert_ne!(run0, run4, "scintille avec le tick");
        assert_eq!(
            marker_style(AgentStatus::Done, 0),
            marker_style(AgentStatus::Done, 4),
            "fixe une fois fini"
        );
        assert_eq!(marker_style(AgentStatus::Failed, 0).fg, Some(theme::BLOOD));
    }

    #[test]
    fn decore_marqueur_curseur_et_deplie() {
        let mut one = agent(1, "loin");
        one.status = AgentStatus::Done;
        let two = agent(2, "ici");
        let list = [one, two];
        let expanded = [1];
        let ctx = AgentCtx {
            list: &list,
            cursor: Some(2),
            expanded: &expanded,
        };
        let mut visible = vec![
            body("✦ agent#1 started — loin"),
            body("✦ agent#2 started — ici"),
        ];
        decorate(&mut visible, &ctx, 0, 40);
        // Marqueur : span `✦` recoloré (3e span de la ligne 0).
        assert_eq!(visible[0].spans[1].content.as_ref(), WIDGET);
        assert_eq!(visible[0].spans[1].style.fg, Some(theme::ECTO));
        // Curseur : fond sur la ligne du widget #2 (décalée par le déplié).
        let head = visible
            .iter()
            .position(|l| crate::clip::plain_text(l).contains("agent#2"))
            .unwrap();
        assert!(visible[head].style.bg.is_some(), "curseur surligné");
        // Déplié : transcrit de #1 inséré après son annonce.
        let plain: Vec<String> = visible.iter().map(crate::clip::plain_text).collect();
        assert!(plain[1].contains("#1 done"), "{plain:?}");
        assert!(plain.iter().any(|l| l.contains("you: u")), "{plain:?}");
    }

    #[test]
    fn deplie_workflow_double_indent() {
        let mut a = agent(1, "spec");
        a.spec = Some(crate::sessions::SpecTag {
            name: String::from("foo"),
            index: 0,
            total: 2,
        });
        let block = transcript_block(&a, 40);
        assert!(block[0].spans[0].content.as_ref() == "    ", "{block:?}");
        let plain: Vec<String> = block.iter().map(crate::clip::plain_text).collect();
        assert!(
            plain.iter().any(|l| l.contains("spec foo · task 1/2")),
            "{plain:?}"
        );
    }
}
