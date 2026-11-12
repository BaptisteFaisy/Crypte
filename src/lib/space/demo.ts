// demo.ts — scénario de démonstration hors Tauri (dev navigateur).
// Mêmes beats que `src/space/demo.rs` du port terminal : lecture, écriture,
// sous-agent poussé, Kiro, /rewind, hub. Sert aussi de repli si le backend
// ne répond pas encore.
import { apply } from "./state";
import type { ProjectTree, SceneEvent, SceneState } from "./types";

/// Arbre de démonstration : 4 dossiers, 24 fichiers.
export function demoTree(): ProjectTree {
  const dirs = [0, 1, 2, 3].map((i) => ({
    path: `src/mod${i}`,
    parent: i > 0 ? 0 : null,
  }));
  const files = Array.from({ length: 24 }, (_, i) => ({
    path: `src/mod${i % 4}/f${String(i).padStart(2, "0")}.rs`,
    dir: i % 4,
    lines: 40 + ((i * 137) % 900),
  }));
  return { root: "/demo", dirs, files };
}

/// Le scénario : (instant, événement).
export function demoScript(): [number, SceneEvent][] {
  const hot = "src/mod1/f05.rs";
  const read = "src/mod0/f04.rs";
  return [
    [0.4, { type: "file_read", path: read, agent: "main" }],
    [1.6, { type: "agent_spawn", id: 1, kind: "sub", target: { path: "src/mod2/f10.rs" } }],
    [2.0, { type: "critic_start", file: hot }],
    [2.6, { type: "step_start", step: "act" }],
    [3.4, { type: "agent_done", id: 1, ok: true }],
    [4.0, { type: "critic_done", file: hot }],
    [4.6, { type: "file_write", path: hot, agent: "main", added: 300, removed: 0 }],
    [6.0, { type: "checkpoint_saved", n: 1 }],
    [6.6, { type: "rewind", to: 1, files: [hot] }],
    [7.6, { type: "step_start", step: "answer" }],
    [8.4, { type: "hub_open" }],
    [11.0, { type: "hub_close" }],
    [11.6, { type: "file_read", path: read, agent: "main" }],
  ];
}

/// Lance le scénario sur `state` ; rend la fonction d'arrêt.
export function startDemo(state: SceneState, now: () => number): () => void {
  apply(state, { type: "project_open", path: "/demo", tree: demoTree() }, 0);
  const script = demoScript();
  let i = 0;
  let loop = 0;
  const id = setInterval(() => {
    const t = now() - loop * 12;
    while (i < script.length && script[i][0] <= t) {
      apply(state, script[i][1], script[i][0]);
      i++;
    }
    if (i >= script.length && t > 12) {
      // rejoue le scénario en boucle pour la démo
      i = 0;
      loop += 1;
      apply(state, { type: "project_open", path: "/demo", tree: demoTree() }, loop * 12);
    }
  }, 100);
  return () => clearInterval(id);
}
