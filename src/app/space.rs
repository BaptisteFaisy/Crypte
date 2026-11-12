//! Fond 3D « project space » : bascule F8, vue de tous les projets F7,
//! pompe à 30 Hz et séquence Kitty écrite après les cellules.
//!
//! Coupé (défaut), tout ceci ne coûte rien : `SceneLog::disabled()` et
//! `space_view = None`. Voir `crate::space::view` pour le protocole Kitty.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::Frame;

use super::App;
use crate::space::scene::SceneLog;
use crate::space::view::SpaceView;
use crate::space::{scan, SceneEvent};

/// Cadence du fond : 30 images par seconde.
const FRAME: Duration = Duration::from_millis(33);
/// Nombre maximal de quartiers dessinés (le projet actif compris).
const MAX_PROJECTS: usize = 6;

impl App {
    /// F8 : active ou coupe le fond 3D.
    pub(crate) fn toggle_space(&mut self) {
        if self.space_view.is_some() {
            self.space_view = None;
            self.scene = SceneLog::disabled();
            self.space_watch = None;
            self.space_delete = true;
            return;
        }
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let tree = scan::scan_tree(&cwd).unwrap_or_default();
        let entries = districts(&cwd, tree.clone());
        let (log, rx) = SceneLog::channel(256);
        let mut view = SpaceView::new(rx, &entries);
        view.state
            .apply(&SceneEvent::ProjectOpen { path: cwd, tree }, 0.0);
        self.scene = log;
        self.space_view = Some(view);
        let mut watch = crate::space::watch::Watcher::new();
        watch.seed(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        self.space_watch = Some(watch);
    }

    /// F7 : la caméra s'éloigne sur tous les projets (ou revient).
    pub(crate) fn toggle_space_hub(&mut self) {
        let open = self.space_view.as_ref().is_some_and(|v| !v.state.hub);
        if open {
            self.scene.emit(SceneEvent::HubOpen);
        } else {
            self.scene.emit(SceneEvent::HubClose);
        }
    }

    /// Pompe le fond à 30 Hz ; vrai = il faut redessiner.
    pub(crate) fn pump_space(&mut self, last: &mut Instant) -> bool {
        if self.space_view.is_none() || last.elapsed() < FRAME {
            return false;
        }
        *last = Instant::now();
        if let Some(v) = self.space_view.as_mut() {
            v.pump();
        }
        true
    }

    /// Émet l'étape Act puis les lectures/écritures devinées dans une
    /// commande shell (il n'existe pas d'outil fichier typé dans ce dépôt).
    pub(crate) fn space_tool_events(&mut self, call: &crate::tools::ToolCall) {
        use crate::space::scene::{read_paths_from_shell, shell_command, write_paths_from_shell};
        use crate::space::{AgentRef, SceneEvent, Step};
        if call.name != "shell" {
            return;
        }
        let cmd = shell_command(&call.args);
        self.scene.emit(SceneEvent::StepStart { step: Step::Act });
        for p in read_paths_from_shell(&cmd) {
            self.scene.emit(SceneEvent::FileRead {
                path: p,
                agent: AgentRef::Main,
            });
        }
        for p in write_paths_from_shell(&cmd) {
            self.scene.emit(SceneEvent::FileWrite {
                path: p,
                agent: AgentRef::Main,
                added: 0,
                removed: 0,
            });
        }
    }

    /// Diff du disque (écritures hors shell) ; `None` quand la 3D est coupée.
    pub(crate) fn poll_space_watch(&mut self) {
        let Some(w) = self.space_watch.as_mut() else {
            return;
        };
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        for ch in w.poll(&cwd) {
            use crate::space::watch::Change;
            match ch {
                Change::Written {
                    path,
                    added,
                    removed,
                } => self.scene.emit(crate::space::SceneEvent::FileWrite {
                    path,
                    agent: crate::space::AgentRef::Main,
                    added,
                    removed,
                }),
                Change::Created { path, .. } => {
                    self.scene.emit(crate::space::SceneEvent::FileCreate {
                        path,
                        agent: crate::space::AgentRef::Main,
                    });
                }
                Change::Removed { .. } => {} // pas de visuel dédié pour l'instant
            }
        }
    }

    /// Séquence Kitty du fond, à écrire après les cellules ; mémorise au
    /// passage le rectangle du panneau (voile cuit dans l'image).
    pub(crate) fn space_sequence(&mut self, frame: &mut Frame) -> Option<String> {
        let view = self.space_view.as_mut()?;
        let area = frame.area();
        let cell = self.bank.kitty_cell()?;
        view.panel = panel_px(area, cell);
        view.sequence(area.width, area.height, cell)
    }

    /// Séquence d'effacement en attente (juste après un F8 vers l'arrêt).
    pub(crate) fn take_space_delete(&mut self) -> Option<String> {
        if self.space_delete {
            self.space_delete = false;
            Some(SpaceView::delete())
        } else {
            None
        }
    }
}

/// Dossiers de travail où chercher les autres projets (`KH_SPACE_WORKSPACES`,
/// séparés par `:`, sinon `~/code`).
fn workspaces() -> Vec<PathBuf> {
    if let Ok(v) = std::env::var("KH_SPACE_WORKSPACES") {
        return v
            .split(':')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();
    }
    std::env::var("HOME")
        .ok()
        .map(|h| vec![PathBuf::from(h).join("code")])
        .unwrap_or_default()
}

/// Le projet actif + jusqu'à [`MAX_PROJECTS`] quartiers voisins.
fn districts(cwd: &PathBuf, tree: scan::ProjectTree) -> Vec<(String, scan::ProjectTree, bool)> {
    let mut out = vec![(name_of(cwd), tree, true)];
    let ws = workspaces();
    for p in scan::list_projects(&ws, cwd) {
        if out.len() >= MAX_PROJECTS {
            break;
        }
        if let Ok(t) = scan::scan_tree(&p) {
            out.push((name_of(&p), t, false));
        }
    }
    out
}

/// Nom d'un projet (dernier composant du chemin).
fn name_of(p: &PathBuf) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string_lossy().to_string())
}

/// Rectangle du panneau de chat, en cellules (proportions du design §3 :
/// x 3,3 %, y 10,2 %, 36,1 % × 84,7 % du cadre de référence).
pub(crate) fn panel_cells(area: Rect) -> Rect {
    Rect {
        x: area.x + area.width * 33 / 1000,
        y: area.y + area.height * 102 / 1000,
        width: (area.width * 361 / 1000).min(area.width),
        height: (area.height * 847 / 1000).min(area.height),
    }
}

/// Le même rectangle, en pixels écran.
fn panel_px(area: Rect, cell: (u16, u16)) -> Option<[i64; 4]> {
    let r = panel_cells(area);
    Some([
        i64::from(r.x) * i64::from(cell.0),
        i64::from(r.y) * i64::from(cell.1),
        i64::from(r.width) * i64::from(cell.0),
        i64::from(r.height) * i64::from(cell.1),
    ])
}

/// Rend les fonds de cellules transparents dans le panneau : sans cela le
/// fond de cellule (opaque) masquerait l'image Kitty placée dessous.
pub(crate) fn clear_panel_bg(buf: &mut Buffer, rect: Rect) {
    for y in rect.y..rect.y.saturating_add(rect.height) {
        for x in rect.x..rect.x.saturating_add(rect.width) {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_bg(Color::Reset);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panneau_aux_proportions_du_design() {
        let r = panel_cells(Rect::new(0, 0, 144, 45));
        assert!((r.x as i64 - 4).abs() <= 1);
        assert!((r.y as i64 - 4).abs() <= 1);
        assert!((r.width as i64 - 52).abs() <= 1);
        assert!((r.height as i64 - 38).abs() <= 1);
    }

    #[test]
    fn panneau_tient_dans_le_cadre() {
        let r = panel_cells(Rect::new(0, 0, 10, 5));
        assert!(r.x + r.width <= 10);
        assert!(r.y + r.height <= 5);
    }

    #[test]
    fn le_voile_efface_les_fonds_du_panneau() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        for y in 0..4 {
            for x in 0..8 {
                buf[(x, y)].set_bg(Color::Red);
            }
        }
        clear_panel_bg(&mut buf, Rect::new(2, 1, 4, 2));
        assert_eq!(buf[(2, 1)].bg, Color::Reset);
        assert_eq!(buf[(0, 0)].bg, Color::Red);
        assert_eq!(buf[(7, 3)].bg, Color::Red);
    }
}
