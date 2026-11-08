//! Tests du runner specs (génération chaînée + run y/n).

use super::*;
use crate::provider::Completion;

/// Dossier temporaire unique par test (projet + checkpoints).
fn tmp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("bt-spec-{}-{name}", std::process::id()))
}

fn runner(dir: &Path) -> (SpecRunner, HooksEngine) {
    let ckpt = dir.join("ckpt");
    std::fs::create_dir_all(&ckpt).unwrap();
    (
        SpecRunner::new(dir.to_path_buf(), Some(ckpt)),
        HooksEngine::new(dir.to_path_buf(), None),
    )
}

fn seed_tasks(dir: &Path, name: &str, md: &str) {
    let spec = dir.join(".kiro/specs").join(name);
    std::fs::create_dir_all(&spec).unwrap();
    std::fs::write(spec.join("tasks.md"), md).unwrap();
}

fn completion(text: &str) -> Completion {
    Completion {
        text: text.to_string(),
        tool_calls: Vec::new(),
        prompt_tokens: 10,
        completion_tokens: 5,
    }
}

#[test]
fn generation_enchaine_trois_docs_puis_conclut() {
    let dir = tmp_dir("gen");
    let (runner, mut hooks) = runner(&dir);
    let gen = runner.gen_plan("auth", "login").unwrap();
    let outs = runner.gen_start(0, gen.clone());
    assert!(outs.iter().any(|o| matches!(o, SpecOut::SpawnGen { .. })));
    let mut gen = gen;
    for (step, file) in ["requirements.md", "design.md", "tasks.md"]
        .iter()
        .enumerate()
    {
        let text = if step == 2 {
            "- [ ] code\n- [ ] test\n"
        } else {
            "doc\n"
        };
        let outs = runner.gen_reply(0, gen.clone(), Ok(completion(text)), &mut hooks);
        assert!(outs.iter().any(|o| matches!(o, SpecOut::LedgerAdd { .. })));
        if step < 2 {
            let next = outs
                .iter()
                .find_map(|o| match o {
                    SpecOut::SpawnGen { gen, .. } => Some(gen.clone()),
                    _ => None,
                })
                .expect("étape suivante");
            assert!(dir.join(".kiro/specs/auth").join(file).is_file());
            gen = next;
        } else {
            assert!(outs.iter().any(|o| matches!(
                o,
                SpecOut::ChatLine { text, .. } if text.contains("2 tasks (3/3)")
            )));
        }
    }
    // Refus : existe déjà, nom invalide.
    assert!(runner.gen_plan("auth", "x").is_err());
    assert!(runner.gen_plan("a/b", "x").is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn run_auto_demande_puis_yn_enchaine_et_termine() {
    let dir = tmp_dir("run");
    seed_tasks(&dir, "auth", "- [ ] one\n- [ ] two\n");
    let (mut runner, mut hooks) = runner(&dir);
    let outs = runner.run_start("auth", 0);
    let prompt = outs
        .iter()
        .find_map(|o| match o {
            SpecOut::SpawnAgent { prompt, .. } => Some(prompt.clone()),
            _ => None,
        })
        .expect("spawn agent");
    assert!(prompt.contains("one"));
    let outs = runner.agent_started("auth", 0, 0, 2, 7, "one", &mut hooks);
    assert!(outs.iter().any(|o| matches!(
        o,
        SpecOut::ChatLine { text, .. } if text.contains("◈ spec auth — task 1/2: one")
    )));
    // Run déjà en cours.
    assert!(runner.run_start("auth", 0).iter().any(|o| matches!(
        o,
        SpecOut::ChatLine { text, .. } if text.contains("already running")
    )));
    // Tâche finie : cochée + demande y/n (auto).
    let outs = runner.agent_done(7, &mut hooks);
    assert!(outs.iter().any(|o| matches!(
        o,
        SpecOut::ChatLine { text, .. } if text.contains("task 1/2 done (1/2)")
    )));
    assert!(outs.iter().any(|o| matches!(o, SpecOut::AskConfirm { .. })));
    assert!(runner.waiting().is_some());
    let raw = std::fs::read_to_string(dir.join(".kiro/specs/auth/tasks.md")).unwrap();
    assert!(raw.contains("- [x] one"), "{raw}");
    // `y` relance, `n` stoppe après la 2e tâche.
    let outs = runner.respond("y");
    assert!(outs.iter().any(|o| matches!(o, SpecOut::SpawnAgent { .. })));
    let outs = runner.agent_started("auth", 0, 1, 2, 8, "two", &mut hooks);
    assert!(outs
        .iter()
        .any(|o| matches!(o, SpecOut::Progress { done: 1, .. })));
    let outs = runner.agent_done(8, &mut hooks);
    assert!(outs.iter().any(|o| matches!(
        o,
        SpecOut::ChatLine { text, .. } if text.contains("complete (2/2)")
    )));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn echec_agent_et_reponse_hors_attente() {
    let dir = tmp_dir("fail");
    seed_tasks(&dir, "auth", "- [ ] one\n");
    let (mut runner, mut hooks) = runner(&dir);
    runner.set_mode(Mode::Yolo);
    let _ = runner.run_start("auth", 0);
    let _ = runner.agent_started("auth", 0, 0, 1, 7, "one", &mut hooks);
    let outs = runner.agent_failed(7, &mut hooks);
    assert!(outs.iter().any(|o| matches!(
        o,
        SpecOut::ChatLine { text, .. } if text.contains("task 1/1 failed")
    )));
    let raw = std::fs::read_to_string(dir.join(".kiro/specs/auth/tasks.md")).unwrap();
    assert!(raw.contains("- [ ] one"), "non cochée : {raw}");
    assert!(runner.respond("y").is_empty());
    assert!(runner.agent_done(999, &mut hooks).is_empty());
    std::fs::remove_dir_all(&dir).ok();
}
