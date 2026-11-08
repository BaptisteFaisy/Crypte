<!-- Écran Haunt : jeu (commandes PLAN §9). Boucle Rust (tick 100 ms),
     frames via event `haunt_frame`, clavier via `haunt_input`.
     Caméra centrée joueur (coordonnées jeu flottantes, espace variable). -->
<script lang="ts">
  import type {
    CrypteUiEvent,
    HauntFrame,
    HauntKey,
  } from "../contracts";
  import { onDestroy, onMount } from "svelte";
  import { errText, invokeCmd, listenCrypte } from "../tauri";

  const VIEW_W = 480;
  const VIEW_H = 270;
  const SCALE = 3;

  let frame = $state<HauntFrame | null>(null);
  let running = $state(false);
  let error = $state<string | null>(null);
  let canvas: HTMLCanvasElement | null = $state(null);
  let unlisten: (() => void) | null = null;

  function draw(f: HauntFrame): void {
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    canvas.width = VIEW_W;
    canvas.height = VIEW_H;
    ctx.fillStyle = "#0C0908";
    ctx.fillRect(0, 0, VIEW_W, VIEW_H);
    const camX = f.player.x * SCALE - VIEW_W / 2;
    const camY = f.player.y * SCALE - VIEW_H / 2;
    const px = (x: number) => x * SCALE - camX;
    const py = (y: number) => y * SCALE - camY;
    for (const foe of f.foes) {
      ctx.fillStyle = "#D7263D";
      ctx.fillRect(px(foe.x) - 4, py(foe.y) - 4, 8, 8);
    }
    for (const s of f.shots) {
      ctx.fillStyle = s.foe ? "#9B8CFF" : "#FFC46B";
      ctx.fillRect(px(s.x) - 1, py(s.y) - 1, 3, 3);
    }
    ctx.fillStyle = "#EBDDC3";
    ctx.fillRect(px(f.player.x) - 4, py(f.player.y) - 6, 8, 12);
  }

  async function start(): Promise<void> {
    error = null;
    try {
      frame = await invokeCmd<HauntFrame>("haunt_start");
      running = true;
      if (frame) draw(frame);
    } catch (e) {
      error = errText(e);
    }
  }

  async function stop(): Promise<void> {
    try {
      await invokeCmd<null>("haunt_stop");
    } catch (e) {
      error = errText(e);
    } finally {
      running = false;
    }
  }

  async function sendInput(key: HauntKey, pressed: boolean): Promise<void> {
    if (!running) return;
    try {
      await invokeCmd<null>("haunt_input", { key, pressed });
    } catch (e) {
      error = errText(e);
    }
  }

  function onKey(e: KeyboardEvent, pressed: boolean): void {
    const map: Record<string, HauntKey> = {
      ArrowLeft: "left",
      ArrowRight: "right",
      ArrowUp: "up",
      ArrowDown: "down",
      " ": "fire",
      Escape: "quit",
    };
    const key = map[e.key];
    if (key === undefined) return;
    e.preventDefault();
    void sendInput(key, pressed);
  }

  function onKeyDown(e: KeyboardEvent): void {
    if (e.repeat) return;
    onKey(e, true);
  }

  function onKeyUp(e: KeyboardEvent): void {
    onKey(e, false);
  }

  onMount(() => {
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    void listenCrypte("crypte-ui", (ev) => {
      if (ev.kind !== "haunt_frame") return;
      frame = ev.frame;
      draw(ev.frame);
    }).then((u) => {
      unlisten = u;
    });
  });

  onDestroy(() => {
    window.removeEventListener("keydown", onKeyDown);
    window.removeEventListener("keyup", onKeyUp);
    unlisten?.();
    void stop();
  });
</script>

<h1>Hantise</h1>
{#if error}<p class="err">{error}</p>{/if}

<div class="panel">
  <div class="row">
    {#if running}
      <button class="danger" onclick={() => void stop()}>Quitter (pause)</button>
    {:else}
      <button class="primary" onclick={() => void start()}>Entrer dans la hantise</button>
    {/if}
    <span class="dim">Flèches = bouger · Espace = tir · Échap = quitter</span>
  </div>
</div>

{#if frame}
  <div class="panel">
    <div class="row mono">
      <span>Niveau {frame.hud.level}</span>
      <span>Score {frame.hud.score}</span>
      <span>Vies {frame.hud.lives}</span>
      <span>Peur {frame.hud.fear}</span>
    </div>
  </div>
{/if}

<canvas
  bind:this={canvas}
  class="pixels"
  width={VIEW_W}
  height={VIEW_H}
  style="border: 1px solid var(--cry-umber); background: var(--cry-void); width: 100%; max-width: 960px;"
  aria-label="Jeu de la hantise"
></canvas>
{#if !frame}<p class="dim">En attente d'une frame — entre dans la hantise.</p>{/if}
