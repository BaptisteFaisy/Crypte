//! Intégration terminal, pensée pour Ghostty : fond de fenêtre (OSC 11,
//! padding compris), titre (OSC 2, pile de titres restaurée en sortie),
//! barre de progression native pendant l'attente (OSC 9;4, Ghostty seul)
//! et rendu synchronisé (mode 2026) pour des animations sans déchirure.
//!
//! Aucun son : ni BEL, ni notification (règle du dépôt).

use std::io::{self, Write};

use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use crossterm::QueueableCommand;
use ratatui::{DefaultTerminal, Frame};

/// Fond de la crypte (= `theme::VOID`), format xterm.
const OSC_BG: &str = "\x1b]11;rgb:0c/09/08\x1b\\";
/// Restaure le fond par défaut du terminal.
const OSC_BG_RESET: &str = "\x1b]111\x1b\\";
/// Empile / dépile le titre de la fenêtre (XTWINOPS 22/23).
const TITLE_PUSH: &str = "\x1b[22;0t";
const TITLE_POP: &str = "\x1b[23;0t";
/// Titre de fenêtre (icône fantôme Nerd Font, embarquée par Ghostty).
const OSC_TITLE: &str = "\x1b]2;\u{f02a0} Crypte \u{2014} kiro-horror\x1b\\";
/// Progression indéterminée / effacée (ConEmu OSC 9;4, géré par Ghostty).
const OSC_BUSY: &str = "\x1b]9;4;3\x1b\\";
const OSC_IDLE: &str = "\x1b]9;4;0\x1b\\";

/// Vrai si le processus tourne dans Ghostty.
pub fn is_ghostty() -> bool {
    std::env::var("TERM_PROGRAM").is_ok_and(|v| v.eq_ignore_ascii_case("ghostty"))
        || std::env::var("TERM").is_ok_and(|v| v == "xterm-ghostty")
}

/// Habille la fenêtre : fond crypte + titre (après `ratatui::init`).
pub fn enter() -> io::Result<()> {
    write_raw(&[OSC_BG, TITLE_PUSH, OSC_TITLE])
}

/// Rend la fenêtre telle qu'on l'a trouvée (avant `ratatui::restore`).
pub fn leave() -> io::Result<()> {
    write_raw(&[OSC_IDLE, OSC_BG_RESET, TITLE_POP])
}

/// Barre de progression native de Ghostty : active pendant une requête.
/// Ignoré hors Ghostty (ailleurs, OSC 9 peut être une notification).
pub fn progress(busy: bool) -> io::Result<()> {
    if !is_ghostty() {
        return Ok(());
    }
    write_raw(&[if busy { OSC_BUSY } else { OSC_IDLE }])
}

/// `terminal.draw` encadré par le mode 2026 : la frame s'affiche d'un bloc.
pub fn draw_synced(
    terminal: &mut DefaultTerminal,
    render: impl FnOnce(&mut Frame),
) -> io::Result<()> {
    io::stdout().queue(BeginSynchronizedUpdate)?;
    terminal.draw(render)?;
    io::stdout().queue(EndSynchronizedUpdate)?.flush()
}

fn write_raw(seqs: &[&str]) -> io::Result<()> {
    let mut out = io::stdout().lock();
    for seq in seqs {
        out.write_all(seq.as_bytes())?;
    }
    out.flush()
}
