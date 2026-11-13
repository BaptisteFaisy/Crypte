// layout.test.ts — la mise en place du monde est pure : elle se teste.
import { describe, expect, it } from "vitest";

import {
  build,
  buildMany,
  districtOrigin,
  layoutCenter,
  plateWidth,
  MAX_FILES_PER_FOLDER,
} from "./layout";
import type { ProjectTree } from "./types";

function tree(dirs: string[], files: [string, number, number][]): ProjectTree {
  return {
    root: "/p",
    dirs: dirs.map((p, i) => ({ path: p, parent: i > 0 ? 0 : null })),
    files: files.map(([path, dir, lines]) => ({ path, dir, lines })),
  };
}

describe("layout", () => {
  it("le projet actif est au centre, les autres alternent", () => {
    expect(districtOrigin(0)).toEqual({ x: 0, y: 0, z: 0 });
    expect(districtOrigin(1).x).toBeGreaterThan(3000);
    expect(districtOrigin(2).x).toBeLessThan(-3000);
    expect(districtOrigin(3).x).toBeGreaterThan(districtOrigin(1).x);
  });

  it("les fichiers d'un dossier sont alignés à 150 d'écart", () => {
    const l = build(tree(["src"], [["src/a.rs", 0, 10], ["src/b.rs", 0, 10]]));
    const a = l.files.get("src/a.rs")!;
    const b = l.files.get("src/b.rs")!;
    expect(b.base.x - a.base.x).toBeCloseTo(150, 5);
    expect(a.base.z).toBe(b.base.z);
  });

  it("les dossiers s'empilent en profondeur", () => {
    const l = build(
      tree(["src", "tests"], [["src/a.rs", 0, 10], ["tests/b.rs", 1, 10]]),
    );
    expect(l.dirs[0].origin.z - l.dirs[1].origin.z).toBeCloseTo(620, 5);
    expect(l.dirs[1].files).toBe(1);
  });

  it("la plaque croît avec le nombre de fichiers, puis sature", () => {
    expect(plateWidth(5)).toBeGreaterThan(plateWidth(1));
    expect(plateWidth(0)).toBe(plateWidth(1));
    expect(plateWidth(999)).toBe(plateWidth(MAX_FILES_PER_FOLDER));
  });

  it("un dossier dense est regroupé en une pile", () => {
    const files: [string, number, number][] = Array.from(
      { length: MAX_FILES_PER_FOLDER + 3 },
      (_, i) => [`src/f${String(i).padStart(2, "0")}.rs`, 0, 10],
    );
    const l = build(tree(["src"], files));
    expect(l.grouped.size).toBe(1);
  });

  it("le centre est le milieu du quartier actif", () => {
    const l = build(tree(["src"], [["src/a.rs", 0, 10]]));
    expect(layoutCenter(l).z).toBeCloseTo(-310, 5);
  });

  it("plusieurs projets : chemins préfixés pour les voisins", () => {
    const l = buildMany([
      { name: "demo", tree: tree(["src"], [["src/a.rs", 0, 10]]), active: true },
      {
        name: "other",
        tree: { ...tree(["src"], [["src/a.rs", 0, 10]]), root: "/other" },
        active: false,
      },
    ]);
    expect(l.files.has("src/a.rs")).toBe(true);
    expect(l.files.has("/other/src/a.rs")).toBe(true);
    expect(l.projects.length).toBe(2);
    expect(l.projects[0].active).toBe(true);
  });
});
