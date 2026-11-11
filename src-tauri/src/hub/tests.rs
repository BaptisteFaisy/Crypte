//! Tests hub Tauri (remplacent `hub/tests.rs` du TUI, couplé à
//! `Sessions`/ratatui) : sync, prune, signaux, panique, déterminisme.

use super::*;

fn demo_view() -> SyncView {
    SyncView {
        chats: vec![
            ChatSync {
                sending: false,
                agents: vec![
                    AgentSync {
                        id: 1,
                        status: AgentStatus::Running,
                    },
                    AgentSync {
                        id: 2,
                        status: AgentStatus::Done,
                    },
                ],
            },
            ChatSync {
                sending: true,
                agents: vec![AgentSync {
                    id: 3,
                    status: AgentStatus::Failed,
                }],
            },
        ],
    }
}

#[test]
fn sync_aligne_humeurs_et_bannit_les_fermes() {
    let mut hub = Hub::new();
    hub.sync(&demo_view());
    assert_eq!(hub.spectres.len(), 5);
    let mood = |key: Key| hub.spectres.iter().find(|s| s.key == key).unwrap().mood;
    assert_eq!(mood(Key::Agent(0)), Mood::Idle);
    assert_eq!(mood(Key::Agent(1)), Mood::Waiting);
    assert_eq!(mood(Key::Sub(1)), Mood::Running);
    assert_eq!(mood(Key::Sub(2)), Mood::Done);
    assert_eq!(mood(Key::Sub(3)), Mood::Failed);
    let closed = SyncView {
        chats: vec![ChatSync {
            sending: false,
            agents: Vec::new(),
        }],
    };
    hub.sync(&closed);
    assert!(hub.spectres.iter().all(|s| s.key == Key::Agent(0)));
}

#[test]
fn signaux_orbes_et_replique() {
    let mut hub = Hub::new();
    hub.sync(&demo_view());
    hub.signal(Signal::Tell { from: 0, to: 1 });
    assert_eq!(hub.orbs.len(), 1);
    for _ in 0..40 {
        hub.step();
    }
    assert!(hub.orbs.is_empty());
    assert!(!hub.bubbles.is_empty());
    hub.signal(Signal::Reply { chat: 0 });
    assert!(hub.bubbles.iter().any(|b| b.key == Key::Agent(0)));
}

#[test]
fn simulation_deterministe_et_bornee() {
    let run = || {
        let mut hub = Hub::new();
        hub.sync(&demo_view());
        for _ in 0..50 {
            hub.step();
        }
        hub.spectres
            .iter()
            .map(|s| (s.key, (s.x * 1000.0) as i32, (s.y * 1000.0) as i32))
            .collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
    let mut hub = Hub::new();
    hub.sync(&demo_view());
    hub.scare();
    for _ in 0..30 {
        hub.step();
    }
    for s in &hub.spectres {
        assert!((0.0..=hub.room()).contains(&s.x));
        assert!((0.0..=hub.room()).contains(&s.y));
    }
}
