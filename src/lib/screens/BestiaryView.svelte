<!-- Galerie du bestiaire : 33 créatures 32x32 (commandes PLAN §7).
     Sprites décodés lettre->couleur (pixel.ts), facteur entier, net. -->
<script lang="ts">
  import type { Creature } from "../contracts";
  import { onMount } from "svelte";
  import { errText, invokeCmd } from "../tauri";
  import { drawGrid, isValidGrid } from "../pixel";

  interface Entry {
    slug: string;
    name: string;
    lore: string;
  }

  let entries = $state<Entry[]>([]);
  let current = $state<Creature | null>(null);
  let error = $state<string | null>(null);
  let canvas: HTMLCanvasElement | null = $state(null);
  let search = $state("");

  let filtered = $derived(
    search
      ? entries.filter(
          (e) =>
            e.slug.includes(search.toLowerCase()) ||
            e.name.toLowerCase().includes(search.toLowerCase()),
        )
      : entries,
  );

  $effect(() => {
    if (current && canvas && isValidGrid(current.grid)) {
      drawGrid(canvas, current.grid, 6);
    }
  });

  async function show(slug: string): Promise<void> {
    error = null;
    try {
      current = await invokeCmd<Creature>("bestiary_get", { slug });
    } catch (e) {
      error = errText(e);
    }
  }

  async function random(): Promise<void> {
    error = null;
    try {
      current = await invokeCmd<Creature>("bestiary_random");
    } catch (e) {
      error = errText(e);
    }
  }

  onMount(() => {
    void (async () => {
      try {
        entries = await invokeCmd<Entry[]>("bestiary_list");
        if (entries.length > 0) await show(entries[0]?.slug ?? "fantome");
      } catch (e) {
        error = errText(e);
      }
    })();
  });
</script>

<h1>Bestiaire</h1>
{#if error}<p class="err">{error}</p>{/if}

<div class="row" style="align-items: flex-start; gap: 1rem;">
  <div class="panel" style="min-width: 16rem;">
    <div class="row">
      <input bind:value={search} placeholder="chercher…" aria-label="Chercher une créature" />
      <button onclick={() => void random()}>Hasard</button>
    </div>
    <p class="dim">{filtered.length} créature(s)</p>
    <ul style="list-style: none; padding: 0; margin: 0; max-height: 24rem; overflow-y: auto;">
      {#each filtered as e (e.slug)}
        <li>
          <button
            onclick={() => void show(e.slug)}
            style={current?.slug === e.slug ? "border-color: var(--cry-pumpkin);" : ""}
            title={e.lore}
          >
            {e.name}
          </button>
        </li>
      {/each}
    </ul>
  </div>

  <div class="panel" style="flex: 1;">
    {#if current}
      <h2>{current.name}</h2>
      <p class="dim mono">{current.slug}</p>
      <p><em>{current.lore}</em></p>
      <canvas bind:this={canvas} class="pixels" aria-label={current.name}></canvas>
    {:else}
      <p class="dim">Choisis une créature dans la liste.</p>
    {/if}
  </div>
</div>
