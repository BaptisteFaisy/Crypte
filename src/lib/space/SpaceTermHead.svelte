<!-- SpaceTermHead.svelte — barre de titre du panneau de chat quand le fond 3D
     est actif (DESIGN §4) : trois points (le premier orange), le fil
     « projet / chat / modèle » avec des barres obliques orange, et ⌘K. -->
<script lang="ts">
  import { space } from "./store.svelte";

  let { chat = "chat-1", model = "" }: { chat?: string; model?: string } = $props();
  let project = $derived(space.state.projects[0]?.name ?? "crypt");
</script>

<div class="space-thead">
  <span class="dots" aria-hidden="true"><i class="on"></i><i></i><i></i></span>
  <span class="crumb">
    {project} <span class="o">/</span> {chat}{#if model} <span class="o">/</span> {model}{/if}
  </span>
  <span class="kbd">⌘K</span>
</div>
