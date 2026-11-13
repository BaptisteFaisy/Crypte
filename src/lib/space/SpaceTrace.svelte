<!-- SpaceTrace.svelte — la trace des 6 étapes dans le panneau de chat
     (DESIGN §4) : mini-dalle isométrique, index, nom. En attente = pâle,
     en cours = fond orange 10 % et icône orange, faite = icône teintée. -->
<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { space } from "./store.svelte";
  import { STEP_ORDER, stepRank, type Step } from "./types";

  const NAMES: Record<Step, string> = {
    ask: "Ask",
    context: "Context",
    think: "Think",
    act: "Act",
    critic: "Critic",
    answer: "Answer",
  };

  let current = $state(0);
  let timer: ReturnType<typeof setInterval> | null = null;
  onMount(() => {
    timer = setInterval(() => {
      const s = space.state.step;
      current = s ? stepRank(s) : 0;
    }, 100);
  });
  onDestroy(() => {
    if (timer) clearInterval(timer);
  });

  function stateOf(rank: number): string {
    if (current === 0 || rank > current) return "";
    return rank === current ? "act" : "done";
  }
</script>

{#if current > 0}
  <div class="space-trace" role="list" aria-label="Steps">
    {#each STEP_ORDER as step, i (step)}
      <div class="tr {stateOf(i + 1)}" role="listitem">
        <svg width="24" height="16" viewBox="0 0 24 16" aria-hidden="true">
          <path class="i2" d="M1 6L12 11V15L1 10Z" />
          <path class="i3" d="M12 11L23 6V10L12 15Z" />
          <path class="i1" d="M12 1L23 6L12 11L1 6Z" />
        </svg>
        <span class="ix">0{i + 1}</span>
        <span class="nm">{NAMES[step]}</span>
        <span class="tm">{stateOf(i + 1) === "act" ? "now" : ""}</span>
      </div>
    {/each}
  </div>
{/if}
