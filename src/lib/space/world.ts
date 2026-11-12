// world.ts — fabrication des objets 3D (dalles, fantômes, faisceaux, impacts).
// Un seul fichier pour la géométrie : la scène (scene.ts) ne fait que les
// placer et les animer. Couleurs : SPACE_THEME uniquement (DESIGN §2).
import * as THREE from "three";

import { SPACE_THEME } from "./types";
import { SLAB, SLAB_H, SLAB_PITCH, type Vec3 } from "./layout";

const FG = new THREE.Color(SPACE_THEME.fg);
const ACCENT = new THREE.Color(SPACE_THEME.accent);
const BG = new THREE.Color(SPACE_THEME.bg);

/** Une dalle : boîte translucide + dessus + arêtes fines. */
export interface SlabPart {
  box: THREE.Mesh;
  top: THREE.Mesh;
  edges: THREE.LineSegments;
}

/** Une pile de dalles (un fichier). */
export interface SlabStack {
  group: THREE.Group;
  parts: SlabPart[];
}

/** Une pile vide, prête à recevoir ses dalles. */
export function makeSlabStack(): SlabStack {
  return { group: new THREE.Group(), parts: [] };
}

/** Recalcule la pile : `slabs` dalles, intensités lu/actif/modifié. */
export function updateSlabStack(
  stack: SlabStack,
  slabs: number,
  read: number,
  active: number,
  changed: number,
): void {
  const want = Math.max(1, Math.min(64, slabs));
  while (stack.parts.length > want) {
    const part = stack.parts.pop();
    if (part) {
      stack.group.remove(part.box, part.top, part.edges);
      part.box.geometry.dispose();
      part.top.geometry.dispose();
      part.edges.geometry.dispose();
    }
  }
  while (stack.parts.length < want) {
    const part = makeSlab(stack.parts.length);
    stack.group.add(part.box, part.top, part.edges);
    stack.parts.push(part);
  }
  const edgeAlpha = Math.min(1, 0.3 + 0.55 * read + 0.7 * active + 0.25 * changed);
  const warm = Math.min(1, active + 0.6 * changed);
  const edgeColor = FG.clone().lerp(ACCENT, warm);
  const topAlpha = 0.025 + 0.16 * read + 0.24 * active + 0.08 * changed;
  for (const part of stack.parts) {
    const m = part.edges.material as THREE.LineBasicMaterial;
    m.color.copy(edgeColor);
    m.opacity = edgeAlpha;
    const tm = part.top.material as THREE.MeshBasicMaterial;
    tm.color.copy(warm > 0.02 ? ACCENT : FG);
    tm.opacity = topAlpha;
  }
}

function makeSlab(index: number): SlabPart {
  const y = index * SLAB_PITCH;
  const geo = new THREE.BoxGeometry(SLAB, SLAB_H, SLAB);
  const box = new THREE.Mesh(
    geo,
    new THREE.MeshBasicMaterial({
      color: FG,
      transparent: true,
      opacity: 0.04,
      depthWrite: false,
    }),
  );
  box.position.y = y + SLAB_H / 2;

  const top = new THREE.Mesh(
    new THREE.PlaneGeometry(SLAB, SLAB),
    new THREE.MeshBasicMaterial({
      color: FG,
      transparent: true,
      opacity: 0.03,
      depthWrite: false,
      side: THREE.DoubleSide,
    }),
  );
  top.rotation.x = -Math.PI / 2;
  top.position.y = y + SLAB_H;

  const edges = new THREE.LineSegments(
    new THREE.EdgesGeometry(geo),
    new THREE.LineBasicMaterial({
      color: FG,
      transparent: true,
      opacity: 0.4,
      depthWrite: false,
    }),
  );
  edges.position.y = y + SLAB_H / 2;

  return { box, top, edges };
}

/// Le chemin 2D du fantôme (DESIGN §6.3), mis à l'échelle `size`.
const GHOST_PATH = [
  [50, 6],
  [27, 6],
  [12, 23],
  [12, 46],
  [12, 104],
  [25, 95],
  [38, 104],
  [50, 95],
  [62, 104],
  [75, 95],
  [88, 104],
  [88, 46],
  [88, 23],
  [73, 6],
] as const;

/** Un fantôme 3D : silhouette extrudée + deux yeux noirs. */
export function makeGhost(size: number): THREE.Group {
  const group = new THREE.Group();
  const shape = new THREE.Shape();
  const k = size / 100;
  GHOST_PATH.forEach(([x, y], i) => {
    if (i === 0) shape.moveTo(x * k, y * k);
    else shape.lineTo(x * k, y * k);
  });
  shape.closePath();
  const geo = new THREE.ExtrudeGeometry(shape, {
    depth: 18 * k,
    bevelEnabled: true,
    bevelThickness: 9 * k,
    bevelSize: 7 * k,
    bevelSegments: 5,
  });
  geo.center();
  const body = new THREE.Mesh(
    geo,
    new THREE.MeshStandardMaterial({
      color: 0xf5f5f3,
      roughness: 0.55,
      metalness: 0,
      emissive: 0xffffff,
      emissiveIntensity: 0.35,
    }),
  );
  group.add(body);
  for (const s of [-1, 1]) {
    const eye = new THREE.Mesh(
      new THREE.SphereGeometry(1, 16, 12),
      new THREE.MeshBasicMaterial({ color: BG }),
    );
    eye.scale.set(5 * k, 7 * k, 2 * k);
    eye.position.set(s * 13 * k, 8 * k, 9 * k);
    group.add(eye);
  }
  return group;
}

/// Les cornes orange de Kiro.
export function makeHorns(size: number): THREE.Group {
  const group = new THREE.Group();
  const k = size / 100;
  for (const s of [-1, 1]) {
    const horn = new THREE.Mesh(
      new THREE.ConeGeometry(5.5 * k, 18 * k, 14),
      new THREE.MeshBasicMaterial({ color: ACCENT }),
    );
    horn.position.set(s * 22 * k, 46 * k, 0);
    horn.rotation.z = s * -0.35;
    group.add(horn);
  }
  return group;
}

/// Un faisceau (ligne) : blanc en lecture, orange en écriture.
export function makeBeam(): THREE.Line {
  const geo = new THREE.BufferGeometry().setFromPoints([
    new THREE.Vector3(),
    new THREE.Vector3(),
  ]);
  return new THREE.Line(
    geo,
    new THREE.LineBasicMaterial({
      color: FG,
      transparent: true,
      opacity: 0,
      depthWrite: false,
    }),
  );
}

/// Le halo radial au sol (sous un fichier lu/écrit).
export function makeGroundGlow(): THREE.Mesh {
  const mat = new THREE.MeshBasicMaterial({
    map: radialTexture(),
    color: FG,
    transparent: true,
    opacity: 0,
    depthWrite: false,
    blending: THREE.AdditiveBlending,
  });
  const mesh = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), mat);
  mesh.rotation.x = -Math.PI / 2;
  return mesh;
}

/// Impact au sol : anneau orange, anneau blanc, flash, poussières.
export interface ImpactObject {
  group: THREE.Group;
  ring: THREE.Mesh;
  white: THREE.Mesh;
  flash: THREE.Mesh;
  dust: THREE.Points;
}

export function makeImpact(): ImpactObject {
  const group = new THREE.Group();
  const ringGeo = new THREE.RingGeometry(0.92, 1, 96);
  const ring = new THREE.Mesh(
    ringGeo,
    new THREE.MeshBasicMaterial({
      color: ACCENT,
      transparent: true,
      opacity: 0,
      side: THREE.DoubleSide,
      depthWrite: false,
    }),
  );
  ring.rotation.x = -Math.PI / 2;
  const white = new THREE.Mesh(
    ringGeo,
    new THREE.MeshBasicMaterial({
      color: FG,
      transparent: true,
      opacity: 0,
      side: THREE.DoubleSide,
      depthWrite: false,
    }),
  );
  white.rotation.x = -Math.PI / 2;
  const flash = new THREE.Mesh(
    new THREE.PlaneGeometry(1, 1),
    new THREE.MeshBasicMaterial({
      map: radialTexture(),
      color: ACCENT,
      transparent: true,
      opacity: 0,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
    }),
  );
  flash.rotation.x = -Math.PI / 2;
  const dustPos = new Float32Array(26 * 3);
  const dust = new THREE.Points(
    new THREE.BufferGeometry().setAttribute(
      "position",
      new THREE.BufferAttribute(dustPos, 3),
    ),
    new THREE.PointsMaterial({
      color: FG,
      size: 5,
      transparent: true,
      opacity: 0,
      depthWrite: false,
    }),
  );
  group.add(ring, white, flash, dust);
  return { group, ring, white, flash, dust };
}

/// Une étiquette (sprite sur texture canvas).
export function makeLabel(text: string): THREE.Sprite {
  const canvas = document.createElement("canvas");
  canvas.width = 256;
  canvas.height = 48;
  const ctx = canvas.getContext("2d");
  if (ctx) {
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.font = "600 26px ui-monospace, monospace";
    ctx.fillStyle = SPACE_THEME.fg;
    ctx.textBaseline = "top";
    ctx.fillText(text.toUpperCase(), 4, 6);
  }
  const texture = new THREE.CanvasTexture(canvas);
  const sprite = new THREE.Sprite(
    new THREE.SpriteMaterial({
      map: texture,
      transparent: true,
      opacity: 0.5,
      depthWrite: false,
    }),
  );
  sprite.scale.set(120, 22, 1);
  return sprite;
}

/// Le contour d'une plaque de dossier.
export function makePlate(w: number, d: number): THREE.Line {
  const geo = new THREE.BufferGeometry().setFromPoints([
    new THREE.Vector3(0, 0, 0),
    new THREE.Vector3(w, 0, 0),
    new THREE.Vector3(w, 0, d),
    new THREE.Vector3(0, 0, d),
    new THREE.Vector3(0, 0, 0),
  ]);
  return new THREE.Line(
    geo,
    new THREE.LineBasicMaterial({
      color: FG,
      transparent: true,
      opacity: 0.16,
      depthWrite: false,
    }),
  );
}

/// Le cadre pointillé du projet actif (« you are here »).
export function makeDistrictFrame(w: number, d: number): THREE.LineSegments {
  const pts: THREE.Vector3[] = [];
  const corners = [
    new THREE.Vector3(0, 1, 0),
    new THREE.Vector3(w, 1, 0),
    new THREE.Vector3(w, 1, d),
    new THREE.Vector3(0, 1, d),
  ];
  for (let i = 0; i < 4; i++) {
    const a = corners[i];
    const b = corners[(i + 1) % 4];
    const len = a.distanceTo(b);
    const steps = Math.max(4, Math.floor(len / 26));
    for (let k = 0; k < steps; k += 2) {
      const t0 = k / steps;
      const t1 = Math.min(1, (k + 1) / steps);
      pts.push(a.clone().lerp(b, t0), a.clone().lerp(b, t1));
    }
  }
  const geo = new THREE.BufferGeometry().setFromPoints(pts);
  return new THREE.LineSegments(
    geo,
    new THREE.LineBasicMaterial({
      color: ACCENT,
      transparent: true,
      opacity: 0.8,
      depthWrite: false,
    }),
  );
}

/// Convertit une position du layout en Vector3.
export function toVec3(p: Vec3): THREE.Vector3 {
  return new THREE.Vector3(p.x, p.y, p.z);
}

/// Texture radiale (halo), partagée.
let radial: THREE.Texture | null = null;
function radialTexture(): THREE.Texture {
  if (radial) return radial;
  const size = 64;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d");
  if (ctx) {
    const g = ctx.createRadialGradient(
      size / 2,
      size / 2,
      0,
      size / 2,
      size / 2,
      size / 2,
    );
    g.addColorStop(0, "rgba(255,255,255,1)");
    g.addColorStop(1, "rgba(255,255,255,0)");
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, size, size);
  }
  radial = new THREE.CanvasTexture(canvas);
  return radial;
}

/// Libère géométries et matériaux d'un objet.
export function disposeObject(obj: THREE.Object3D): void {
  obj.traverse((child) => {
    const mesh = child as THREE.Mesh;
    if (mesh.geometry) mesh.geometry.dispose();
    const mat = mesh.material;
    if (Array.isArray(mat)) mat.forEach((m) => m.dispose());
    else if (mat) mat.dispose();
  });
}
