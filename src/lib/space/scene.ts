// scene.ts — le monde 3D « project space » en three.js (webview).
// Port du prototype `crypt-lab.html` + du rendu Rust : mêmes constantes
// (DESIGN §6-§8). Ne lit QUE `SceneState` + un temps : aucun événement brut.
import * as THREE from "three";
import { EffectComposer } from "three/examples/jsm/postprocessing/EffectComposer.js";
import { RenderPass } from "three/examples/jsm/postprocessing/RenderPass.js";
import { UnrealBloomPass } from "three/examples/jsm/postprocessing/UnrealBloomPass.js";
import { OutputPass } from "three/examples/jsm/postprocessing/OutputPass.js";

import {
  buildMany,
  layoutCenter,
  type Vec3,
  type WorldLayout,
} from "./layout";
import { intensities } from "./state";
import { SPACE_THEME, type SceneState } from "./types";
import {
  disposeObject,
  makeBeam,
  makeDistrictFrame,
  makeGhost,
  makeGroundGlow,
  makeHorns,
  makeImpact,
  makeLabel,
  makePlate,
  makeSlabStack,
  seen,
  sizeLabel,
  toVec3,
  updateSlabStack,
  type ImpactObject,
  type SlabStack,
} from "./world";

export interface SpaceScene {
  frame(state: SceneState, now: number, dt: number): void;
  resize(width: number, height: number): void;
  dispose(): void;
}

const FOV = 42;
const YAW = 0.62;
const deg = (d: number) => (d * Math.PI) / 180;
const CLOSE = { dist: 900, pitch: deg(20) };
const OVERVIEW = { dist: 1850, pitch: deg(27) };
const HUB = { dist: 10500, pitch: deg(38) };
const PUSH = { vx: 260, vy: 200, g: 900 };
const FLY = { file: 1.2, web: 2.2, arc: 140 };

interface Fall {
  from: THREE.Vector3;
  started: number;
  dir: number;
  impacted: boolean;
}

export function createSpaceScene(canvas: HTMLCanvasElement): SpaceScene {
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
  // Fond transparent : le noir vient du CSS du canvas (#050505). Une couleur
  // de fond opaque passait deux fois en sRGB dans le composer et sortait
  // en #262626 (gris) au lieu de #050505 (DESIGN §2).
  renderer.setClearColor(new THREE.Color(SPACE_THEME.bg), 0);
  const scene = new THREE.Scene();
  scene.fog = new THREE.Fog(new THREE.Color(SPACE_THEME.bg), 180, 1500);
  const camera = new THREE.PerspectiveCamera(FOV, 1, 10, 80000);
  scene.add(new THREE.HemisphereLight(0xffffff, 0x161616, 0.62));
  const sun = new THREE.DirectionalLight(0xffffff, 0.7);
  sun.position.set(500, 900, 800);
  scene.add(sun);
  const composer = new EffectComposer(renderer);
  composer.addPass(new RenderPass(scene, camera));
  composer.addPass(new UnrealBloomPass(new THREE.Vector2(480, 270), 0.7, 0.5, 0.62));
  // Sans OutputPass, le composer sort du linéaire non converti : le noir
  // #050505 ressortait gris. OutputPass applique l'espace sRGB (DESIGN §2).
  composer.addPass(new OutputPass());

  const world = new THREE.Group();
  scene.add(world);
  const floor = makeFloor();
  scene.add(floor);
  const mainGhost = makeGhost(60);
  const mainBeam = makeBeam();
  const mainGlow = makeGroundGlow();
  scene.add(mainGhost, mainBeam, mainGlow);
  const kiro = makeGhost(36);
  kiro.add(makeHorns(36));
  scene.add(kiro);

  let layout: WorldLayout | null = null;
  let layoutKey = "";
  const stacks = new Map<string, SlabStack>();
  const glows = new Map<string, THREE.Mesh>();
  const labels = new Map<string, THREE.Sprite>();
  const allLabels: THREE.Sprite[] = [];
  let viewH = 900;
  const subGhosts = new Map<number, THREE.Group>();
  const falls = new Map<number, Fall>();
  const impacts: { obj: ImpactObject; at: number; pos: THREE.Vector3; big: boolean }[] = [];
  const cam = {
    dist: 300,
    pitch: deg(3),
    target: new THREE.Vector3(),
    shake: 0,
    punch: 0,
    lastRewind: -1,
  };

  function rebuild(state: SceneState): void {
    clearWorld();
    layout = buildMany(
      state.projects
        .filter((p) => p.tree)
        .map((p, i) => ({ name: p.name, tree: p.tree!, active: i === 0 })),
    );
    layoutKey = keyOf(state);
    buildStatic(layout);
  }

  function frame(state: SceneState, now: number, dt: number): void {
    if (!layout || keyOf(state) !== layoutKey) rebuild(state);
    const lay = layout!;
    updateFiles(state, now, lay);
    updateMain(state, now, lay);
    updateSubs(state, now, lay);
    updateKiro(state, now, lay);
    updateImpacts(now);
    updateCamera(state, now, dt, lay);
    composer.render();
  }

  function resize(width: number, height: number): void {
    renderer.setSize(width, height, false);
    composer.setSize(width, height);
    viewH = Math.max(1, height);
    for (const l of allLabels) sizeLabel(l, viewH, FOV);
    camera.aspect = width / Math.max(1, height);
    // Le chat occupe la gauche (DESIGN §3) : on décale le centre de la vue
    // vers la droite pour que la scène tombe dans la zone libre.
    const shift = width > 900 ? Math.min(width * 0.2, 360) : 0;
    if (shift > 0) camera.setViewOffset(width, height, -shift, 0, width, height);
    else camera.clearViewOffset();
    camera.updateProjectionMatrix();
  }

  function dispose(): void {
    clearWorld();
    disposeObject(world);
    disposeObject(floor);
    renderer.dispose();
  }

  return { frame, resize, dispose };

  // ---------------------------------------------------------- monde statique
  function clearWorld(): void {
    for (const child of [...world.children]) {
      world.remove(child);
      disposeObject(child);
    }
    stacks.clear();
    glows.clear();
    labels.clear();
    allLabels.length = 0;
  }

  function addLabel(sprite: THREE.Sprite): THREE.Sprite {
    sizeLabel(sprite, viewH, FOV);
    allLabels.push(sprite);
    world.add(sprite);
    return sprite;
  }

  function buildStatic(lay: WorldLayout): void {
    for (const dir of lay.dirs) {
      const plate = makePlate(dir.w, dir.d);
      plate.position.copy(toVec3(dir.origin));
      world.add(plate);
      // Le chemin du dossier est écrit au sol, au bord avant de la plaque.
      const label = makeLabel(dir.path, "dir");
      label.center.set(0, 0.5);
      label.position.copy(toVec3(dir.origin)).add(new THREE.Vector3(6, 2, dir.d + 18));
      labels.set(dir.path, label);
      addLabel(label);
    }
    for (const p of lay.projects) {
      if (p.active) {
        const frameObj = makeDistrictFrame(p.w + 170, p.d + 170);
        frameObj.position.copy(toVec3(p.origin)).add(new THREE.Vector3(-85, 0, -85));
        world.add(frameObj);
        const you = makeLabel("you are here", "here");
        you.position.copy(toVec3(p.origin)).add(new THREE.Vector3(p.w / 2, 6, p.d + 160));
        addLabel(you);
      }
      const name = makeLabel(p.name, "project");
      name.position.copy(toVec3(p.origin)).add(new THREE.Vector3(p.w / 2, 10, -120));
      addLabel(name);
    }
    for (const [path, box] of lay.files) {
      const stack = makeSlabStack();
      stack.group.position.copy(toVec3(box.base));
      stacks.set(path, stack);
      world.add(stack.group);
      const glow = makeGroundGlow();
      glow.position.copy(toVec3(box.base)).setY(1.5);
      glow.scale.setScalar(165);
      glows.set(path, glow);
      world.add(glow);
      const label = makeLabel(path.split("/").pop() ?? path, "file");
      label.position.copy(toVec3(box.base)).setY(60);
      labels.set(path, label);
      addLabel(label);
    }
  }

  // ---------------------------------------------------------- animation
  function updateFiles(state: SceneState, now: number, lay: WorldLayout): void {
    for (const [path, box] of lay.files) {
      const stack = stacks.get(path);
      const glow = glows.get(path);
      const label = labels.get(path);
      const st = state.files.get(path);
      const { read, active, changed } = intensities(st, now);
      if (stack) updateSlabStack(stack, st?.slabs ?? 1, read, active, changed);
      if (glow) {
        const m = glow.material as THREE.MeshBasicMaterial;
        const warm = 0.45 * active + 0.3 * changed;
        const cool = 0.28 * read;
        m.opacity = Math.max(warm, cool);
        m.color.set(warm > cool ? SPACE_THEME.accent : SPACE_THEME.fg);
      }
      if (label) {
        // L'étiquette du fichier flotte juste au-dessus de sa pile.
        label.position.y = (st?.slabs ?? 1) * 22 + 12 + 10;
        const s = label.material as THREE.SpriteMaterial;
        s.opacity = state.hub ? 0 : Math.min(0.85, 0.25 + 0.6 * Math.max(read, active) + (changed ? 0.3 : 0));
      }
      void box;
    }
  }

  function updateMain(state: SceneState, now: number, lay: WorldLayout): void {
    const path = state.mainFile;
    const box = path ? lay.files.get(path) : undefined;
    if (!box) {
      mainGhost.visible = false;
      mainBeam.visible = false;
      mainGlow.visible = false;
      return;
    }
    const st = state.files.get(path!);
    const topY = (st?.slabs ?? 1) * 22 + 12;
    const bob = Math.sin(now * 2.6) * 5;
    const pos = toVec3(box.base).setY(topY + 46 + bob);
    mainGhost.visible = true;
    mainGhost.position.copy(pos);
    mainGhost.rotation.y = Math.sin(now * 0.4) * 0.45;
    const target = toVec3(box.base).setY(topY);
    setBeam(mainBeam, pos, target, state.mainWrote);
    mainGlow.visible = true;
    mainGlow.position.copy(target).setY(1.5);
    const gm = mainGlow.material as THREE.MeshBasicMaterial;
    gm.opacity = 0.2;
    gm.color.set(SPACE_THEME.fg);
  }

  function updateSubs(state: SceneState, now: number, lay: WorldLayout): void {
    for (const [id, sub] of state.subs) {
      let ghost = subGhosts.get(id);
      if (!ghost) {
        ghost = makeGhost(40);
        subGhosts.set(id, ghost);
        scene.add(ghost);
      }
      const goal = subTarget(sub.target, lay);
      const start = layoutCenterVec(lay).setY(80);
      if (sub.doneAt === null) {
        const dur = sub.kind === "web" ? FLY.web : FLY.file;
        const u = clamp01((now - sub.spawnedAt) / dur);
        const e = u * u * (3 - 2 * u);
        const arc = Math.sin(Math.PI * u) * FLY.arc;
        ghost.visible = true;
        ghost.position.lerpVectors(start, goal, e).setY(
          THREE.MathUtils.lerp(start.y, goal.y, e) + arc,
        );
        continue;
      }
      let fall = falls.get(id);
      if (!fall) {
        fall = { from: ghost.position.clone(), started: sub.doneAt, dir: id % 2 === 0 ? 1 : -1, impacted: false };
        falls.set(id, fall);
      }
      const t = now - fall.started;
      const land = landTime(fall.from.y);
      if (t <= land) {
        ghost.visible = true;
        ghost.position.set(
          fall.from.x + fall.dir * PUSH.vx * t,
          Math.max(0, fall.from.y + PUSH.vy * t - 0.5 * PUSH.g * t * t),
          fall.from.z,
        );
        ghost.rotation.z = t * 2.0 * fall.dir;
      } else if (t < land + 0.6) {
        ghost.visible = true;
        ghost.position.setY(-60 * ((t - land) / 0.6));
      } else {
        ghost.visible = false;
      }
      if (!fall.impacted && t >= land) {
        fall.impacted = true;
        spawnImpact(ghost.position.clone().setY(0), false, now);
      }
    }
  }

  function updateKiro(state: SceneState, now: number, lay: WorldLayout): void {
    const k = state.kiro;
    if (!k) {
      kiro.visible = false;
      return;
    }
    const base = k.file ? lay.files.get(k.file)?.base : undefined;
    const at = base
      ? toVec3(base).add(new THREE.Vector3(143, 0, 0))
      : layoutCenterVec(lay);
    if (k.doneAt === null) {
      kiro.visible = true;
      kiro.position.copy(at).setY(90 + Math.sin(now * 2.6) * 5);
      kiro.rotation.z = 0;
      return;
    }
    const t = now - k.doneAt;
    const land = landTime(90);
    if (t <= land) {
      kiro.visible = true;
      kiro.position.set(at.x + PUSH.vx * t, Math.max(0, 90 + PUSH.vy * t - 0.5 * PUSH.g * t * t), at.z);
      kiro.rotation.z = t * 2.0;
    } else if (t < land + 0.6) {
      kiro.visible = true;
      kiro.position.setY(-60 * ((t - land) / 0.6));
    } else {
      kiro.visible = false;
    }
    if (t >= land && impacts.length === 0) spawnImpact(new THREE.Vector3(at.x + PUSH.vx * land, 0, at.z), false, now);
  }

  function updateImpacts(now: number): void {
    for (let i = impacts.length - 1; i >= 0; i--) {
      const im = impacts[i];
      const dt = now - im.at;
      if (dt < 0 || dt > 1.8) {
        if (dt > 1.8) {
          scene.remove(im.obj.group);
          disposeObject(im.obj.group);
          impacts.splice(i, 1);
        }
        continue;
      }
      const size = im.big ? 420 : 260;
      const u = Math.min(1, dt / 1.4);
      const e = 1 - (1 - u) * (1 - u);
      im.obj.ring.scale.setScalar(size * e + 1);
      (im.obj.ring.material as THREE.MeshBasicMaterial).opacity = (1 - u) * 0.7;
      const u2 = (dt - 0.12) / 1.6;
      if (u2 > 0) {
        const e2 = 1 - (1 - Math.min(1, u2)) ** 2;
        im.obj.white.scale.setScalar(size * 1.5 * e2 + 1);
        (im.obj.white.material as THREE.MeshBasicMaterial).opacity = (1 - Math.min(1, u2)) * 0.3;
      }
      if (dt < 0.45) {
        im.obj.flash.scale.setScalar(size * 1.2);
        (im.obj.flash.material as THREE.MeshBasicMaterial).opacity = (1 - dt / 0.45) * 0.5;
      }
      const pos = im.obj.dust.geometry.getAttribute("position") as THREE.BufferAttribute;
      for (let j = 0; j < 26; j++) {
        const a = (j / 26) * Math.PI * 2 + im.pos.x * 0.001;
        const spd = 40 + ((j * 37) % 160);
        pos.setXYZ(
          j,
          Math.cos(a) * spd * dt,
          Math.max(1, 60 * dt - 45 * dt * dt),
          Math.sin(a) * spd * dt,
        );
      }
      pos.needsUpdate = true;
      (im.obj.dust.material as THREE.PointsMaterial).opacity = Math.max(0, 1 - dt / 1.2) * 0.9;
    }
  }

  function updateCamera(state: SceneState, now: number, dt: number, lay: WorldLayout): void {
    if (state.rewind && state.rewind.at !== cam.lastRewind) {
      cam.lastRewind = state.rewind.at;
      cam.punch = 8;
      cam.shake = 12;
    }
    cam.punch = Math.max(0, cam.punch - dt * 9);
    cam.shake = Math.max(0, cam.shake - dt * (cam.shake / 0.9));
    const goal = state.hub
      ? HUB
      : state.mainFile && lay.files.get(state.mainFile)
        ? CLOSE
        : OVERVIEW;
    const target = state.hub
      ? layoutCenterVec(lay)
      : state.mainFile && lay.files.get(state.mainFile)
        ? toVec3(lay.files.get(state.mainFile)!.base).setY(60)
        : layoutCenterVec(lay);
    const k = 1 - Math.exp(-3.2 * dt);
    cam.dist += (goal.dist - cam.dist) * k;
    cam.pitch += (goal.pitch - cam.pitch) * k;
    cam.target.lerp(target, k);
    const shake = cam.shake;
    const off = new THREE.Vector3(
      Math.sin(now * 37) * shake,
      Math.cos(now * 29) * shake * 0.6,
      Math.sin(now * 43) * shake,
    );
    const sp = Math.sin(cam.pitch);
    const cp = Math.cos(cam.pitch);
    camera.position
      .set(cp * Math.sin(YAW), sp, cp * Math.cos(YAW))
      .multiplyScalar(cam.dist)
      .add(cam.target)
      .add(off);
    camera.lookAt(cam.target);
    camera.fov = FOV + cam.punch;
    camera.updateProjectionMatrix();
    const far = state.hub ? 26000 : state.mainFile ? 4000 : 1500;
    const fog = scene.fog as THREE.Fog;
    fog.far = Math.max(far, cam.dist * 2.2);
    fog.near = fog.far * 0.12;
  }

  function spawnImpact(pos: THREE.Vector3, big: boolean, now: number): void {
    const obj = makeImpact();
    obj.group.position.copy(pos);
    scene.add(obj.group);
    impacts.push({ obj, at: now, pos, big });
    cam.shake = Math.max(cam.shake, big ? 12 : 4);
  }
}

// ---------------------------------------------------------------- helpers
function keyOf(state: SceneState): string {
  return state.projects
    .map((p) => `${p.root}:${p.tree?.files.length ?? 0}`)
    .join("|");
}

function layoutCenterVec(lay: WorldLayout): THREE.Vector3 {
  return toVec3(layoutCenter(lay));
}

function subTarget(target: { path: string } | "web", lay: WorldLayout): THREE.Vector3 {
  if (target === "web") return layoutCenterVec(lay).add(new THREE.Vector3(-1500, 140, -800));
  const box = lay.files.get(target.path);
  return box ? toVec3(box.base).setY(40) : layoutCenterVec(lay);
}

function landTime(fromY: number): number {
  const y = Math.max(0, fromY);
  return (PUSH.vy + Math.sqrt(PUSH.vy * PUSH.vy + 2 * PUSH.g * y)) / PUSH.g;
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}

function setBeam(beam: THREE.Line, from: THREE.Vector3, to: THREE.Vector3, wrote: boolean): void {
  beam.visible = true;
  const pos = beam.geometry.getAttribute("position") as THREE.BufferAttribute;
  pos.setXYZ(0, from.x, from.y, from.z);
  pos.setXYZ(1, to.x, to.y, to.z);
  pos.needsUpdate = true;
  const m = beam.material as THREE.LineBasicMaterial;
  m.color.set(wrote ? SPACE_THEME.accent : SPACE_THEME.fg);
  m.opacity = wrote ? 0.85 : 0.55;
}

/// Sol en grille de points (pas 56 : 2× le design, pour tenir 60 fps webview).
function makeFloor(): THREE.Points {
  const step = 56;
  const n = 48;
  const positions: number[] = [];
  for (let iz = -n; iz <= n; iz++) {
    for (let ix = -n; ix <= n; ix++) {
      positions.push(ix * step, 0, iz * step);
    }
  }
  const geo = new THREE.BufferGeometry();
  geo.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  return new THREE.Points(
    geo,
    new THREE.PointsMaterial({
      color: new THREE.Color(SPACE_THEME.fg),
      size: 3,
      sizeAttenuation: true,
      transparent: true,
      opacity: seen(0.3),
      depthWrite: false,
    }),
  );
}

export type { Vec3 };
