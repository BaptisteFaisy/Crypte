<!-- ApparitionPanel — créature du panneau droit (apparition.rs) :
     réponse en cours → chaudron, agents actifs → araignée, présage
     récent → erreur/compaction/effroi, sinon rotation lente du bestiaire
     (20 s). Rafraîchi par event `apparition` + commande au montage. -->
<script lang="ts">
  import PixelSprite from "./PixelSprite.svelte";
  import { apparition } from "./store";

  const OMEN_FR: Record<string, string> = {
    error: "omen of error",
    compact: "omen of compaction",
    scare: "jump scare",
  };
</script>

<section class="crypte-frame apparition" aria-label="Apparition">
  <div class="frame-title title">apparition</div>
  {#if $apparition}
    <PixelSprite
      grid={$apparition.creature.grid}
      box={128}
      label={$apparition.creature.name}
    />
    <p class="frame-title name">{$apparition.creature.name}</p>
    <p class="lore">{$apparition.creature.lore}</p>
    {#if $apparition.omen}
      <p class="omen" role="status">{OMEN_FR[$apparition.omen] ?? $apparition.omen}</p>
    {/if}
  {:else}
    <p class="lore">the dark is empty — for now.</p>
  {/if}
</section>

<style>
  .apparition {
    padding: 0.4rem 0.6rem 0.5rem;
    text-align: center;
  }
  .title {
    color: var(--cry-sand);
    font-size: 0.8rem;
    text-align: left;
    padding-bottom: 0.25rem;
  }
  .apparition :global(canvas) {
    margin: 0 auto;
  }
  .name {
    color: var(--cry-bone);
    margin: 0.3rem 0 0;
  }
  .lore {
    color: var(--cry-sand);
    font-size: 0.75rem;
    font-style: italic;
    margin: 0.15rem 0 0;
  }
  .omen {
    color: var(--cry-blood);
    font-size: 0.8rem;
    margin: 0.3rem 0 0;
  }
</style>
