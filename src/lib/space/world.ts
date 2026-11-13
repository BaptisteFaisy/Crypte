// world.ts — fabrication des objets 3D (dalles, fantômes, faisceaux, impacts).
// Un seul fichier pour la géométrie : la scène (scene.ts) ne fait que les
// placer et les animer. Couleurs : SPACE_THEME uniquement (DESIGN §2).
import * as THREE from "three";

import { SPACE_THEME } from "./types";
import { SLAB, SLAB_H, SLAB_PITCH, type Vec3 } from "./layout";

const FG = new THREE.Color(SPACE_THEME.fg);
const ACCENT = new THREE.Color(SPACE_THEME.accent);
const BG = new THREE.Color(SPACE_THEME.bg);

/// Les opacités de DESIGN.md sont des opacités « vues » (mélange en sRGB, comme
/// le prototype). three.js mélange en linéaire : un blanc à 4 % y ressort
/// comme ~20 % de gris. On convertit pour garder le rendu du design.
export function seen(alpha: number): number {
  return Math.pow(Math.max(0, Math.min(1, alpha)), 2.2);
}

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
    m.opacity = seen(edgeAlpha);
    const tm = part.top.material as THREE.MeshBasicMaterial;
    tm.color.copy(warm > 0.02 ? ACCENT : FG);
    tm.opacity = seen(topAlpha);
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
      opacity: seen(0.04),
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

/// Silhouette 2D du fantôme (DESIGN §6.3), coordonnées du SVG (y vers le bas).
/// L'axe Y est retourné (y' = 104 − y) : sinon l'ondulation passe en haut.
/// La tête garde ses deux courbes de Bézier : en segments, elle devient anguleuse.
function ghostShape(): THREE.Shape {
  const X = (x: number) => x - 50;
  const Y = (y: number) => 104 - y;
  const s = new THREE.Shape();
  s.moveTo(X(50), Y(6));
  s.bezierCurveTo(X(27), Y(6), X(12), Y(23), X(12), Y(46));
  s.lineTo(X(12), Y(104));
  s.lineTo(X(25), Y(95));
  s.lineTo(X(38), Y(104));
  s.lineTo(X(50), Y(95));
  s.lineTo(X(62), Y(104));
  s.lineTo(X(75), Y(95));
  s.lineTo(X(88), Y(104));
  s.lineTo(X(88), Y(46));
  s.bezierCurveTo(X(88), Y(23), X(73), Y(6), X(50), Y(6));
  return s;
}

/// Géométrie partagée, à l'échelle 100 (largeur), centrée sur son volume.
let ghostGeo: THREE.ExtrudeGeometry | null = null;
function ghostGeometry(): THREE.ExtrudeGeometry {
  if (ghostGeo) return ghostGeo;
  ghostGeo = new THREE.ExtrudeGeometry(ghostShape(), {
    depth: 18,
    bevelEnabled: true,
    bevelThickness: 9,
    bevelSize: 7,
    bevelSegments: 5,
    curveSegments: 24,
  });
  ghostGeo.center();
  ghostGeo.computeVertexNormals();
  return ghostGeo;
}

/// Hauteur des yeux après centrage : y' = 104 − 48 = 56, centre de la boîte ≈ 49.
const EYE_Y = 7;
/// Face avant après centrage : profondeur 18 + biseau 9 de chaque côté → ±18.
const EYE_Z = 18.6;

/** Un fantôme 3D : silhouette extrudée mate + deux yeux noirs posés sur la face avant. */
export function makeGhost(size: number): THREE.Group {
  const group = new THREE.Group();
  const inner = new THREE.Group();
  inner.scale.setScalar(size / 100);
  const body = new THREE.Mesh(
    ghostGeometry(),
    new THREE.MeshStandardMaterial({
      color: 0xdeded9,
      roughness: 0.55,
      metalness: 0,
      emissive: 0xffffff,
      emissiveIntensity: 0.06,
    }),
  );
  inner.add(body);
  for (const s of [-1, 1]) {
    const eye = new THREE.Mesh(
      new THREE.SphereGeometry(1, 20, 14),
      new THREE.MeshBasicMaterial({ color: BG }),
    );
    eye.scale.set(6.5, 8.5, 2.2);
    eye.position.set(s * 11, EYE_Y, EYE_Z);
    inner.add(eye);
  }
  group.add(inner);
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
    horn.position.set(s * 16 * k, 56 * k, 0);
    horn.rotation.z = s * -0.4;
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
  const ringGeo = new THREE.RingGeometry(0.975, 1, 128);
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

/// Style d'étiquette (DESIGN §6.1) : fichier en minuscules 10.5 px, dossier
/// en majuscules 9.5 px posé au sol, projet en 15 px.
export type LabelKind = "file" | "dir" | "project" | "here";

const LABEL_FONT = '"Geist Mono", ui-monospace, "Cascadia Mono", Menlo, monospace';
const LABEL_PX: Record<LabelKind, number> = { file: 10.5, dir: 9.5, project: 15, here: 11 };

/// Une étiquette à taille constante à l'écran (elle ne grossit pas près de
/// la caméra). Texture dessinée en 4× pour rester nette.
export function makeLabel(text: string, kind: LabelKind = "file"): THREE.Sprite {
  const px = LABEL_PX[kind];
  const dpr = 4;
  const label = kind === "dir" ? text.toUpperCase() : text;
  const font = `${kind === "project" ? 600 : 500} ${px * dpr}px ${LABEL_FONT}`;
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d");
  let w = 64;
  if (ctx) {
    ctx.font = font;
    w = Math.ceil(ctx.measureText(label).width + (kind === "dir" ? label.length * 0.12 * px * dpr : 0)) + 8;
  }
  const h = Math.ceil(px * dpr * 1.5);
  canvas.width = w;
  canvas.height = h;
  if (ctx) {
    ctx.font = font;
    ctx.textBaseline = "middle";
    ctx.fillStyle = kind === "here" ? SPACE_THEME.accent : SPACE_THEME.fg;
    if (kind === "dir" && "letterSpacing" in ctx) {
      (ctx as CanvasRenderingContext2D & { letterSpacing: string }).letterSpacing = `${0.12 * px * dpr}px`;
    }
    ctx.fillText(label, 4, h / 2);
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.minFilter = THREE.LinearFilter;
  const sprite = new THREE.Sprite(
    new THREE.SpriteMaterial({
      map: texture,
      transparent: true,
      opacity: kind === "dir" ? 0.4 : 0.6,
      depthWrite: false,
      depthTest: false,
      sizeAttenuation: false,
    }),
  );
  // sizeAttenuation=false : l'échelle est en unités écran (hauteur de la
  // vue ≈ 2 / f). On vise `h / dpr` px sur une vue de 900 px de haut.
  sprite.userData.labelPx = { w: w / dpr, h: h / dpr };
  sprite.center.set(0.5, 0);
  sizeLabel(sprite, 900);
  return sprite;
}

/// Recalcule l'échelle d'une étiquette pour une vue de `viewH` px de haut.
export function sizeLabel(sprite: THREE.Sprite, viewH: number, fovDeg = 42): void {
  const d = sprite.userData.labelPx as { w: number; h: number } | undefined;
  if (!d) return;
  const f = 1 / Math.tan((fovDeg * Math.PI) / 360);
  const unit = 2 / (viewH * f);
  sprite.scale.set(d.w * unit, d.h * unit, 1);
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
      opacity: seen(0.16),
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
