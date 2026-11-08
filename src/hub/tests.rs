use super::*;
use crate::sessions::{CloseOutcome, Sessions};
use ratatui::{buffer::Buffer, layout::Rect};

/// 2 chats ; chat 0 : un sous-agent en cours + un terminé ; chat 1 : un échoué.
fn crypt() -> Sessions {
    let mut s = Sessions::new();
    s.add();
    let a = s.add_agent(0, "fouiller", Vec::new()).unwrap();
    let b = s.add_agent(0, "lire", Vec::new()).unwrap();
    let c = s.add_agent(1, "traduire", Vec::new()).unwrap();
    s.find_agent_mut(b).unwrap().1.status = AgentStatus::Done;
    s.find_agent_mut(c).unwrap().1.status = AgentStatus::Failed;
    assert_eq!((a, b, c), (1, 2, 3));
    s
}

fn hub_for(s: &Sessions) -> Hub {
    let mut hub = Hub::new();
    hub.set_room(10.0);
    hub.sync(s);
    hub
}

fn mood(hub: &Hub, key: Key) -> Mood {
    hub.spectres.iter().find(|x| x.key == key).unwrap().mood
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

#[test]
fn sync_maps_chats_to_white_and_subagents_to_black_ghosts() {
    let s = crypt();
    let hub = hub_for(&s);
    let agents = hub
        .spectres
        .iter()
        .filter(|x| matches!(x.key, Key::Agent(_)))
        .count();
    let subs = hub
        .spectres
        .iter()
        .filter(|x| matches!(x.key, Key::Sub(_)))
        .count();
    assert_eq!((agents, subs), (2, 3));
    assert_eq!(mood(&hub, Key::Sub(1)), Mood::Running);
    assert_eq!(mood(&hub, Key::Sub(2)), Mood::Done);
    assert_eq!(mood(&hub, Key::Sub(3)), Mood::Failed);
    assert_eq!(
        hub.spectres
            .iter()
            .find(|x| x.key == Key::Sub(3))
            .unwrap()
            .owner,
        1
    );
}

#[test]
fn closing_a_chat_banishes_its_ghosts_on_next_sync() {
    let mut s = Sessions::new();
    s.add();
    s.add();
    let sub = s.add_agent(0, "fouiller", Vec::new()).unwrap();
    s.find_agent_mut(sub).unwrap().1.status = AgentStatus::Done;
    let mut hub = hub_for(&s);
    assert!(hub.pos(Key::Agent(2)).is_some());
    assert!(hub.pos(Key::Sub(sub)).is_some());
    hub.signal(Signal::Tell { from: 1, to: 2 });
    hub.signal(Signal::Reply { chat: 2 });
    assert_eq!(s.close(0), CloseOutcome::Closed);
    hub.sync(&s);
    assert!(
        hub.pos(Key::Agent(2)).is_none(),
        "le fantôme du chat fermé reste affiché"
    );
    assert!(
        hub.pos(Key::Sub(sub)).is_none(),
        "le sous-agent du chat fermé reste affiché"
    );
    assert!(hub.orbs.is_empty(), "orbe vers le chat fermé");
    assert!(hub.bubbles.is_empty(), "bulle sur le chat fermé");
    assert_eq!(
        hub.spectres
            .iter()
            .filter(|x| matches!(x.key, Key::Agent(_)))
            .count(),
        2
    );
}

#[test]
fn everyone_stays_inside_the_crypt_even_in_panic() {
    let s = crypt();
    let mut hub = hub_for(&s);
    for t in 0..3000 {
        if t % 400 == 0 {
            hub.scare();
        }
        if t == 1500 {
            hub.set_room(5.0);
        }
        hub.sync(&s);
        hub.step();
        let hi = hub.room - WALL_GAP;
        for x in &hub.spectres {
            assert!(
                x.x >= WALL_GAP - 1e-4 && x.x <= hi + 1e-4,
                "x={} hors crypte",
                x.x
            );
            assert!(
                x.y >= WALL_GAP - 1e-4 && x.y <= hi + 1e-4,
                "y={} hors crypte",
                x.y
            );
        }
    }
}

#[test]
fn working_subagent_orbits_close_to_its_agent() {
    let s = crypt();
    let mut hub = hub_for(&s);
    for _ in 0..300 {
        hub.step();
    }
    for _ in 0..200 {
        hub.step();
        let d = dist(
            hub.pos(Key::Agent(0)).unwrap(),
            hub.pos(Key::Sub(1)).unwrap(),
        );
        assert!(d < 2.5, "le sous-agent s'éloigne ({d:.2} dalles)");
    }
}

#[test]
fn a_tell_flies_as_an_orb_and_startles_the_receiver() {
    let s = crypt();
    let mut hub = hub_for(&s);
    hub.signal(Signal::Tell { from: 0, to: 1 });
    assert_eq!(hub.orbs.len(), 1);
    let mut landed = false;
    for _ in 0..40 {
        hub.step();
        let alerted = hub
            .bubbles
            .iter()
            .any(|b| b.key == Key::Agent(1) && b.glyph == Glyph::Alert);
        landed |= hub.orbs.is_empty() && alerted;
    }
    assert!(landed, "l'orbe n'est pas arrivé");
    hub.signal(Signal::Deliver { agent: 1, to: 1 });
    hub.signal(Signal::Ask { from: 1, agent: 3 });
    hub.signal(Signal::Tell { from: 0, to: 9 });
    assert_eq!(hub.orbs.len(), 3);
}

#[test]
fn idle_agents_that_meet_end_up_chatting() {
    let mut s = Sessions::new();
    s.add();
    let mut hub = hub_for(&s);
    let mut chatted = false;
    for _ in 0..4000 {
        for x in hub.spectres.iter_mut() {
            x.goal = (5.0, 5.0);
        }
        hub.step();
        chatted |= hub.spectres.iter().any(|x| x.meet > 0);
    }
    assert!(chatted, "deux agents voisins n'ont jamais bavardé");
}

#[test]
fn halfblock_render_paints_the_crypt_at_any_size() {
    let s = crypt();
    let mut hub = hub_for(&s);
    for _ in 0..50 {
        hub.step();
    }
    for (w, h) in [(1, 1), (8, 4), (50, 30), (140, 40)] {
        let area = Rect::new(2, 1, w, h);
        let mut buf = Buffer::empty(Rect::new(0, 0, w + 4, h + 2));
        render::draw(&mut hub, &s, area, &mut buf, None);
    }
    let area = Rect::new(0, 0, 100, 30);
    let mut buf = Buffer::empty(area);
    render::draw(&mut hub, &s, area, &mut buf, None);
    let blocks = buf.content.iter().filter(|c| c.symbol() == "▀").count();
    assert!(blocks > 300, "scène presque vide ({blocks} demi-blocs)");
    assert!(
        hub.view.take_pending().is_none(),
        "pas d'image Kitty en demi-blocs"
    );
}

#[test]
fn kitty_render_sends_one_image_then_a_frame_per_draw() {
    let s = crypt();
    let mut hub = hub_for(&s);
    let area = Rect::new(0, 0, 60, 20);
    let mut buf = Buffer::empty(area);
    render::draw(&mut hub, &s, area, &mut buf, Some((10, 20)));
    assert!(
        hub.view.take_pending().is_none(),
        "1re frame : transmise par les placeholders"
    );
    let first: String = buf.content.iter().take(1).map(|c| c.symbol()).collect();
    assert!(first.contains("\x1b_G"), "1re frame non transmise");
    hub.step();
    render::draw(&mut hub, &s, area, &mut buf, Some((10, 20)));
    let frame = hub.view.wait_pending().expect("frame suivante");
    assert!(frame.starts_with("\x1b_Ga=T,f=100,U=1,q=2,i="));
    assert!(hub.room >= 3.0 && hub.room <= 14.0);
}
