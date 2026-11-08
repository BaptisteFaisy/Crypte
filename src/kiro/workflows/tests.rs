use super::*;
use std::collections::HashSet;

/// Exemple officiel (`plan-and-implement`, doc authoring).
fn plan_recipe() -> Recipe {
    serde_json::from_str(
        r#"{"name": "plan-and-implement", "description": "Plan then implement",
        "inputs": {"task": "prompt"},
        "steps": [
          {"type": "step", "id": "plan", "agent": "wf-planner",
           "prompt": "Create a plan for: {{task}}. Do not edit files."},
          {"type": "step", "id": "implement", "agent": "wf-coder",
           "prompt": "Implement:\n\n{{previous.output}}"}]}"#,
    )
    .unwrap()
}

/// L'exemple officiel valide sans erreur ni warning.
#[test]
fn exemple_officiel_valide() {
    let lints = validate(&plan_recipe(), &HashSet::new());
    assert!(lints.is_empty(), "{lints:?}");
}

/// Erreurs doc : ids, agents, gabarits, repeat, parallel.
#[test]
fn validate_signale_erreurs_doc() {
    let customs = HashSet::from(["moi".to_string()]);
    let bad: Recipe = serde_json::from_str(
        r#"{"name": "", "steps": [
        {"type": "step", "id": "a", "agent": "nope", "prompt": "{{previous.output}} {{x}}"},
        {"type": "step", "id": "a", "agent": "moi", "prompt": "ok {{b.output}}"},
        {"type": "repeat", "id": "r", "steps": []},
        {"type": "parallel", "id": "p", "branches": []},
        {"type": "zzz", "id": "z"}]}"#,
    )
    .unwrap();
    let all = validate(&bad, &customs);
    let texts: Vec<String> = all.iter().map(|lint| lint.text.clone()).collect();
    for want in [
        "missing recipe name",
        "unknown agent 'nope'",
        "{{previous.output}} in first node",
        "duplicate id 'a'",
        "repeat without maxIterations",
        "parallel without joinPolicy",
        "unknown node type 'zzz'",
    ] {
        assert!(
            texts.iter().any(|t| t.contains(want)),
            "{texts:?} manque {want}"
        );
    }
    let warns: Vec<&Lint> = all
        .iter()
        .filter(|lint| lint.level == Level::Warning)
        .collect();
    assert!(
        warns.iter().any(|lint| lint.text.contains("{{x}}")),
        "{warns:?}"
    );
}

/// Expansion : entrées, previous, sorties, artefacts, inconnu littéral.
#[test]
fn expand_toutes_refs() {
    let inputs = HashMap::from([(String::from("task"), String::from("T"))]);
    let outputs = HashMap::from([
        (String::from("plan"), String::from("P")),
        (String::from("doc"), String::from("D")),
    ]);
    let ctx = Ctx {
        inputs: &inputs,
        outputs: &outputs,
        previous: Some("prev"),
    };
    assert_eq!(expand("{{task}}!", &ctx), "T!");
    assert_eq!(expand("[{{previous.output}}]", &ctx), "[prev]");
    assert_eq!(expand("{{plan.output}}", &ctx), "P");
    assert_eq!(expand("{{steps.plan.output}}", &ctx), "P");
    assert_eq!(expand("{{artifacts.doc}}", &ctx), "D");
    assert_eq!(
        expand("{{artifacts.unknown}}", &ctx),
        "{{artifacts.unknown}}"
    );
    assert_eq!(expand("{{nope}}", &ctx), "{{nope}}");
    assert_eq!(expand("sans {{fermeture", &ctx), "sans {{fermeture");
}

/// Extraction linéaire : aplati les séquences, refuse le reste.
#[test]
fn linear_steps_aplatit_et_refuse() {
    let recipe: Recipe = serde_json::from_str(
        r#"{"name": "w", "steps": [
        {"type": "step", "id": "a", "agent": "wf-coder", "prompt": "A",
         "artifacts": {"doc": "d.md"}},
        {"type": "sequence", "id": "s", "steps": [
          {"type": "step", "id": "b", "agent": "wf-coder", "prompt": "B {{artifacts.doc}}"}]}]}"#,
    )
    .unwrap();
    let (steps, producers) = linear_steps(&recipe).unwrap();
    assert_eq!(
        steps.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert_eq!(producers.get("doc").map(String::as_str), Some("a"));
    for kind in ["repeat", "parallel", "watch", "zzz"] {
        let bad: Recipe = serde_json::from_str(&format!(
            r#"{{"name": "w", "steps": [{{"type": "{kind}", "id": "x"}}]}}"#
        ))
        .unwrap();
        assert!(linear_steps(&bad).is_err(), "{kind} devrait être refusé");
    }
}

/// Chargement : projet gagne, YAML signalé, JSON invalide signalé.
#[test]
fn load_precedence_et_formats() {
    let dir = std::env::temp_dir().join(format!("kh-wf-load-{}", std::process::id()));
    let (global, project) = (dir.join("home/.kiro"), dir.join("proj"));
    std::fs::create_dir_all(global.join("workflows")).unwrap();
    std::fs::create_dir_all(project.join(".kiro/workflows")).unwrap();
    std::fs::write(
        global.join("workflows/a.workflow.json"),
        r#"{"name": "global", "steps": []}"#,
    )
    .unwrap();
    std::fs::write(
        project.join(".kiro/workflows/a.workflow.json"),
        r#"{"name": "project", "steps": []}"#,
    )
    .unwrap();
    std::fs::write(project.join(".kiro/workflows/b.workflow.yaml"), "name: y\n").unwrap();
    std::fs::write(project.join(".kiro/workflows/c.workflow.json"), "{oops").unwrap();
    let found = load_all_in(&project, Some(&global));
    assert_eq!(
        found.iter().map(|f| f.stem.as_str()).collect::<Vec<_>>(),
        vec!["a", "b", "c"]
    );
    assert_eq!(found[0].recipe.as_ref().unwrap().name, "project");
    assert!(found[1].recipe.as_ref().unwrap_err().contains("yaml"));
    assert!(found[2]
        .recipe
        .as_ref()
        .unwrap_err()
        .contains("invalid json"));
    std::fs::remove_dir_all(&dir).ok();
}
