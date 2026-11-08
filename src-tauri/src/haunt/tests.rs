//! Tests du jeu : niveau, physique de Kiro, linceul, tokens, dangers,
//! reprise, Faucheuse, cadence 240 Hz, déterminisme.

use std::time::Duration;

use super::boss::DYING;
use super::foes::{Foe, Kind};
use super::input::{Act, KeyKind};
use super::level::{Level, TILE};
use super::player::{Kiro, DEATH};
use super::shots::{Loot, Pickup, Weapon};
use super::{Haunt, Phase, HZ, STEP, SUB};

/// Partie lancée (phase jeu) sur le niveau 1.
fn playing() -> Haunt {
    let mut game = Haunt::new();
    game.set_releases(true);
    game.new_run();
    game
}

/// Partie lancée sur un niveau de test (13 lignes : 12 + zones).
fn on(rows: &[&str]) -> Haunt {
    let mut game = playing();
    game.level = Level::parse(&rows.join("\n")).expect("niveau de test valide");
    game.armed = vec![false; game.level.marks.len()];
    game.checkpoint = game.level.mark('P').unwrap().x;
    game.respawn();
    game.director = u16::MAX;
    game
}

/// `n` pas de référence (1/30 s).
fn run(game: &mut Haunt, n: u32) {
    for _ in 0..n * SUB {
        game.tick();
    }
}

fn press(game: &mut Haunt, act: Act) {
    game.feed(act, KeyKind::Press);
}

fn release(game: &mut Haunt, act: Act) {
    game.feed(act, KeyKind::Release);
}

/// Sol plat, un mur de briques, une dalle, une fosse, de l'ectoplasme.
const FLAT: [&str; 13] = [
    "B.............................",
    "B.............................",
    "B.............................",
    "B.............................",
    "B.............................",
    "B.............................",
    "B.............................",
    "B.........====..........R.....",
    "B..P..........................",
    "#############...####~~~#######",
    "#############...##############",
    "#############...##############",
    "..............................",
];

#[test]
fn level_one_parses_with_its_landmarks() {
    let level = Level::first();
    assert_eq!((level.rows, level.cols), (96, 960));
    let start = level.mark('P').unwrap();
    assert_eq!(level.ground_below(start.x, start.y - 1.0), Some(start.y));
    let slabs = (0..level.rows as i64)
        .flat_map(|r| (0..level.cols as i64).map(move |c| (c, r)))
        .filter(|&(c, r)| level.tile(c, r) == super::level::Tile::Slab)
        .count();
    assert!(
        slabs > 1000,
        "un vrai niveau de plateformes : {slabs} dalles"
    );
    assert!(level.mark('k').is_some() && level.mark('R').is_some());
    assert_eq!(level.marks.iter().filter(|m| m.ch == 'C').count(), 4);
    assert!(level.marks.iter().filter(|m| m.ch == 'H').count() >= 3);
}

#[test]
fn level_parse_rejects_bad_grids() {
    assert!(Level::parse("").is_err());
    assert!(Level::parse("P.R\n...\n...").is_ok());
    assert!(Level::parse("P.R\n..\n...").is_err(), "lignes inégales");
    assert!(Level::parse("P.R\n.Q.\n...").is_err(), "lettre inconnue");
    assert!(Level::parse("..R\n...\n...").is_err(), "sans départ");
    assert!(Level::parse("P..\n...\n...").is_err(), "sans arène");
    assert!(Level::parse("P.R\n...\n.X.").is_err(), "zone inconnue");
}

#[test]
fn sprites_load_in_the_bestiary_style() {
    let sp = super::sprites::Sprites::load();
    assert!(sp.tiles.iter().all(|t| t.dimensions() == (6, 6)));
    assert_eq!(sp.reaper.get(1.0).dimensions(), (32, 36));
    assert_eq!(
        sp.shroud[0].get(-1.0).dimensions(),
        (9, 11),
        "fantôme du hub"
    );
    assert_eq!(sp.kiro[0].get(1.0).dimensions(), (9, 11));
    assert_eq!(sp.kiro[0].pad, 0, "Kiro posé sur ses franges");
}

#[test]
fn kiro_lands_then_walks_39_pixels_per_second() {
    let mut game = on(&FLAT);
    run(&mut game, 10);
    assert!(game.kiro.body.ground);
    assert_eq!(game.kiro.body.feet(), 9.0 * TILE);
    let x0 = game.kiro.body.x;
    press(&mut game, Act::Right);
    run(&mut game, 30);
    let walked = game.kiro.body.x - x0;
    assert!((37.0..=41.0).contains(&walked), "{walked} px en 1 s");
    release(&mut game, Act::Right);
    let x1 = game.kiro.body.x;
    run(&mut game, 5);
    assert_eq!(game.kiro.body.x, x1, "relâché = arrêt");
}

#[test]
fn jump_clears_four_tiles_and_lands() {
    let mut game = on(&FLAT);
    run(&mut game, 5);
    press(&mut game, Act::Up);
    let mut top = f32::MAX;
    for _ in 0..40 * SUB {
        game.tick();
        top = top.min(game.kiro.body.feet());
    }
    let height = 9.0 * TILE - top;
    assert!(
        (24.0..=29.0).contains(&height),
        "saut de {height} px (4 dalles)"
    );
    assert!(game.kiro.body.ground);
}

#[test]
fn walls_stop_kiro() {
    let mut game = on(&FLAT);
    press(&mut game, Act::Left);
    run(&mut game, 60);
    assert_eq!(game.kiro.body.x, TILE, "collé au mur de briques");
}

#[test]
fn slabs_carry_from_above_and_drop_with_sink_and_jump() {
    let mut game = on(&FLAT);
    let slab_x = 12.0 * TILE;
    game.kiro = Kiro::spawn(slab_x, 30.0, Weapon::Token);
    run(&mut game, 20);
    assert_eq!(game.kiro.body.feet(), 7.0 * TILE, "posé sur la dalle");
    press(&mut game, Act::Down);
    run(&mut game, 2);
    assert!(game.kiro.sunk);
    press(&mut game, Act::Up);
    run(&mut game, 20);
    assert_eq!(game.kiro.body.feet(), 9.0 * TILE, "redescendu au sol");
}

#[test]
fn shroud_absorbs_one_hit_then_kiro_dies() {
    let mut game = on(&FLAT);
    run(&mut game, 5);
    let x = game.kiro.body.cx();
    game.hurt_kiro(x + 5.0);
    assert!(game.kiro.alive() && !game.kiro.shroud);
    assert!(game.kiro.body.vx < 0.0, "projeté loin du coup");
    game.hurt_kiro(x);
    assert!(game.kiro.alive(), "invulnérable juste après");
    run(&mut game, 60);
    game.hurt_kiro(x);
    assert!(!game.kiro.alive());
}

#[test]
fn pits_ectoplasm_and_spikes_are_deadly() {
    let mut game = on(&FLAT);
    game.kiro = Kiro::spawn(15.0 * TILE, 30.0, Weapon::Token);
    run(&mut game, 40);
    assert!(!game.kiro.alive(), "fosse");
    assert_eq!(game.cause, "SWALLOWED BY THE DARK");
    let mut game = on(&FLAT);
    game.kiro = Kiro::spawn(21.0 * TILE, 30.0, Weapon::Token);
    run(&mut game, 20);
    assert_eq!(game.cause, "DROWNED IN ECTOPLASM");
}

#[test]
fn a_token_destroys_a_black_ghost_and_scores() {
    let mut game = on(&FLAT);
    run(&mut game, 5);
    let (x, y) = game.kiro.muzzle();
    game.foes.push(Foe::new(Kind::Wisp, x + 40.0, y + 3.0));
    press(&mut game, Act::Fire);
    run(&mut game, 20);
    assert!(game.foes.iter().all(|f| f.kind != Kind::Wisp));
    assert_eq!((game.score, game.sent), (50, 1));
}

#[test]
fn tokens_in_flight_are_capped() {
    let mut game = on(&FLAT);
    run(&mut game, 5);
    for _ in 0..12 {
        press(&mut game, Act::Fire);
        run(&mut game, 1);
    }
    assert!(game.shots.len() <= Weapon::Token.max());
    assert!(game.sent >= 2);
}

#[test]
fn candle_burning_out_kills_and_three_deaths_end_the_run() {
    let mut game = on(&FLAT);
    for life in 0..3 {
        game.candle = 1;
        game.tick();
        assert!(!game.kiro.alive(), "vie {life}");
        assert_eq!(game.cause, "THE CANDLE BURNT OUT");
        for _ in 0..u32::from(DEATH) + 2 {
            game.tick();
        }
    }
    assert_eq!(game.phase, Phase::Over);
    assert_eq!(game.lives, 0);
}

#[test]
fn checkpoint_is_where_kiro_rises_again() {
    let mut game = playing();
    let mark = game.level.mark('k').unwrap();
    let k = mark.x;
    game.kiro = Kiro::spawn(k + 10.0, mark.y, Weapon::Sharp);
    run(&mut game, 1);
    assert_eq!(game.checkpoint, k);
    game.kill_kiro("TEST");
    run(&mut game, 80);
    assert!(game.kiro.alive() && game.kiro.shroud);
    assert!((game.kiro.body.cx() - k).abs() < 1.0);
    assert_eq!(
        game.kiro.body.feet(),
        mark.y,
        "debout au pied de la lanterne"
    );
    assert_eq!(game.kiro.weapon, Weapon::Sharp, "l'arme reste");
    assert_eq!(game.lives, 2);
}

#[test]
fn the_reaper_locks_the_arena_and_falls() {
    let mut game = playing();
    let gate = game.level.mark('R').unwrap();
    game.kiro = Kiro::spawn(gate.x + 40.0, gate.y, Weapon::Token);
    game.cam = gate.x - 400.0;
    run(&mut game, 1);
    let left = gate.x - TILE / 2.0;
    assert_eq!(game.arena, Some(left));
    assert!(game.boss.is_some());
    run(&mut game, 100);
    let lock = left.min(game.level.width() - super::VIEW_W as f32);
    assert!((game.cam - lock).abs() < 1.0, "vue fermée sur l'arène");
    let boss = game.boss.as_mut().unwrap();
    boss.hp = 1;
    assert!(boss.hurt(1));
    game.kiro.hurt = u16::MAX;
    for _ in 0..u32::from(DYING) + 2 {
        game.tick();
    }
    assert_eq!(game.phase, Phase::Clear);
    assert!(game.score >= 5000 || game.score > 0);
}

#[test]
fn coffins_open_on_their_loot() {
    let mut game = on(&FLAT);
    run(&mut game, 5);
    let x = game.kiro.body.cx() + 20.0;
    let mut coffin = Foe::new(Kind::Coffin, x, 9.0 * TILE);
    coffin.loot = Some(Loot::Hot);
    game.foes.push(coffin);
    let j = game.foes.len() - 1;
    for _ in 0..3 {
        game.strike(j, 1);
    }
    assert!(game.foes[j].open);
    assert_eq!(game.pickups.len(), 1);
    game.pickups[0] = Pickup::new(Loot::Hot, game.kiro.body.cx(), 9.0 * TILE);
    run(&mut game, 2);
    assert_eq!(game.kiro.weapon, Weapon::Hot);
    assert!(game.pickups.is_empty());
}

#[test]
fn pump_runs_240_steps_per_second() {
    assert_eq!(HZ, 240);
    let mut game = playing();
    let start = game.step;
    for _ in 0..1000 {
        game.pump(Duration::from_millis(1));
    }
    let steps = game.step - start;
    assert!((239..=240).contains(&steps), "{steps} pas en 1 s");
    assert!(STEP < Duration::from_micros(4200));
    let before = game.step;
    assert!(game.pump(Duration::from_secs(5)));
    assert!(game.step - before <= 32, "rattrapage borné");
}

#[test]
fn same_inputs_same_game() {
    let play = || {
        let mut game = playing();
        press(&mut game, Act::Right);
        for i in 0..600 {
            if i % 25 == 0 {
                press(&mut game, Act::Fire);
            }
            if i % 70 == 0 {
                press(&mut game, Act::Up);
            }
            run(&mut game, 1);
        }
        (game.score, game.sent, game.kiro.body.x, game.foes.len())
    };
    assert_eq!(play(), play());
}

#[test]
fn a_long_random_run_stays_sane() {
    let mut game = playing();
    let acts = [
        Act::Left,
        Act::Right,
        Act::Up,
        Act::Down,
        Act::Fire,
        Act::Start,
    ];
    for i in 0..60 * HZ as u64 {
        if i % 40 == 0 {
            let act = acts[(game.dice.next() % acts.len() as u64) as usize];
            let kind = if i % 80 == 0 {
                KeyKind::Press
            } else {
                KeyKind::Release
            };
            game.feed(act, kind);
        }
        game.tick();
        assert!(game.foes.len() < 80, "créatures sans fin");
        assert!(game.sparks.list.len() <= 420);
    }
}

/// Kiro peut-il se tenir sur la dalle (c, r) ? Sol plein ou dalle fine,
/// pas de pieux, deux dalles d'air au-dessus.
fn standable(level: &Level, c: i64, r: i64) -> bool {
    use super::level::Tile;
    let floor = level.tile(c, r);
    let roof = (1..=2).all(|k| !level.tile(c, r - k).solid());
    (floor.solid() || floor == Tile::Slab) && floor != Tile::Spikes && roof
}

#[test]
fn level_one_arena_is_reachable_by_jumps() {
    let level = Level::first();
    let start = level.mark('P').unwrap();
    let gate = level.mark('R').unwrap();
    let cell = |x: f32, y: f32| ((x / TILE) as i64, (y / TILE) as i64);
    let (cols, rows) = (level.cols as i64, level.rows as i64);
    let mut seen = std::collections::HashSet::from([cell(start.x, start.y)]);
    let mut todo = vec![cell(start.x, start.y)];
    while let Some((c, r)) = todo.pop() {
        for c2 in (c - 9).max(0)..=(c + 9).min(cols - 1) {
            for r2 in 2..rows {
                let (dx, dy) = ((c2 - c).abs(), r2 - r);
                let jump = dy >= -4 && dx <= 5 || dy > 0 && dx <= 5 + dy / 2;
                if jump && standable(&level, c2, r2) && seen.insert((c2, r2)) {
                    todo.push((c2, r2));
                }
            }
        }
    }
    let far = seen.iter().map(|&(c, _)| c).max().unwrap();
    assert!(
        far > cell(gate.x, gate.y).0,
        "arène hors d'atteinte (col {far})"
    );
}
