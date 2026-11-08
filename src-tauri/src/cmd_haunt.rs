//! Commandes §9 : `haunt_start|stop|input` (jeu, 240 Hz simu).
//!
//! `haunt_start` rend la première frame ; la boucle de l'intégrateur
//! pompe (`pump(100 ms)`, jeu ouvert seulement) et émet `haunt_frame`
//! à 10 Hz. Touches : comme le TUI (`a`/`d`/`w`/`s`, espace, `p`,
//! Entrée, flèches) ; `quit` = fermer (comme Échap/F9).

use std::sync::Mutex;

use tauri::State;

use crate::haunt::input::{Act, KeyKind};
use crate::haunt::FrameData;
use crate::state::{lock, ToolsBackend};
use crate::views::{DotView, FoeView, HauntFrameView, HauntHudView, ShotView};

/// `haunt_start` : ouvre le jeu (reprise, jamais reset) + 1re frame.
/// Les relâchements sont actifs (comme le protocole Kitty du TUI).
#[tauri::command]
pub fn haunt_start(backend: State<Mutex<ToolsBackend>>) -> Result<HauntFrameView, String> {
    let mut backend = lock(&backend)?;
    backend.haunt.set_releases(true);
    backend.haunt_open = true;
    Ok(frame_view(&backend.haunt.frame()))
}

/// `haunt_stop` : pause + fermeture (les touches s'oublient).
#[tauri::command]
pub fn haunt_stop(backend: State<Mutex<ToolsBackend>>) -> Result<(), String> {
    let mut backend = lock(&backend)?;
    backend.haunt.pause();
    backend.haunt_open = false;
    Ok(())
}

/// `haunt_input` : touche (`left|right|up|down|fire|quit` + alias,
/// `pressed` = appui/relâchement). `quit` = fermer comme Échap.
#[tauri::command]
pub fn haunt_input(
    backend: State<Mutex<ToolsBackend>>,
    key: String,
    pressed: bool,
) -> Result<(), String> {
    let mut backend = lock(&backend)?;
    if key == "quit" && pressed {
        backend.haunt.pause();
        backend.haunt_open = false;
        return Ok(());
    }
    if let Some(act) = act_of(&key) {
        let kind = if pressed {
            KeyKind::Press
        } else {
            KeyKind::Release
        };
        backend.haunt.feed(act, kind);
    }
    Ok(())
}

/// Touche contrat → action de jeu (noms + alias TUI `a`/`d`/`w`/`s`).
fn act_of(key: &str) -> Option<Act> {
    Some(match key {
        "left" | "a" => Act::Left,
        "right" | "d" => Act::Right,
        "up" | "w" => Act::Up,
        "down" | "s" => Act::Down,
        "fire" | "space" | " " => Act::Fire,
        "start" | "enter" => Act::Start,
        "pause" | "p" => Act::Pause,
        _ => return None,
    })
}

/// Frame contrat depuis la frame simu (boss déjà inclus en `reaper`).
pub fn frame_view(frame: &FrameData) -> HauntFrameView {
    HauntFrameView {
        running: frame.running,
        player: DotView {
            x: frame.player.x,
            y: frame.player.y,
        },
        foes: frame
            .foes
            .iter()
            .map(|(at, kind)| FoeView {
                x: at.x,
                y: at.y,
                kind: kind.clone(),
            })
            .collect(),
        shots: frame
            .shots
            .iter()
            .map(|(at, foe)| ShotView {
                x: at.x,
                y: at.y,
                foe: *foe,
            })
            .collect(),
        hud: HauntHudView {
            level: frame.hud.level,
            score: frame.hud.score,
            lives: frame.hud.lives,
            fear: frame.hud.fear,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touches_mappees_et_inconnues_ignorees() {
        assert_eq!(act_of("left"), Some(Act::Left));
        assert_eq!(act_of("a"), Some(Act::Left));
        assert_eq!(act_of("fire"), Some(Act::Fire));
        assert_eq!(act_of("space"), Some(Act::Fire));
        assert_eq!(act_of("start"), Some(Act::Start));
        assert_eq!(act_of("pause"), Some(Act::Pause));
        assert_eq!(act_of("nope"), None);
    }

    #[test]
    fn frame_titre_puis_jeu() {
        let mut game = crate::haunt::Haunt::new();
        let title = frame_view(&game.frame());
        assert!(!title.running);
        assert_eq!(title.hud.lives, 3);
        game.feed(Act::Start, KeyKind::Press);
        game.pump(std::time::Duration::from_millis(100));
        let play = frame_view(&game.frame());
        assert!(play.running);
        assert!(play.player.x > 0.0);
    }
}
