//! Tests phantom Tauri (remplacent `phantom/tests.rs` du TUI, couplé
//! à ratatui/gfx) : errance, coups/K.O., effroi, critiques, déterminisme.

use super::*;

#[test]
fn tick_fait_apparaitre_et_errer_dans_l_arene() {
    let mut ghost = Phantom::new(7);
    assert_eq!(ghost.tick_count(), 0);
    ghost.tick();
    assert_eq!(ghost.tick_count(), 1);
    let (x0, y0) = ghost.pos();
    for _ in 0..200 {
        ghost.tick();
    }
    let (x1, y1) = ghost.pos();
    assert!((x0 - x1).abs() + (y0 - y1).abs() > 0.5, "Kiro erre");
    assert!((0.0..=100.0).contains(&x1));
    assert!((0.0..=30.0).contains(&y1));
    assert_eq!(ghost.mood(), Mood::Roam);
}

#[test]
fn trois_coups_rapproches_font_ko_puis_releve() {
    let mut ghost = Phantom::new(7);
    ghost.tick();
    let first = ghost.smack_center().expect("réplique");
    assert!(!first.is_empty());
    assert!(matches!(ghost.mood(), Mood::Hurt(_)));
    ghost.smack_center();
    let ko = ghost.smack_center().expect("réplique K.O.");
    assert!(!ko.is_empty());
    assert!(matches!(ghost.mood(), Mood::Ko(_)));
    assert_eq!(ghost.creature().slug, "kiro-ko");
    for _ in 0..50 {
        ghost.tick();
    }
    assert_eq!(ghost.mood(), Mood::Roam);
}

#[test]
fn effroi_f5_et_bulle_critique() {
    let mut ghost = Phantom::new(7);
    ghost.tick();
    ghost.scare();
    assert!(matches!(ghost.mood(), Mood::Scared(_)));
    assert_eq!(ghost.creature().slug, "kiro-peur");
    assert!(ghost.wants_critique());
    ghost.begin_critique();
    assert!(!ghost.wants_critique());
    ghost.critique_done(Some(String::from("Vague. Very vague.")), 42);
    assert_eq!(ghost.stats(), (1, 42));
    let said = ghost.speech().expect("bulle").text.clone();
    assert!(said.contains("Vague"));
    ghost.hush();
    assert!(ghost.speech().is_none());
    ghost.critique_failed();
    assert!(ghost.speech().is_some());
}

#[test]
fn eteint_ni_tick_ni_coup() {
    let mut ghost = Phantom::new(7);
    ghost.set_on(false);
    assert!(!ghost.on());
    ghost.tick();
    assert_eq!(ghost.pos(), (0.0, 0.0));
    assert_eq!(ghost.smack_center(), None);
    ghost.scare();
    assert_eq!(ghost.mood(), Mood::Roam);
}

#[test]
fn deterministe_a_graine_donnee() {
    let run = || {
        let mut ghost = Phantom::new(1234);
        for _ in 0..60 {
            ghost.tick();
        }
        ghost.smack_center();
        for _ in 0..20 {
            ghost.tick();
        }
        let (x, y) = ghost.pos();
        (ghost.tick_count(), (x * 100.0) as i32, (y * 100.0) as i32)
    };
    assert_eq!(run(), run());
}
