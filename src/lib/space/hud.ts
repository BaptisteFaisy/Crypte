// hud.ts — ce que les calques au-dessus de la 3D affichent (DESIGN §3, §5).
// Fonction pure de l'état de scène : testable sans DOM ni three.js.
import { STEP_ORDER, stepRank, type SceneState, type Step } from "./types";

export type WhoKind = "main" | "sub" | "kiro" | "alert" | "";

export interface HudMetric {
  label: string;
  value: string;
  /** Remplissage de la barre, 0..1. */
  fill: number;
}

export interface Hud {
  /** Petite ligne au-dessus du titre : « 04 / 06 », « checkpoint 3 », « hub ». */
  index: string;
  /** Grand titre : l'étape en cours. */
  title: string;
  who: string;
  whoKind: WhoKind;
  caption: string;
  metrics: HudMetric[];
}

const TITLES: Record<Step, string> = {
  ask: "Ask",
  context: "Context",
  think: "Think",
  act: "Act",
  critic: "Critic",
  answer: "Answer",
};

/** Durée pendant laquelle un /rewind reste la nouvelle du moment (s). */
const REWIND_NEWS = 4;

export function hudOf(state: SceneState, now: number): Hud {
  const subsAlive = [...state.subs.values()].filter((s) => s.doneAt === null).length;
  let changed = 0;
  let slabs = 0;
  for (const [path, f] of state.files) {
    // Les fichiers des projets voisins ont un chemin absolu : on ne compte
    // que le projet actif.
    if (path.startsWith("/")) continue;
    if (f.changed) changed++;
    slabs += f.slabs;
  }
  const project = state.projects[0]?.name ?? "";

  let index = state.step ? `0${stepRank(state.step)} / 0${STEP_ORDER.length}` : project;
  let title = state.step ? TITLES[state.step] : "Ready";
  let who = "main ghost";
  let whoKind: WhoKind = "main";
  let caption = project ? `Waits on the ground of ${project}` : "Waits on the ground";

  const rewound = state.rewind && now - state.rewind.at < REWIND_NEWS;
  const kiroOn = state.kiro && state.kiro.doneAt === null;

  if (state.hub) {
    index = `${state.projects.length} project${state.projects.length > 1 ? "s" : ""}`;
    title = "Hub";
    caption = project
      ? `All your projects. ${project} is the one you are working on.`
      : "All your projects.";
  } else if (rewound && state.rewind) {
    index = `checkpoint ${state.checkpoint}`;
    title = "Rewind";
    caption = `Back to checkpoint ${state.checkpoint}: what was built since collapses`;
  } else if (kiroOn && state.kiro) {
    who = "kiro";
    whoKind = "kiro";
    caption = state.kiro.file ? `Kiro, the critic, reviews ${state.kiro.file}` : "Kiro, the critic, judges the answer";
  } else if (subsAlive > 0) {
    who = "sub-agents";
    whoKind = "sub";
    const targets = [...state.subs.values()]
      .filter((s) => s.doneAt === null)
      .map((s) => (s.target === "web" ? "the web" : s.target.path));
    caption = `${subsAlive} at work · ${targets.join(" · ")}`;
  } else if (state.mainFile) {
    caption = `${state.mainWrote ? "Writes" : "Reads"} ${state.mainFile}`;
  } else if (state.step) {
    caption = `${TITLES[state.step]}…`;
  }

  return {
    index,
    title,
    who,
    whoKind,
    caption,
    metrics: [
      { label: "Step", value: state.step ? `${stepRank(state.step)}/6` : "–", fill: state.step ? stepRank(state.step) / 6 : 0 },
      { label: "Files changed", value: String(changed), fill: Math.min(1, changed / 10) },
      { label: "Sub-agents", value: String(subsAlive), fill: Math.min(1, subsAlive / 6) },
      { label: "Slabs", value: String(slabs), fill: Math.min(1, slabs / 200) },
      { label: "Checkpoint", value: String(state.checkpoint), fill: Math.min(1, state.checkpoint / 10) },
    ],
  };
}
