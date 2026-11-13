// hud.test.ts — les calques au-dessus de la 3D suivent l'état de scène.
import { describe, expect, it } from "vitest";
import { hudOf } from "./hud";
import { apply } from "./state";
import { demoTree } from "./demo";
import { newSceneState } from "./types";

function opened() {
  const s = newSceneState();
  apply(s, { type: "project_open", path: "/demo", tree: demoTree() }, 0);
  return s;
}

describe("hudOf", () => {
  it("au repos : Ready, le fantôme attend", () => {
    const h = hudOf(opened(), 0);
    expect(h.title).toBe("Ready");
    expect(h.whoKind).toBe("main");
    expect(h.caption).toMatch(/Waits/);
  });

  it("étape act + écriture : titre Act, légende Writes <fichier>", () => {
    const s = opened();
    apply(s, { type: "step_start", step: "act" }, 1);
    apply(s, { type: "file_write", path: "src/mod1/f05.rs", agent: "main", added: 4, removed: 1 }, 1.2);
    const h = hudOf(s, 1.5);
    expect(h.title).toBe("Act");
    expect(h.index).toBe("04 / 06");
    expect(h.caption).toBe("Writes src/mod1/f05.rs");
  });

  it("un sous-agent au travail prend la légende", () => {
    const s = opened();
    apply(s, { type: "agent_spawn", id: 1, kind: "sub", target: { path: "src/mod2/f10.rs" } }, 1);
    const h = hudOf(s, 1.5);
    expect(h.whoKind).toBe("sub");
    expect(h.caption).toContain("src/mod2/f10.rs");
    expect(h.metrics.find((m) => m.label === "Sub-agents")?.value).toBe("1");
  });

  it("hub : titre Hub", () => {
    const s = opened();
    apply(s, { type: "hub_open" }, 2);
    expect(hudOf(s, 2.1).title).toBe("Hub");
  });
});
