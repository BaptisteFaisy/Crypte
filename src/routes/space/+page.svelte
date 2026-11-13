<!-- Route /space — harnais de vérification du fond 3D hors Tauri.
     Monte SpaceBackground seul, avec le scénario de démo (demo.ts) :
     `npm run dev` puis ouvrir /space dans un navigateur. -->
<script lang="ts">
  import { onMount } from "svelte";
  import SpaceBackground from "../../lib/space/SpaceBackground.svelte";
  import { space } from "../../lib/space/store.svelte";

  onMount(() => {
    void space.enable();
  });
</script>

<SpaceBackground />

<div class="space-harness">
  <p>
    fond 3D — <button onclick={() => space.toggleHub()}>F7 hub</button>
    <button onclick={() => void space.toggle()}>F8 on/off</button>
  </p>
</div>

<style>
  .space-harness {
    position: fixed;
    left: 16px;
    bottom: 16px;
    z-index: 5;
    color: #f5f5f3;
    font: 12px ui-monospace, monospace;
    background: rgba(5, 5, 5, 0.55);
    border: 1px solid rgba(245, 245, 243, 0.13);
    border-radius: 10px;
    padding: 8px 12px;
  }
  .space-harness button {
    color: #ff5b1f;
    background: none;
    border: 1px solid rgba(255, 91, 31, 0.5);
    border-radius: 8px;
    margin-left: 8px;
    font: inherit;
    cursor: pointer;
  }
</style>
