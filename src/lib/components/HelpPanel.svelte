<!-- HelpPanel — aide /help en modale (texte de texts.rs via help.ts). -->
<script lang="ts">
  import { HELP_TEXT } from "../help";
  import { shell } from "../crypte.svelte";

  function close(): void {
    shell.helpOpen = false;
  }

  function onKey(event: KeyboardEvent): void {
    if (event.key === "Escape") close();
  }
</script>

<svelte:window onkeydown={onKey} />

<div
  class="veil"
  data-testid="help-panel"
  role="dialog"
  aria-modal="true"
  aria-label="Aide"
>
  <div class="card">
    <div class="head">
      <h2>grimoire — /help</h2>
      <button class="crypte-btn ghost" onclick={close} data-testid="help-close">
        close
      </button>
    </div>
    <pre class="body">{HELP_TEXT}</pre>
  </div>
</div>

<style>
  .veil {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(12, 9, 8, 0.82);
    padding: 1rem;
  }
  .card {
    width: min(46rem, 100%);
    max-height: calc(100dvh - 4rem);
    display: flex;
    flex-direction: column;
    background: var(--cry-crypt);
    border: 1px solid var(--cry-umber);
    border-radius: 0.5rem;
    padding: 1rem 1.25rem;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    margin-bottom: 0.5rem;
  }
  h2 {
    margin: 0;
    font-size: 1rem;
    color: var(--cry-pumpkin);
  }
  .body {
    margin: 0;
    overflow: auto;
    white-space: pre-wrap;
    color: var(--cry-bone);
    font: inherit;
    font-size: 0.85em;
  }
</style>
