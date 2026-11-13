// state.test.ts — le passage événements → état de scène doit être testé
// (exigence du BRIEF). Miroir des tests Rust de `src/space/scene.rs`.
import { describe, expect, it } from "vitest";

import { apply, envelope, intensities } from "./state";
import {
  newSceneState,
  type ProjectTree,
  type SceneEvent,
} from "./types";

function tree(): ProjectTree {
  return {
    root: "/p",
    dirs: [{ path: "src", parent: null }],
    files: [{ path: "src/a.rs", dir: 0, lines: 300 }],
  };
}

function open(): ReturnType<typeof newSceneState> {
  const s = newSceneState();
  apply(s, { type: "project_open", path: "/p", tree: tree() }, 0);
  return s;
}

describe("apply", () => {
  it("pose l'arbre du projet", () => {
    const s = open();
    expect(s.projects[0].name).toBe("p");
    expect(s.files.get("src/a.rs")?.slabs).toBe(2);
  });

  it("lecture : faisceau blanc et fichier courant", () => {
    const s = open();
    apply(s, { type: "file_read", path: "src/a.rs", agent: "main" }, 1);
    const f = s.files.get("src/a.rs")!;
    expect(f.readAt).toBe(1);
    expect(s.mainFile).toBe("src/a.rs");
    expect(s.mainWrote).toBe(false);
  });

  it("écriture : orange, aura et dalles gagnées", () => {
    const s = open();
    const before = s.files.get("src/a.rs")!.slabs;
    apply(
      s,
      { type: "file_write", path: "src/a.rs", agent: "main", added: 400, removed: 10 },
      2,
    );
    const f = s.files.get("src/a.rs")!;
    expect(f.changed).toBe(true);
    expect(f.writeAt).toBe(2);
    expect(f.slabs).toBe(before + 2);
    expect(s.mainWrote).toBe(true);
  });

  it("création : une dalle", () => {
    const s = open();
    apply(s, { type: "file_create", path: "src/neuf.rs", agent: "main" }, 3);
    expect(s.files.get("src/neuf.rs")?.slabs).toBe(1);
  });

  it("sous-agent : poussé à sa fin", () => {
    const s = open();
    apply(
      s,
      { type: "agent_spawn", id: 7, kind: "sub", target: { path: "src/a.rs" } },
      1,
    );
    expect(s.subs.get(7)?.doneAt).toBeNull();
    apply(s, { type: "agent_done", id: 7, ok: true }, 4);
    expect(s.subs.get(7)?.doneAt).toBe(4);
    expect(s.subs.get(7)?.pushed).toBe(true);
  });

  it("kiro : perchoir puis poussé", () => {
    const s = open();
    apply(s, { type: "critic_start", file: "src/a.rs" }, 1);
    expect(s.kiro?.doneAt).toBeNull();
    apply(s, { type: "critic_done", file: null }, 5);
    expect(s.kiro?.doneAt).toBe(5);
    expect(s.kiro?.file).toBe("src/a.rs");
  });

  it("rewind : effondre les dalles et garde les comptes d'avant", () => {
    const s = open();
    apply(
      s,
      { type: "file_write", path: "src/a.rs", agent: "main", added: 900, removed: 0 },
      1,
    );
    const grown = s.files.get("src/a.rs")!.slabs;
    expect(grown).toBeGreaterThan(2);
    apply(s, { type: "rewind", to: 3, files: ["src/a.rs"] }, 9);
    expect(s.rewind?.from).toEqual([grown]);
    expect(s.files.get("src/a.rs")?.slabs).toBe(1);
    expect(s.files.get("src/a.rs")?.changed).toBe(false);
    expect(s.checkpoint).toBe(3);
  });

  it("hub et étapes", () => {
    const s = open();
    apply(s, { type: "hub_open" }, 1);
    expect(s.hub).toBe(true);
    apply(s, { type: "hub_close" }, 2);
    expect(s.hub).toBe(false);
    apply(s, { type: "step_start", step: "act" }, 3);
    expect(s.step).toBe("act");
    apply(s, { type: "prompt_sent", chat: 0 }, 4);
    expect(s.step).toBe("ask");
    apply(s, { type: "checkpoint_saved", n: 5 }, 5);
    expect(s.checkpoint).toBe(5);
  });
});

describe("enveloppes", () => {
  it("monte puis redescend", () => {
    expect(envelope(null, 1)).toBe(0);
    expect(envelope(0, -1)).toBe(0);
    expect(envelope(0, 0.15)).toBeCloseTo(0.5, 5);
    expect(envelope(0, 0.3)).toBe(1);
    expect(envelope(0, 0.9)).toBe(0);
  });

  it("intensités d'un fichier inconnu", () => {
    expect(intensities(undefined, 1)).toEqual({ read: 0, active: 0, changed: 0 });
  });
});

describe("contrat de sérialisation", () => {
  it("accepte la forme serde snake_case venue de Rust", () => {
    const raw =
      '{"type":"file_write","path":"src/a.rs","agent":"main","added":150,"removed":0}';
    const ev = JSON.parse(raw) as SceneEvent;
    const s = open();
    apply(s, ev, 1);
    expect(s.mainWrote).toBe(true);
  });

  it("accepte un sous-agent en cible web", () => {
    const raw = '{"type":"agent_spawn","id":2,"kind":"web","target":"web"}';
    const ev = JSON.parse(raw) as SceneEvent;
    const s = open();
    apply(s, ev, 0);
    expect(s.subs.get(2)?.target).toBe("web");
  });
});
