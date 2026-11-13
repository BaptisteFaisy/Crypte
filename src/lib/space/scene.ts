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
  layoutCentroid,
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
const HUB = { dist: 15500, pitch: deg(36) };
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
  // three r155+ : unités physiques, il faut ×π pour retrouver l'éclairage du
  // prototype (r128). Sinon les fantômes sortent gris (DESIGN §6.3).
  scene.add(new THREE.HemisphereLight(0xffffff, 0x161616, 0.62 * Math.PI));
  const sun = new THREE.DirectionalLight(0xffffff, 0.7 * Math.PI);
  sun.position.set(500, 900, 800);
  scene.add(sun);
  const composer = new EffectComposer(renderer);
  composer.addPass(new RenderPass(scene, camera));
  // Bloom plus serré qu'au lab : avec l'éclairage ×π, un rayon de 0.5 faisait
  // autour du fantôme un halo géant qui délavait le sol (« disque sombre »).
  composer.addPass(new UnrealBloomPass(new THREE.Vector2(480, 270), 0.45, 0.22, 0.78));
  // Sans OutputPass, le composer sort du linéaire non converti : le noir
  // #050505 ressortait gris. OutputPass applique l'espace sRGB (DESIGN §2).
  composer.addPass(new OutputPass());

  const world = new THREE.Group();
  scene.add(world);
  const floor = makeFloor();
  scene.add(floor);
  const ambience = makeAmbience();
  scene.add(ambience);
  const plaza = makeGroundGlow();
  (plaza.material as THREE.MeshBasicMaterial).color.set(SPACE_THEME.accent);
  (plaza.material as THREE.MeshBasicMaterial).opacity = 0.18;
  plaza.scale.setScalar(520);
  scene.add(plaza);
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
  const dirLabels: THREE.Sprite[] = [];
  /// 1 près du projet, 0 en vue d'ensemble (F7) : les noms de fichiers et de
  /// dossiers s'effacent quand on s'éloigne, seuls les projets restent.
  let near = 1;
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
    disposeObject(ambience);
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
      dirLabels.push(label);
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
        s.opacity = near * Math.min(0.85, 0.25 + 0.6 * Math.max(read, active) + (changed ? 0.3 : 0));
      }
      void box;
    }
  }

  function updateMain(state: SceneState, now: number, lay: WorldLayout): void {
    const path = state.mainFile;
    const box = path ? lay.files.get(path) : undefined;
    const front = plazaOf(lay);
    plaza.position.copy(front).setY(1);
    if (!box) {
      // Pas de fichier en cours : le fantôme attend au sol, devant le projet.
      const bob = Math.sin(now * 2.6) * 5;
      mainGhost.visible = true;
      mainGhost.position.copy(front).setY(60 + bob);
      mainGhost.rotation.y = Math.sin(now * 0.4) * 0.45;
      mainBeam.visible = false;
      mainGlow.visible = true;
      mainGlow.position.copy(front).setY(1.5);
      const gm = mainGlow.material as THREE.MeshBasicMaterial;
      gm.opacity = 0.18;
      gm.color.set(SPACE_THEME.fg);
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
      ? toVec3(layoutCentroid(lay))
      : state.mainFile && lay.files.get(state.mainFile)
        ? toVec3(lay.files.get(state.mainFile)!.base).setY(
            // On vise le haut de la pile : le fantôme reste au centre de la vue,
            // sous le titre de l'étape, même sur une pile haute.
            (state.files.get(state.mainFile)?.slabs ?? 1) * 22 + 40,
          )
        : layoutCenterVec(lay);
    const k = 1 - Math.exp(-3.2 * dt);
    cam.dist += (goal.dist - cam.dist) * k;
    near = 1 - Math.min(1, Math.max(0, (cam.dist - 2600) / 2400));
    for (const l of dirLabels) (l.material as THREE.SpriteMaterial).opacity = 0.4 * near;
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
    const fu = (floor.material as THREE.ShaderMaterial).uniforms;
    fu.uNear.value = fog.near;
    fu.uFar.value = fog.far;
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

/// Sol (DESIGN §6.4) : points tous les 28, qui s'éteignent entre 1 600 et
/// 5 200 de la caméra, et lignes discrètes tous les 600. Shader, comme le
/// prototype : un plan infini, sans bord visible.
function makeFloor(): THREE.Mesh {
  const mat = new THREE.ShaderMaterial({
    transparent: true,
    depthWrite: false,
    uniforms: {
      uNear: { value: 300 },
      uFar: { value: 4000 },
      uDot: { value: seen(0.3) },
      uLine: { value: seen(0.08) },
    },
    vertexShader:
      "varying vec3 vW;void main(){vec4 w=modelMatrix*vec4(position,1.);vW=w.xyz;gl_Position=projectionMatrix*viewMatrix*w;}",
    fragmentShader:
      "uniform float uNear,uFar,uDot,uLine;varying vec3 vW;" +
      "void main(){vec2 p=vW.xz/28.;vec2 g=abs(fract(p)-.5);float d=length(g);float aa=fwidth(d);" +
      "float dt=1.-smoothstep(.075-aa,.075+aa,d);float dist=distance(vW,cameraPosition);dt*=(1.-smoothstep(1600.,5200.,dist))*uDot;" +
      "vec2 q=vW.xz/600.;vec2 r=.5-abs(fract(q)-.5);vec2 fw=fwidth(q)*1.3;float ln=1.-min(smoothstep(0.,fw.x,r.x),smoothstep(0.,fw.y,r.y));ln*=uLine;" +
      "float fog=1.-smoothstep(uNear,uFar,dist);gl_FragColor=vec4(vec3(.96),(dt+ln)*fog);}",
  });
  const mesh = new THREE.Mesh(new THREE.PlaneGeometry(120000, 120000), mat);
  mesh.rotation.x = -Math.PI / 2;
  return mesh;
}

/// Ambiance (DESIGN §6.4) : poussière et piliers très pâles, loin du projet.
function makeAmbience(): THREE.Group {
  const group = new THREE.Group();
  let seed = 7;
  const rnd = () => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed / 4294967296;
  };
  const n = 1000;
  const pos = new Float32Array(n * 3);
  for (let i = 0; i < n; i++) {
    pos[i * 3] = (rnd() - 0.5) * 12000;
    pos[i * 3 + 1] = rnd() * 2000;
    pos[i * 3 + 2] = (rnd() - 0.5) * 12000;
  }
  const dust = new THREE.Points(
    new THREE.BufferGeometry().setAttribute("position", new THREE.BufferAttribute(pos, 3)),
    new THREE.PointsMaterial({ color: 0xf5f5f3, size: 3, transparent: true, opacity: seen(0.4), depthWrite: false }),
  );
  group.add(dust);
  const c = document.createElement("canvas");
  c.width = 4;
  c.height = 256;
  const ctx = c.getContext("2d");
  if (ctx) {
    const g = ctx.createLinearGradient(0, 256, 0, 0);
    g.addColorStop(0, "rgba(255,255,255,1)");
    g.addColorStop(0.5, "rgba(255,255,255,.25)");
    g.addColorStop(1, "rgba(255,255,255,0)");
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, 4, 256);
  }
  const tex = new THREE.CanvasTexture(c);
  const geo = new THREE.BoxGeometry(1, 1, 1);
  geo.translate(0, 0.5, 0);
  for (let i = 0; i < 34; i++) {
    const a = rnd() * Math.PI * 2;
    const r = 2200 + rnd() * 9000;
    const w = 40 + rnd() * 50;
    const pillar = new THREE.Mesh(
      geo,
      new THREE.MeshBasicMaterial({ map: tex, color: 0xf5f5f3, transparent: true, opacity: seen(0.05 + rnd() * 0.035), depthWrite: false, side: THREE.DoubleSide }),
    );
    pillar.scale.set(w, 3000 + rnd() * 4000, w);
    pillar.position.set(Math.cos(a) * r, 0, Math.sin(a) * r);
    group.add(pillar);
  }
  return group;
}

/// Le parvis : devant le projet actif, là où le fantôme attend.
function plazaOf(lay: WorldLayout): THREE.Vector3 {
  const p = lay.projects.find((x) => x.active) ?? lay.projects[0];
  if (!p) return new THREE.Vector3();
  return toVec3(p.origin).add(new THREE.Vector3(p.w / 2, 0, 520));
}

export type { Vec3 };
