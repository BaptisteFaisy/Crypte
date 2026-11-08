<!-- KiroWidget — le fantôme gardien : `kiro-spectre` au repos
     ("It roams the crypt."), `kiro-effroi` en effroi ("Something scared
     it!"). Clic = `critic_smack` (« hit him »), réponse en bulle. -->
<script lang="ts">
  import PixelSprite from "./PixelSprite.svelte";
  import { critic, kiroIdle, kiroSpooked, smackKiro, spooked } from "./store";

  const kiro = $derived($spooked ? $kiroSpooked : $kiroIdle);
</script>

<section class="crypte-frame kiro" aria-label="Kiro">
  <button class="ghost" onclick={() => void smackKiro()} title="hit him">
    {#if kiro}
      <PixelSprite grid={kiro.grid} box={96} label="Kiro the ghost" />
    {:else}
      <span class="missing">…</span>
    {/if}
    <span class="frame-title name">Kiro</span>
  </button>
  <p class="lore">{$spooked ? "Something scared it!" : "It roams the crypt."}</p>
  {#if $critic}
    <p class="bubble" role="status">{$critic}</p>
  {/if}
</section>

<style>
  .kiro {
    padding: 0.5rem 0.6rem;
    text-align: center;
  }
  .ghost {
    display: inline-flex;
    flex-direction: column;
    align-items: center;
    gap: 0.15rem;
  }
  .name {
    color: var(--cry-spectre);
    font-size: 0.85rem;
  }
  .lore {
    margin: 0.25rem 0 0;
    color: var(--cry-sand);
    font-size: 0.75rem;
    font-style: italic;
  }
  .missing {
    color: var(--cry-umber);
  }
  .bubble {
    margin: 0.4rem 0 0;
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--cry-spectre-dark);
    border-radius: 6px;
    color: var(--cry-bone);
    font-size: 0.8rem;
    text-align: left;
  }
</style>
