//! Kiro en balade : apparition, errance bornée, coups, K.O., critique,
//! clics, rendu (rien sur le sprite, bulle dans l'arène).

use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

use super::*;
use crate::gfx::ImageBank;

/// Arène type : sidebar + discussion d'un 100×30 (sous l'en-tête).
const ARENA: Rect = Rect::new(0, 1, 70, 28);

/// Rend une frame (demi-blocs) et la rend.
fn frame(p: &mut Phantom, arena: Rect) -> Buffer {
    let mut bank = ImageBank::headless();
    let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
    term.draw(|f| {
        p.begin_frame();
        p.draw(f, arena, &mut bank);
    })
    .unwrap();
    term.backend().buffer().clone()
}

/// Kiro dessiné une fois (arène connue), graine fixe.
fn placed(seed: u64) -> Phantom {
    let mut p = Phantom::new(seed);
    frame(&mut p, ARENA);
    p
}

/// Centre du sprite (clic en plein corps).
fn centre(p: &Phantom) -> Position {
    let body = p.body.unwrap();
    Position::new(body.x + body.width / 2, body.y + body.height / 2)
}

#[test]
fn appears_centred_in_the_arena_and_idles() {
    let p = placed(7);
    let body = p.body.expect("dessiné");
    assert_eq!((body.width, body.height), draw::HALFBLOCK_CELLS);
    assert_eq!(body.x, ARENA.x + (ARENA.width - body.width) / 2);
    assert_eq!(body.y, ARENA.y + (ARENA.height - body.height) / 2);
    assert!(ARENA.contains(body.as_position()) && body.bottom() <= ARENA.bottom());
    assert_eq!(p.mood(), Mood::Roam);
    assert!(["kiro-spectre", "kiro-flotte"].contains(&p.creature().slug));
}

#[test]
fn roams_but_never_leaves_the_arena_nor_sits_on_the_input() {
    for seed in [1, 2, 3, 99] {
        let mut p = placed(seed);
        let (mx, my, _) = p.limits().unwrap();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..600 {
            p.tick();
            assert!((0.0..=mx).contains(&p.x) && (0.0..=my).contains(&p.y));
            seen.insert((p.x.round() as i32, p.y.round() as i32));
        }
        assert!(seen.len() > 20, "seed {seed}: Kiro reste figé ({seen:?})");
    }
}

#[test]
fn a_smack_knocks_him_away_with_stars_then_he_recovers() {
    let mut p = placed(5);
    let body = p.body.unwrap();
    let x0 = p.x;
    p.smack(Position::new(body.x + 1, body.y + 2)); // côté gauche
    assert!(matches!(p.mood(), Mood::Hurt(_)));
    assert_eq!(p.creature().slug, "kiro-frappe");
    assert_eq!(p.speech().unwrap().tone, Tone::Ouch);
    assert!(lines::OUCH.contains(&p.speech().unwrap().text.as_str()));
    for _ in 0..3 {
        p.tick();
    }
    assert!(p.x > x0, "chassé vers la droite : {} -> {}", x0, p.x);
    let buf = frame(&mut p, ARENA);
    let stars = buf
        .content
        .iter()
        .filter(|c| STAR_GLYPHS.contains(&c.symbol()))
        .count();
    assert!(stars >= 1, "étoiles absentes");
    for _ in 0..HURT_TICKS {
        p.tick();
    }
    assert_eq!(p.mood(), Mood::Roam);
}

/// Glyphes des étoiles (voir `draw`).
const STAR_GLYPHS: [&str; 3] = ["✦", "✧", "★"];

#[test]
fn three_quick_smacks_knock_him_out_upside_down_then_he_gets_up() {
    let mut p = placed(11);
    for _ in 0..3 {
        let at = centre(&p);
        p.smack(at);
        p.tick();
        frame(&mut p, ARENA);
    }
    assert!(matches!(p.mood(), Mood::Ko(_)));
    assert_eq!(p.creature().slug, "kiro-ko");
    assert!(lines::KO.contains(&p.speech().unwrap().text.as_str()));
    for _ in 0..KO_TICKS - 5 {
        p.tick();
    }
    let (_, _, floor) = p.limits().unwrap();
    assert_eq!(p.y, floor, "tombé au sol");
    assert!(matches!(p.mood(), Mood::Ko(_)));
    for _ in 0..5 {
        p.tick();
    }
    assert_eq!(p.mood(), Mood::Roam);
    assert!(lines::GETUP.contains(&p.speech().unwrap().text.as_str()));
}

#[test]
fn slow_smacks_never_knock_him_out() {
    let mut p = placed(3);
    for _ in 0..5 {
        let at = centre(&p);
        p.smack(at);
        for _ in 0..(COMBO_TICKS + 1) {
            p.tick();
        }
        frame(&mut p, ARENA);
        assert!(!matches!(p.mood(), Mood::Ko(_)));
    }
}

#[test]
fn clicks_hit_body_or_bubble_and_the_bubble_hushes() {
    let mut p = placed(4);
    assert_eq!(p.hit_test(centre(&p)), Hit::Body);
    assert_eq!(p.hit_test(Position::new(0, 0)), Hit::Miss);
    p.critique_done(Some(String::from("Vague, lazy, and somehow too long.")), 30);
    frame(&mut p, ARENA);
    let bubble = p.bubble.expect("bulle dessinée");
    assert_eq!(p.hit_test(bubble.as_position()), Hit::Bubble);
    p.hush();
    frame(&mut p, ARENA);
    assert!(p.speech().is_none() && p.bubble.is_none());
    p.set_on(false);
    frame(&mut p, ARENA);
    assert_eq!(p.hit_test(Position::new(35, 14)), Hit::Miss);
}

#[test]
fn critique_reads_then_talks_then_judges_then_fades() {
    let mut p = placed(8);
    assert!(p.wants_critique());
    p.begin_critique();
    assert!(!p.wants_critique(), "une critique à la fois");
    assert_eq!(p.creature().slug, "kiro-juge");
    let buf = frame(&mut p, ARENA);
    assert!(text_of(&buf).contains("hm."), "pas de « hm... » en vol");
    p.critique_done(Some(String::from("Ten words to say nothing.")), 42);
    assert!(p.wants_critique());
    assert_eq!(p.stats(), (1, 42));
    let mut frames = std::collections::HashSet::new();
    for _ in 0..8 {
        p.tick();
        frames.insert(p.creature().slug);
    }
    assert!(frames.contains("kiro-parle") && frames.contains("kiro-juge"));
    assert!(text_of(&frame(&mut p, ARENA)).contains("Ten words"));
    let still = (p.x, p.y);
    for _ in 0..30 {
        p.tick();
    }
    assert_eq!((p.x, p.y), still, "immobile pendant qu'il parle");
    assert_eq!(p.creature().slug, "kiro-juge");
    for _ in 0..200 {
        p.tick();
    }
    assert!(p.speech().is_none());
}

#[test]
fn off_hides_him_but_still_counts_tokens_and_failures_mumble() {
    let mut p = placed(2);
    p.critique_failed();
    assert_eq!(p.speech().unwrap().text, lines::FAILED);
    p.set_on(false);
    assert!(!p.wants_critique());
    p.critique_done(Some(String::from("meh")), 12);
    assert!(p.speech().is_none());
    assert_eq!(p.stats(), (1, 12));
    frame(&mut p, ARENA);
    assert!(p.body.is_none());
    p.scare();
    assert_eq!(p.mood(), Mood::Roam, "éteint : pas d'effroi");
}

#[test]
fn f5_scares_him_away() {
    let mut p = placed(6);
    p.scare();
    assert_eq!(p.creature().slug, "kiro-peur");
    assert_eq!(p.speech().unwrap().text, lines::SCARED);
    let x0 = p.x;
    for _ in 0..SCARE_TICKS {
        p.tick();
    }
    assert!((p.x - x0).abs() >= 10.0, "n'a pas fui : {x0} -> {}", p.x);
    assert_eq!(p.mood(), Mood::Roam);
}

#[test]
fn rendering_never_writes_on_the_sprite_and_bubble_stays_in_the_arena() {
    let mut p = placed(9);
    let at = centre(&p);
    p.smack(at);
    for _ in 0..12 {
        let buf = frame(&mut p, ARENA);
        let body = p.body.unwrap();
        // Même Kiro sans bulle ni « BONK! » : seules les cellules du
        // sprite comptent.
        let sprite = {
            let mut solo = Phantom::new(1);
            solo.mood = p.mood;
            solo.tick = p.tick;
            solo.arena = p.arena;
            solo.size = p.size;
            solo.spawned = true;
            solo.x = p.x;
            solo.y = p.y;
            frame(&mut solo, ARENA)
        };
        for pos in body.positions() {
            assert_eq!(
                buf[pos], sprite[pos],
                "cellule du sprite touchée en {pos:?}"
            );
        }
        if let Some(bubble) = p.bubble {
            assert!(bubble.intersection(body).is_empty());
            assert_eq!(bubble.intersection(ARENA), bubble);
        }
        p.tick();
    }
}

/// Tout le texte du buffer, ligne par ligne.
fn text_of(buf: &Buffer) -> String {
    let area = buf.area;
    (area.top()..area.bottom())
        .map(|y| {
            (area.left()..area.right())
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
