<!-- StatusBar — barre powerline (miroir de draw_status, app/ui.rs).
  Pastilles à gauche (chat, modèle, tokens), raccourcis à droite.
  Compact : pastilles courtes, modèle à 10 caractères, hints réduits. -->
<script lang="ts">
  import { shell } from "../crypte.svelte";

  /** Modèle court du TUI (tokens::short_model : après `/` ou `:`). */
  function shortModel(model: string): string {
    const cut = Math.max(model.lastIndexOf("/"), model.lastIndexOf(":"));
    return cut >= 0 ? model.slice(cut + 1) : model;
  }

  /** Débit du TUI (tokens::fmt_tps). */
  function fmtTps(tps: number): string {
    if (tps <= 0) return "— tok/s";
    return tps >= 100 ? `${Math.round(tps)} tok/s` : `${tps.toFixed(1)} tok/s`;
  }

  let chatLabel = $derived(
    shell.chatCount === 0
      ? "no chat"
      : `chat ${shell.chatIndex + 1}/${shell.chatCount}`,
  );
  let modelLabel = $derived(shortModel(shell.model) || "no model");
  let rateLabel = $derived(
    `${shell.totalTokens} tok · ${shell.grandTotal} total · ${fmtTps(shell.tps)}`,
  );
  let rateShort = $derived(
    `${shell.totalTokens} tok · ${fmtTps(shell.tps)}`,
  );
</script>

<footer class="powerline" data-testid="status-bar">
  <div class="pills">
    <span
      class="pill chat"
      class:asking={shell.asking}
      data-testid="status-chat">{chatLabel}</span
    >
    <span class="pill model" data-testid="status-model">{modelLabel}</span>
    <span class="pill rate full" data-testid="status-rate">{rateLabel}</span>
    <span class="pill rate short" aria-hidden="true">{rateShort}</span>
  </div>
  {#if shell.game}
    <div class="hints" aria-label="Raccourcis jeu">
      <span class="hint full"><b>F5</b> thunder&ensp;</span>
      <span class="hint full"><b>Esc/F9</b> back&ensp;</span>
      <span class="hint full"><b>F6</b> shop&ensp;</span>
      <span class="hint short" aria-hidden="true"><b>Esc F9</b></span>
    </div>
  {:else}
    <div class="hints" aria-label="Raccourcis">
      <span class="hint full"><b>F5</b> scare&ensp;</span>
      <span class="hint full"><b>Ctrl+←/→</b> column&ensp;</span>
      <span class="hint full"><b>q</b> quit&ensp;</span>
      <span class="hint short" aria-hidden="true"><b>F5</b></span>
    </div>
  {/if}
</footer>

<style>
  .powerline {
    height: var(--cry-footer-h);
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0 1rem;
    border-top: 1px solid var(--cry-umber);
    background: var(--cry-void);
    flex: none;
    overflow: hidden;
    white-space: nowrap;
  }
  .pills {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
    overflow: hidden;
  }
  .pill {
    border-radius: 999px;
    padding: 0 0.7rem;
    font-weight: 700;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: none;
  }
  .pill.chat {
    background: var(--cry-pumpkin);
    color: var(--cry-void);
  }
  .pill.chat.asking {
    background: var(--cry-candle);
  }
  .pill.model {
    background: var(--cry-umber);
    color: var(--cry-bone);
    max-width: 22ch;
  }
  .pill.rate {
    background: var(--cry-pill-dim);
    color: var(--cry-sand);
    font-weight: 400;
  }
  .pill.rate.short {
    display: none;
  }
  .hints {
    margin-left: auto;
    color: var(--cry-ash);
    flex: none;
  }
  .hints b {
    color: var(--cry-pumpkin);
    font-weight: 700;
  }
  .hint.short {
    display: none;
  }
  @media (max-width: 45rem) {
    .pill.model {
      max-width: 10ch;
    }
    .pill.rate.full,
    .hint.full {
      display: none;
    }
    .pill.rate.short,
    .hint.short {
      display: inline;
    }
    .powerline {
      padding: 0 0.6rem;
    }
  }
</style>
