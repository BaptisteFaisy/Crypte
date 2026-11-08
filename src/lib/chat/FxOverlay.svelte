<!-- FxOverlay — effets horreur SANS toucher au texte (fx.rs : la brume
     ne teinte que le fond, `protect` garde texte et pixel art nets).
     Donc : vignette chaude + brume en couche de FOND (derrière le
     contenu), éclair en liseré de cadre uniquement — jamais par-dessus
     les mots. Figé si `prefers-reduced-motion`. -->
<script lang="ts">
  import { fear, flash } from "./store";

  const fog = $derived(Math.min(0.16, 0.04 + ($fear / 100) * 0.12));
  const glow = $derived($flash > 0);
</script>

<div class="fx" aria-hidden="true">
  <div
    class="veil"
    style="opacity: {fog.toFixed(3)}; --fear: {($fear / 100).toFixed(3)}"
  ></div>
  {#if glow}
    <div class="edge"></div>
  {/if}
</div>

<style>
  /* Derrière tout : z-index 0, le contenu passe au-dessus. */
  .fx {
    position: absolute;
    inset: 0;
    z-index: 0;
    pointer-events: none;
    overflow: hidden;
  }
  /* Vignette chaude centrée haut (fx.rs `paint_background`), qui
     rougit avec la peur : CRYPT → DRIED sur fond VOID. */
  .veil {
    position: absolute;
    inset: 0;
    background:
      radial-gradient(
        ellipse 120% 90% at 50% 30%,
        color-mix(in srgb, #1e1512 70%, #6e101a calc(var(--fear, 0) * 100%)),
        #0c0908 75%
      );
  }
  /* Éclair d'effroi : liseré sang sur le cadre, pas sur le texte. */
  .edge {
    position: absolute;
    inset: 0;
    border: 3px solid var(--cry-blood);
    border-radius: 10px;
    opacity: 0.8;
  }
  @media (prefers-reduced-motion: reduce) {
    .edge {
      display: none;
    }
  }
</style>
