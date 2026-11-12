// layout.ts — mise en place du monde 3D (positions pures, testables).
// Miroir TS de `src/space/layout.rs` du port terminal : mêmes constantes,
// mêmes règles (rangées de dossiers, fichiers en ligne, districts alternés,
// regroupement des dossiers denses). Le rendu three.js et le rendu Rust
// concordent ainsi frame pour frame.
import type { ProjectTree } from "./types";

export const SLAB = 110;
export const SLAB_H = 12;
export const SLAB_PITCH = 22;
export const FILE_PITCH = 150;
export const FOLDER_PAD = 85;
export const ROW_PITCH = 620;
export const DISTRICT_PITCH = 4500;
export const MAX_FILES_PER_FOLDER = 24;

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}
export const v3 = (x = 0, y = 0, z = 0): Vec3 => ({ x, y, z });

export interface DirBox {
  path: string;
  origin: Vec3;
  w: number;
  d: number;
  files: number;
}
export interface FileBox {
  base: Vec3;
  dir: number;
}
export interface ProjectPlate {
  name: string;
  origin: Vec3;
  w: number;
  d: number;
  active: boolean;
}
export interface WorldLayout {
  dirs: DirBox[];
  files: Map<string, FileBox>;
  grouped: Map<number, FileBox>;
  projects: ProjectPlate[];
}

/// Origine du quartier d'un projet (0 = actif au centre, alternés ensuite).
export function districtOrigin(index: number): Vec3 {
  if (index === 0) return v3();
  const ring = Math.floor((index + 1) / 2);
  const side = index % 2 === 1 ? 1 : -1;
  return v3(side * DISTRICT_PITCH * ring, 0, 0);
}

/// Largeur d'une plaque pour `n` fichiers (bornée au regroupement).
export function plateWidth(n: number): number {
  const c = Math.min(Math.max(n, 1), MAX_FILES_PER_FOLDER);
  return FOLDER_PAD * 2 + (c - 1) * FILE_PITCH + SLAB;
}

/// Construit le monde d'un seul projet (l'actif).
export function build(tree: ProjectTree): WorldLayout {
  const name = tree.root.split("/").filter(Boolean).pop() ?? tree.root;
  return buildMany([{ name, tree, active: true }]);
}

/// Construit plusieurs quartiers : un par projet.
export function buildMany(
  entries: { name: string; tree: ProjectTree; active: boolean }[],
): WorldLayout {
  const out: WorldLayout = {
    dirs: [],
    files: new Map(),
    grouped: new Map(),
    projects: [],
  };
  entries.forEach((entry, i) => {
    const off = districtOrigin(i);
    const dirs = dirBoxes(entry.tree, off);
    const base = out.dirs.length;
    for (const [file, rank] of ranked(entry.tree)) {
      const dir = dirs[file.dir];
      if (!dir) continue;
      const key = entry.active ? file.path : joinPath(entry.tree.root, file.path);
      out.files.set(key, slotOf(dir, rank, base + file.dir));
      const g = base + file.dir;
      if (dir.files > MAX_FILES_PER_FOLDER && !out.grouped.has(g)) {
        out.grouped.set(g, groupCenter(dir, g));
      }
    }
    const [w, d] = bounds(dirs);
    out.projects.push({ name: entry.name, origin: off, w, d, active: entry.active });
    out.dirs.push(...dirs);
  });
  return out;
}

/// Centre du quartier actif (cible de caméra au repos).
export function layoutCenter(layout: WorldLayout): Vec3 {
  const p = layout.projects.find((q) => q.active) ?? layout.projects[0];
  if (!p) return v3();
  return v3(p.origin.x + p.w / 2, 0, p.origin.z - p.d / 2);
}

function joinPath(root: string, rel: string): string {
  return `${root.replace(/\/$/, "")}/${rel}`;
}

function dirBoxes(tree: ProjectTree, off: Vec3): DirBox[] {
  return tree.dirs.map((dir, i) => {
    const n = tree.files.filter((f) => f.dir === i).length;
    return {
      path: dir.path,
      origin: v3(off.x, 0, off.z - i * ROW_PITCH),
      w: plateWidth(n),
      d: SLAB + FOLDER_PAD * 2,
      files: n,
    };
  });
}

/// Fichiers triés par dossier puis par chemin, avec leur rang dans le dossier.
function ranked(tree: ProjectTree): [typeof tree.files[number], number][] {
  const out: [typeof tree.files[number], number][] = [];
  tree.dirs.forEach((_, i) => {
    const siblings = tree.files.filter((f) => f.dir === i).sort((a, b) => a.path.localeCompare(b.path));
    siblings.forEach((f, r) => out.push([f, r]));
  });
  return out;
}

function slotOf(dir: DirBox, rank: number, dirIndex: number): FileBox {
  const r = Math.min(rank, MAX_FILES_PER_FOLDER - 1);
  return {
    base: v3(
      dir.origin.x + FOLDER_PAD + SLAB / 2 + r * FILE_PITCH,
      0,
      dir.origin.z + FOLDER_PAD + SLAB / 2,
    ),
    dir: dirIndex,
  };
}

function groupCenter(dir: DirBox, dirIndex: number): FileBox {
  return { base: v3(dir.origin.x + dir.w / 2, 0, dir.origin.z + dir.d / 2), dir: dirIndex };
}

function bounds(dirs: DirBox[]): [number, number] {
  const w = dirs.reduce((m, b) => Math.max(m, b.w), 0);
  return [Math.max(w, SLAB), dirs.length * ROW_PITCH];
}
