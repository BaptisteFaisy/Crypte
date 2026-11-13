<!-- SpaceHud.svelte — calques au-dessus de la 3D (DESIGN §3, §5) :
     titre de l'étape en cours, légende « qui fait quoi », colonne de mesures.
     Lit l'état logique du store 10 fois par seconde ; aucun événement brut. -->
<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { hudOf, type Hud } from "./hud";
  import { space } from "./store.svelte";

  let hud: Hud = $state(hudOf(space.state, 0));
  let timer: ReturnType<typeof setInterval> | null = null;

  onMount(() => {
    timer = setInterval(() => {
      if (space.enabled) hud = hudOf(space.state, space.now());
    }, 100);
  });
  onDestroy(() => {
    if (timer) clearInterval(timer);
  });
</script>

{#if space.enabled}
  <div class="space-hud" aria-live="polite">
    <div class="now">
      <div class="lab"><span class="o">{hud.index}</span>&nbsp;&nbsp;now</div>
      <div class="w">{hud.title}</div>
    </div>
    <div class="cap">
      {#if hud.who}<span class="who {hud.whoKind}">{hud.who}</span>{/if}
      <span>{hud.caption}</span>
    </div>
    <div class="mets">
      {#each hud.metrics as m (m.label)}
        <div class="mt">
          <div class="lab">{m.label}</div>
          <div class="v">{m.value}</div>
          <div class="bar"><i style="width: {Math.round(m.fill * 100)}%"></i></div>
        </div>
      {/each}
    </div>
  </div>
{/if}
