// demo.ts — scénario de démonstration hors Tauri (dev navigateur).
// Même histoire que le prototype `docs/design/project-space/crypt-lab.html` :
// on travaille sur le projet crypt lui-même. Le fantôme charge le contexte,
// lit server.rs et error.log, lance deux sous-agents (nginx.conf, le web),
// corrige encode.rs, Kiro juge, réponse, puis F7 sur tous les projets.
// Sert aussi de repli si le backend ne répond pas encore.
import { apply } from "./state";
import { newSceneState, type ProjectTree, type SceneEvent, type SceneState } from "./types";

/// Arborescence de crypt (1 dalle ≈ 150 lignes, comme DESIGN §6.1).
const CRYPT: [string, [string, number][]][] = [
  ["crypt", [["AGENTS.md", 2], ["Cargo.toml", 1], ["README.md", 2]]],
  [".kiro/steering", [["tone.md", 1], ["rust.md", 1]]],
  ["skills", [["debug-http.md", 1]]],
  ["src", [["main.rs", 2], ["app.rs", 4]]],
  ["src/chat", [["session.rs", 5], ["stream.rs", 3], ["tokens.rs", 2]]],
  ["src/hub", [["server.rs", 4], ["encode.rs", 5]]],
  ["src/tui", [["render.rs", 6], ["crypt_scene.rs", 5], ["widgets.rs", 3]]],
  ["src/agents", [["spawn.rs", 4], ["kiro.rs", 3]]],
  ["src/store", [["checkpoints.rs", 3], ["coins.rs", 2]]],
  ["deploy", [["nginx.conf", 2]]],
  ["logs", [["error.log", 3]]],
  ["tests", [["hub_test.rs", 2]]],
];

function pathOf(dir: string, name: string): string {
  return dir === "crypt" ? name : `${dir}/${name}`;
}

/// Arbre de démonstration : le projet crypt (12 dossiers, 25 fichiers).
export function demoTree(): ProjectTree {
  const dirs = CRYPT.map(([path], i) => ({ path, parent: i > 0 ? 0 : null }));
  const files = CRYPT.flatMap(([dir, list], d) =>
    list.map(([name, slabs]) => ({ path: pathOf(dir, name), dir: d, lines: slabs * 150 - 20 })),
  );
  return { root: "/crypt", dirs, files };
}

/// Durée d'un tour du scénario (s).
export const DEMO_LOOP = 25;

/// Le scénario : (instant, événement).
export function demoScript(): [number, SceneEvent][] {
  return [
    [0.4, { type: "prompt_sent", chat: 1 }],
    [1.6, { type: "step_start", step: "context" }],
    [1.7, {
      type: "context_load",
      files: ["AGENTS.md", ".kiro/steering/tone.md", ".kiro/steering/rust.md", "skills/debug-http.md"],
    }],
    [3.6, { type: "step_start", step: "think" }],
    [5.0, { type: "step_start", step: "act" }],
    [5.2, { type: "file_read", path: "src/hub/server.rs", agent: "main" }],
    [7.0, { type: "file_read", path: "logs/error.log", agent: "main" }],
    [8.0, { type: "agent_spawn", id: 1, kind: "sub", target: { path: "deploy/nginx.conf" } }],
    [8.3, { type: "agent_spawn", id: 2, kind: "web", target: "web" }],
    [9.6, { type: "file_write", path: "src/hub/encode.rs", agent: "main", added: 150, removed: 1 }],
    [12.4, { type: "agent_done", id: 1, ok: true }],
    [13.2, { type: "agent_done", id: 2, ok: true }],
    [13.6, { type: "step_start", step: "critic" }],
    [13.7, { type: "critic_start", file: "src/hub/encode.rs" }],
    [16.6, { type: "critic_done", file: "src/hub/encode.rs" }],
    [17.2, { type: "step_start", step: "answer" }],
    [17.8, { type: "checkpoint_saved", n: 3 }],
    [19.5, { type: "hub_open" }],
    [23.5, { type: "hub_close" }],
  ];
}

/// Lance le scénario sur `state` ; rend la fonction d'arrêt.
export function startDemo(state: SceneState, now: () => number): () => void {
  apply(state, { type: "project_open", path: "/crypt", tree: demoTree() }, 0);
  const script = demoScript();
  let i = 0;
  let loop = 0;
  const id = setInterval(() => {
    const t = now() - loop * DEMO_LOOP;
    while (i < script.length && script[i][0] <= t) {
      apply(state, script[i][1], script[i][0] + loop * DEMO_LOOP);
      i++;
    }
    if (i >= script.length && t > DEMO_LOOP) {
      // rejoue le scénario en boucle, depuis un état propre
      i = 0;
      loop += 1;
      Object.assign(state, newSceneState());
      apply(state, { type: "project_open", path: "/crypt", tree: demoTree() }, loop * DEMO_LOOP);
    }
  }, 100);
  return () => clearInterval(id);
}
