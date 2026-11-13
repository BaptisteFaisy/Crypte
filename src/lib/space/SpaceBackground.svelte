<!-- SpaceBackground.svelte — le monde 3D en fond plein écran.
     Canvas fixe derrière toute l'app ; le rendu three.js lit l'état logique
     du store à chaque frame. Hors activation (F8) : rien n'est monté. -->
<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { createSpaceScene, type SpaceScene } from "./scene";
  import { space } from "./store.svelte";

  let canvas: HTMLCanvasElement | undefined = $state();
  let scene: SpaceScene | null = null;
  let raf = 0;
  let last = 0;

  function resize(): void {
    if (scene && canvas) scene.resize(canvas.clientWidth, canvas.clientHeight);
  }

  onMount(() => {
    if (!canvas) return;
    try {
      scene = createSpaceScene(canvas);
    } catch {
      // Pas de WebGL (machine sans GPU, certains headless) : pas de fond 3D,
      // l'app reste utilisable.
      scene = null;
      return;
    }
    resize();
    window.addEventListener("resize", resize);
    last = performance.now();
    const loop = (t: number): void => {
      const dt = Math.min(0.1, (t - last) / 1000);
      last = t;
      if (scene && space.enabled) scene.frame(space.state, space.now(), dt);
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
  });

  onDestroy(() => {
    cancelAnimationFrame(raf);
    window.removeEventListener("resize", resize);
    scene?.dispose();
    scene = null;
  });
</script>

<canvas class="space-bg" bind:this={canvas} aria-hidden="true"></canvas>
{#if space.enabled}<div class="space-vig" aria-hidden="true"></div>{/if}
