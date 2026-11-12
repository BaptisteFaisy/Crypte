// state.ts — pont événements → état logique du monde 3D (DESIGN §7).
// Port TS de `src/space/scene.rs` du TUI : même sémantique, mêmes durées.
// Le rendu three.js (`scene.ts`) ne lit QUE cet état : aucun couplage
// entre la logique de l'agent et le dessin.

import {
  slabsFor,
  type FileState,
  type SceneEvent,
  type SceneState,
} from "./types";

/// Enveloppe d'un fichier lu/écrit : 0,3 s de montée, 0,6 s de descente.
export const FADE_IN = 0.3;
export const FADE_OUT = 0.6;

/// Plie un événement dans l'état logique (muté sur place, comme en Rust).
export function apply(state: SceneState, ev: SceneEvent, now: number): void {
  switch (ev.type) {
    case "project_open":
      openProject(state, ev.path, ev.tree);
      break;
    case "prompt_sent":
      state.step = "ask";
      break;
    case "step_start":
      state.step = ev.step;
      break;
    case "context_load":
      for (const f of ev.files) touchRead(state, f, now);
      break;
    case "file_read":
      touchRead(state, ev.path, now);
      if (ev.agent === "main") {
        state.mainFile = ev.path;
        state.mainWrote = false;
      }
      break;
    case "file_write":
      touchWrite(state, ev.path, now, ev.added);
      if (ev.agent === "main") {
        state.mainFile = ev.path;
        state.mainWrote = true;
      }
      break;
    case "file_create": {
      const f = fileOf(state, ev.path);
      f.slabs = 1;
      touchWrite(state, ev.path, now, 0);
      if (ev.agent === "main") {
        state.mainFile = ev.path;
        state.mainWrote = true;
      }
      break;
    }
    case "agent_spawn":
      state.subs.set(ev.id, {
        kind: ev.kind,
        target: ev.target,
        spawnedAt: now,
        doneAt: null,
        pushed: false,
      });
      break;
    case "agent_done": {
      const s = state.subs.get(ev.id);
      if (s) {
        s.doneAt = now;
        s.pushed = true;
      }
      break;
    }
    case "critic_start":
      state.kiro = { file: ev.file, startedAt: now, doneAt: null, pushed: false };
      break;
    case "critic_done":
      if (state.kiro) {
        if (state.kiro.file === null) state.kiro.file = ev.file;
        state.kiro.doneAt = now;
        state.kiro.pushed = true;
      }
      break;
    case "checkpoint_saved":
      state.checkpoint = ev.n;
      break;
    case "rewind": {
      const from = ev.files.map(
        (f) => state.files.get(f)?.slabs ?? 1,
      );
      state.checkpoint = ev.to;
      state.rewind = { at: now, files: [...ev.files], from };
      for (const f of ev.files) {
        const fs = state.files.get(f);
        if (fs) {
          fs.changed = false;
          fs.slabs = 1;
        }
      }
      break;
    }
    case "hub_open":
      state.hub = true;
      break;
    case "hub_close":
      state.hub = false;
      break;
  }
}

/// Ouvre (ou rouvre) le projet actif : son arbre alimente les dalles.
export function openProject(
  state: SceneState,
  path: string,
  tree: import("./types").ProjectTree,
): void {
  state.projects = state.projects.filter((p) => p.root !== path);
  state.projects.unshift({
    root: path,
    name: path.split("/").filter(Boolean).pop() ?? path,
    tree,
  });
  state.files.clear();
  for (const f of tree.files) {
    state.files.set(f.path, {
      slabs: slabsFor(f.lines),
      readAt: null,
      writeAt: null,
      changed: false,
    });
  }
}

function fileOf(state: SceneState, path: string): FileState {
  let f = state.files.get(path);
  if (!f) {
    f = { slabs: 1, readAt: null, writeAt: null, changed: false };
    state.files.set(path, f);
  }
  return f;
}

function touchRead(state: SceneState, path: string, now: number): void {
  fileOf(state, path).readAt = now;
}

function touchWrite(
  state: SceneState,
  path: string,
  now: number,
  added: number,
): void {
  const f = fileOf(state, path);
  f.writeAt = now;
  f.changed = true;
  f.slabs += Math.floor(added / 150);
}

/// Intensités 0..1 d'un fichier à `now` : lecture, écriture, modification.
export function intensities(
  f: FileState | undefined,
  now: number,
): { read: number; active: number; changed: number } {
  if (!f) return { read: 0, active: 0, changed: 0 };
  return {
    read: envelope(f.readAt, now),
    active: envelope(f.writeAt, now),
    changed: f.changed ? 1 : 0,
  };
}

/// Enveloppe d'un événement daté (montée puis descente).
export function envelope(at: number | null, now: number): number {
  if (at === null) return 0;
  const dt = now - at;
  if (dt < 0) return 0;
  if (dt < FADE_IN) return dt / FADE_IN;
  return Math.max(0, 1 - (dt - FADE_IN) / FADE_OUT);
}
