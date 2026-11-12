// types.ts — contrat du « project space » (fond 3D des projets).
// Miroir exact de `SceneEvent`/`SceneState` côté Rust (src-tauri/src/space/),
// sérialisé en snake_case avec tag `type` (serde). Tout écoute du canal
// `space-event` DOIT passer par ces types, sans duplication.

// ---------------------------------------------------------------- palette
// DESIGN.md §2 : le monde 3D a sa palette, distincte de la crypte.
export const SPACE_THEME = {
  bg: "#050505",
  fg: "#F5F5F3",
  accent: "#FF5B1F",
} as const;

/// Une dalle ≈ 150 lignes (DESIGN §6.1).
export const LINES_PER_SLAB = 150;

// ---------------------------------------------------------------- étapes
export type Step = "ask" | "context" | "think" | "act" | "critic" | "answer";

export const STEP_ORDER: Step[] = [
  "ask",
  "context",
  "think",
  "act",
  "critic",
  "answer",
];

export function stepRank(step: Step): number {
  return STEP_ORDER.indexOf(step) + 1;
}

export function stepLabel(step: Step): string {
  return step.toUpperCase();
}

// ---------------------------------------------------------------- acteurs
export type AgentRef = "main" | { sub: number };
export type AgentKind = "sub" | "web";
export type Target = { path: string } | "web";

// ---------------------------------------------------------------- arbre
export interface DirNode {
  path: string;
  parent: number | null;
}
export interface FileNode {
  path: string;
  dir: number;
  lines: number;
}
export interface ProjectTree {
  root: string;
  dirs: DirNode[];
  files: FileNode[];
}

// ---------------------------------------------------------------- événements
export interface SceneEventProjectOpen {
  type: "project_open";
  path: string;
  tree: ProjectTree;
}
export interface SceneEventPromptSent {
  type: "prompt_sent";
  chat: number;
}
export interface SceneEventStepStart {
  type: "step_start";
  step: Step;
}
export interface SceneEventContextLoad {
  type: "context_load";
  files: string[];
}
export interface SceneEventFileRead {
  type: "file_read";
  path: string;
  agent: AgentRef;
}
export interface SceneEventFileWrite {
  type: "file_write";
  path: string;
  agent: AgentRef;
  added: number;
  removed: number;
}
export interface SceneEventFileCreate {
  type: "file_create";
  path: string;
  agent: AgentRef;
}
export interface SceneEventAgentSpawn {
  type: "agent_spawn";
  id: number;
  kind: AgentKind;
  target: Target;
}
export interface SceneEventAgentDone {
  type: "agent_done";
  id: number;
  ok: boolean;
}
export interface SceneEventCriticStart {
  type: "critic_start";
  file: string | null;
}
export interface SceneEventCriticDone {
  type: "critic_done";
  file: string | null;
}
export interface SceneEventCheckpointSaved {
  type: "checkpoint_saved";
  n: number;
}
export interface SceneEventRewind {
  type: "rewind";
  to: number;
  files: string[];
}
export interface SceneEventHubOpen {
  type: "hub_open";
}
export interface SceneEventHubClose {
  type: "hub_close";
}

export type SceneEvent =
  | SceneEventProjectOpen
  | SceneEventPromptSent
  | SceneEventStepStart
  | SceneEventContextLoad
  | SceneEventFileRead
  | SceneEventFileWrite
  | SceneEventFileCreate
  | SceneEventAgentSpawn
  | SceneEventAgentDone
  | SceneEventCriticStart
  | SceneEventCriticDone
  | SceneEventCheckpointSaved
  | SceneEventRewind
  | SceneEventHubOpen
  | SceneEventHubClose;

// ---------------------------------------------------------------- état logique
export interface FileState {
  slabs: number;
  readAt: number | null;
  writeAt: number | null;
  changed: boolean;
}
export interface SubState {
  kind: AgentKind;
  target: Target;
  spawnedAt: number;
  doneAt: number | null;
  pushed: boolean;
}
export interface KiroState {
  file: string | null;
  startedAt: number;
  doneAt: number | null;
  pushed: boolean;
}
export interface RewindState {
  at: number;
  files: string[];
  from: number[];
}
export interface Project {
  root: string;
  name: string;
  tree: ProjectTree | null;
}
export interface SceneState {
  projects: Project[];
  files: Map<string, FileState>;
  subs: Map<number, SubState>;
  mainFile: string | null;
  mainWrote: boolean;
  kiro: KiroState | null;
  step: Step | null;
  hub: boolean;
  rewind: RewindState | null;
  checkpoint: number;
}

export function newSceneState(): SceneState {
  return {
    projects: [],
    files: new Map(),
    subs: new Map(),
    mainFile: null,
    mainWrote: false,
    kiro: null,
    step: null,
    hub: false,
    rewind: null,
    checkpoint: 0,
  };
}

export function slabsFor(lines: number): number {
  return Math.max(1, Math.ceil(Math.max(1, lines) / LINES_PER_SLAB));
}
