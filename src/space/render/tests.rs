use super::*;
use crate::space::paint::envelope;
use crate::space::scan::{DirNode, FileNode};

fn tree() -> ProjectTree {
    ProjectTree {
        root: PathBuf::from("/p"),
        dirs: vec![
            DirNode {
                path: PathBuf::from("src"),
                parent: None,
            },
            DirNode {
                path: PathBuf::from("src/space"),
                parent: Some(0),
            },
        ],
        files: vec![
            FileNode {
                path: PathBuf::from("src/main.rs"),
                dir: 0,
                lines: 300,
            },
            FileNode {
                path: PathBuf::from("src/space/mod.rs"),
                dir: 1,
                lines: 1200,
            },
        ],
    }
}

fn state(read: bool, write: bool) -> SceneState {
    let mut s = SceneState::new();
    s.open_project(std::path::Path::new("/p"), tree());
    let main = PathBuf::from("src/main.rs");
    if read {
        s.files.get_mut(&main).unwrap().read_at = Some(0.1);
        s.main_file = Some(main.clone());
    }
    if write {
        s.files.get_mut(&main).unwrap().write_at = Some(0.1);
        s.files.get_mut(&main).unwrap().changed = true;
        s.main_wrote = true;
        s.main_file = Some(main);
    }
    s
}

fn render(state: &SceneState, now: f64) -> RgbaImage {
    let mut cv = RgbaImage::from_pixel(320, 180, Rgba([0, 0, 0, 255]));
    let mut space = Space::new(&tree());
    for i in 0..40 {
        space.frame(
            &mut cv,
            state,
            now - (40 - i) as f64 / 30.0,
            1.0 / 30.0,
            None,
        );
    }
    cv
}

fn count(cv: &RgbaImage, f: impl Fn(&Rgba<u8>) -> bool) -> usize {
    cv.pixels().filter(|p| f(p)).count()
}

#[test]
fn le_projet_actif_est_dessiné() {
    let cv = render(&state(false, false), 1.0);
    assert!(count(&cv, |p| p[0] > 20) > 150, "sol et dalles visibles");
}

#[test]
fn une_écriture_allume_l_orange() {
    let cv = render(&state(false, true), 0.3);
    assert!(
        count(&cv, |p| p[0] > 120 && p[1] < 110 && p[2] < 80) > 40,
        "orange présent"
    );
}

#[test]
fn une_lecture_allume_le_blanc_sans_orange() {
    let cv = render(&state(true, false), 0.3);
    assert!(count(&cv, |p| p[0] > 150 && p[1] > 150 && p[2] > 150) > 40);
}

#[test]
fn le_rendu_est_déterministe() {
    let a = render(&state(true, true), 0.4);
    let b = render(&state(true, true), 0.4);
    assert_eq!(a.as_raw(), b.as_raw());
}

#[test]
fn le_voile_du_panneau_assombrit() {
    let mut cv = RgbaImage::from_pixel(200, 120, Rgba([0, 0, 0, 255]));
    let mut space = Space::new(&tree());
    let s = state(true, false);
    space.frame(&mut cv, &s, 1.0, 1.0 / 30.0, None);
    let sans = cv.get_pixel(30, 60)[0];
    space.frame(&mut cv, &s, 1.0, 1.0 / 30.0, Some([0, 0, 80, 120]));
    assert!(cv.get_pixel(30, 60)[0] <= sans);
}

#[test]
fn intensités_et_enveloppes() {
    assert_eq!(intensities(None, 1.0), (0.0, 0.0, 0.0));
    let mut f = FileState::default();
    f.read_at = Some(0.0);
    f.changed = true;
    let (r, _a, c) = intensities(Some(&f), 0.15);
    assert!(r > 0.4 && c == 1.0);
    assert_eq!(envelope(Some(0.0), 100.0), 0.0);
    assert_eq!(envelope(None, 1.0), 0.0);
}

/// Planche de contrôle visuelle (ignorée en CI) :
/// `cargo test --release -- --ignored space_preview --nocapture`
/// puis ouvrir `/tmp/space-preview.png`.
#[test]
#[ignore]
fn space_preview() {
    let tree = ProjectTree {
        root: PathBuf::from("/p"),
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
                lines: (40 + (i * 137) % 900) as u32,
            })
            .collect(),
    };
    let mut s = SceneState::new();
    s.open_project(std::path::Path::new("/p"), tree.clone());
    let hot = PathBuf::from("src/mod1/f05.rs");
    s.files.get_mut(&hot).unwrap().write_at = Some(3.9);
    s.files.get_mut(&hot).unwrap().changed = true;
    s.files.get_mut(&hot).unwrap().slabs = 8;
    s.files
        .get_mut(&PathBuf::from("src/mod0/f04.rs"))
        .unwrap()
        .read_at = Some(3.8);
    s.main_file = Some(hot.clone());
    s.main_wrote = true;
    s.step = Some(super::super::Step::Act);
    s.subs.insert(
        1,
        super::super::SubState {
            kind: super::super::AgentKind::Sub,
            target: super::super::Target::Path(PathBuf::from("src/mod2/f10.rs")),
            spawned_at: 1.0,
            done_at: Some(3.0),
            pushed: true,
        },
    );
    s.kiro = Some(super::super::KiroState {
        file: Some(hot.clone()),
        started_at: 2.0,
        done_at: Some(4.0),
        pushed: true,
    });
    s.rewind = Some(super::super::RewindState {
        at: 3.5,
        files: vec![hot],
        from: vec![8],
    });
    let mut cv = RgbaImage::from_pixel(960, 540, Rgba([0, 0, 0, 255]));
    let mut space = Space::new(&tree);
    for i in 0..150 {
        space.frame(
            &mut cv,
            &s,
            i as f64 / 30.0,
            1.0 / 30.0,
            Some([32, 55, 347, 457]),
        );
    }
    cv.save("/tmp/space-preview.png").expect("png");
    println!("/tmp/space-preview.png écrit (960×540)");
}

#[test]
fn brouillard_selon_l_activité() {
    assert_eq!(fog_far(&SceneState::new(), 100.0), FOG_REST);
    let mut s = SceneState::new();
    s.hub = true;
    assert_eq!(fog_far(&s, 100.0), FOG_HUB);
    let s2 = state(true, false);
    assert_eq!(fog_far(&s2, 100.0), FOG_WORK);
    assert!(fog_far(&SceneState::new(), 4000.0) > FOG_REST);
}
