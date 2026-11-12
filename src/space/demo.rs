//! Planche PNG hors terminal : `kiro-horror-tui --space-shot /tmp/x.png`
//! (pattern de `src/haunt/demo.rs`). Permet de voir le monde 3D sans TUI.

use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};

use super::render::Space;
use super::scan::{DirNode, FileNode, ProjectTree};
use super::{
    AgentKind, AgentRef, KiroState, RewindState, SceneEvent, SceneState, Step, SubState, Target,
};

/// Cadre de référence du prototype, en demi-résolution.
const W: u32 = 960;
const H: u32 = 540;
/// Rectangle du panneau de chat (proportions DESIGN §3).
const PANEL: [i64; 4] = [32, 55, 347, 457];

/// Écrit une planche de la scène 3D dans `path` (PNG 960×540).
pub fn write_shot(path: &str) -> std::io::Result<()> {
    let tree = demo_tree();
    let mut state = demo_state(&tree);
    let mut cv = RgbaImage::from_pixel(W, H, Rgba([0, 0, 0, 255]));
    let mut space = Space::new_many(&[("demo".to_string(), tree, true)]);
    for i in 0..90 {
        space.frame(&mut cv, &state, i as f64 / 30.0, 1.0 / 30.0, Some(PANEL));
    }
    state.step = Some(Step::Answer);
    space.frame(&mut cv, &state, 3.0, 1.0 / 30.0, Some(PANEL));
    cv.save(path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(())
}

/// Arbre de démonstration : 4 dossiers, 24 fichiers.
fn demo_tree() -> ProjectTree {
    ProjectTree {
        root: PathBuf::from("/demo"),
        dirs: (0..4)
            .map(|i| DirNode {
                path: PathBuf::from(format!("src/mod{i}")),
                parent: (i > 0).then_some(0),
            })
            .collect(),
        files: (0..24)
            .map(|i| FileNode {
                path: PathBuf::from(format!("src/mod{}/f{i:02}.rs", i % 4)),
                dir: i % 4,
                lines: 40 + ((i * 137) % 900) as u32,
            })
            .collect(),
    }
}

/// État de démonstration : une écriture en cours, une lecture, un sous-agent
/// poussé, Kiro en critique, un rewind qui effondre une pile.
fn demo_state(tree: &ProjectTree) -> SceneState {
    let mut s = SceneState::new();
    s.open_project(Path::new("/demo"), tree.clone());
    let hot = PathBuf::from("src/mod1/f05.rs");
    let read = PathBuf::from("src/mod0/f04.rs");
    s.apply(&SceneEvent::StepStart { step: Step::Act }, 0.0);
    s.apply(
        &SceneEvent::FileRead {
            path: read,
            agent: AgentRef::Main,
        },
        2.4,
    );
    s.apply(
        &SceneEvent::FileWrite {
            path: hot.clone(),
            agent: AgentRef::Main,
            added: 300,
            removed: 0,
        },
        2.6,
    );
    s.apply(
        &SceneEvent::AgentSpawn {
            id: 1,
            kind: AgentKind::Sub,
            target: Target::Path(PathBuf::from("src/mod2/f10.rs")),
        },
        1.0,
    );
    s.apply(&SceneEvent::AgentDone { id: 1, ok: true }, 2.2);
    s.kiro = Some(KiroState {
        file: Some(hot.clone()),
        started_at: 1.5,
        done_at: Some(2.5),
        pushed: true,
    });
    s.rewind = Some(RewindState {
        at: 2.0,
        files: vec![hot.clone()],
        from: vec![s.files.get(&hot).map(|f| f.slabs).unwrap_or(1) + 4],
    });
    s.subs.insert(
        1,
        SubState {
            kind: AgentKind::Sub,
            target: Target::Path(PathBuf::from("src/mod2/f10.rs")),
            spawned_at: 1.0,
            done_at: Some(2.2),
            pushed: true,
        },
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_planche_est_un_png() {
        let path =
            std::env::temp_dir().join(format!("crypt-space-shot-{}.png", std::process::id()));
        write_shot(path.to_str().expect("utf8")).expect("shot");
        let bytes = std::fs::read(&path).expect("lu");
        assert_eq!(&bytes[0..4], b"\x89PNG");
        assert!(bytes.len() > 5000);
        let _ = std::fs::remove_file(&path);
    }
}
