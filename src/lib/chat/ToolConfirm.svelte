<!-- ToolConfirm — modale y/n pour les appels d'outils (event tool_confirm).
     Un seul ToolRun à la fois : pas d'id, `tool_decide(allow)` suffit. -->
<script lang="ts">
  import { invokeCmd, errText } from "../tauri";
  import CrypteDialog from "../widgets/CrypteDialog.svelte";
  import { pendingConfirm } from "./store";

  let busy = $state(false);
  let error = $state<string | null>(null);

  /** Échap/clic fond = refus (fail-closed, jamais d'exécution surprise). */
  function dismiss(): void {
    if ($pendingConfirm && !busy) void decide(false);
  }

  async function decide(allow: boolean): Promise<void> {
    busy = true;
    error = null;
    try {
      await invokeCmd<null>("tool_decide", { allow });
      pendingConfirm.set(null);
    } catch (e) {
      error = errText(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if $pendingConfirm}
  <CrypteDialog title="tool call — allow?" open={true} onclose={dismiss}>
    <p class="call">
      <code>{$pendingConfirm.call.name}</code>
    </p>
    <pre class="args">{$pendingConfirm.call.argsJson}</pre>
    <p class="hint">chat {$pendingConfirm.chat + 1} · y = allow once, n = deny</p>
    {#if error}
      <p class="err" role="alert">{error}</p>
    {/if}
    {#snippet footer()}
      <button onclick={() => void decide(false)} disabled={busy}>deny (n)</button>
      <button class="primary" onclick={() => void decide(true)} disabled={busy}>
        allow (y)
      </button>
    {/snippet}
  </CrypteDialog>
{/if}

<style>
  .call {
    margin: 0 0 0.5rem;
  }
  .call code {
    color: var(--cry-candle);
    font-size: 1.05rem;
  }
  .args {
    margin: 0;
    padding: 0.6rem;
    background: var(--cry-ink);
    border: var(--cry-border);
    border-radius: 3px;
    max-height: 12rem;
    overflow: auto;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .hint {
    color: var(--cry-ash);
    font-size: 0.8rem;
  }
  .err {
    color: var(--cry-blood);
  }
</style>
