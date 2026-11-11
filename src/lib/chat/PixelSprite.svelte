<!-- PixelSprite — grille bestiaire 32×32 sur canvas, facteur entier.
     Partagé par Kiro et les apparitions (zéro duplication). -->
<script lang="ts">
  import { bestScale, drawGrid } from "../pixel";

  interface Props {
    grid: string[];
    box?: number;
    label: string;
  }

  let { grid, box = 128, label }: Props = $props();
  let canvas: HTMLCanvasElement | null = $state(null);

  $effect(() => {
    if (canvas && grid.length > 0) {
      drawGrid(canvas, grid, bestScale(grid, { w: box, h: box }));
    }
  });
</script>

<span role="img" aria-label={label}><canvas class="pixel" bind:this={canvas} aria-hidden="true"></canvas></span>

<style>
  canvas.pixel {
    display: block;
  }
</style>
