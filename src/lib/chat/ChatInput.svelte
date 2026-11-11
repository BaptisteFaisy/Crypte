<!-- ChatInput — saisie : prompt + aperçu du flow `/connect` (clé
     masquée `•`, modèle en clair, brouillon intact), pastille question
     bougie quand une question IA attend (digit = réponse), jauge ctx,
     Entrée = envoi, Ctrl+C = stop. -->
<script lang="ts">
  import {
    cancelConnect,
    chatState,
    chats,
    connectCommit,
    connectFlow,
    connectType,
    gauge,
    notice,
    promptLabel,
    send,
    stop,
  } from "./store";

  let draft = $state("");
  let area: HTMLTextAreaElement | null = $state(null);

  const masked = $derived($connectFlow?.step === "key");
  const shown = $derived(
    $connectFlow ? $connectFlow.buffer : ($chatState?.input ?? draft),
  );
  const sending = $derived($chatState?.sending ?? false);
  const pending = $derived($chatState?.pendingQuestion ?? null);
  const gaugePct = $derived(
    $gauge && $gauge.window > 0
      ? Math.min(100, Math.round((100 * $gauge.used) / $gauge.window))
      : 0,
  );

  function onInput(value: string): void {
    if ($connectFlow) connectType(value);
    else draft = value;
  }

  async function commit(): Promise<void> {
    if ($connectFlow) {
      await connectCommit();
      return;
    }
    const body = draft;
    draft = "";
    await send(body);
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void commit();
    } else if (e.key === "Escape" && $connectFlow) {
      e.preventDefault();
      cancelConnect();
    }
  }

  function answerDigit(n: number): void {
    draft = String(n);
    area?.focus();
  }
</script>

<footer class="crypte-frame composer">
  {#if $notice}
    <p class="notice" role="status">{$notice}</p>
  {/if}
  {#if pending}
    <div class="pill" role="note">
      <span class="q">— the crypt asks you: {pending.question}</span>
      <span class="opts">
        {#each pending.options as opt, i}
          <button class="opt" onclick={() => answerDigit(i + 1)} title="reply {i + 1}">
            {i + 1}) {opt}
          </button>
        {/each}
      </span>
    </div>
  {/if}
  <div class="row">
    <div class="field">
      {#if $promptLabel}
        <label class="prompt" for="crypte-input">{$promptLabel}</label>
      {/if}
      <textarea
        id="crypte-input"
        bind:this={area}
        class:masked
        rows={masked ? 1 : 3}
        value={masked ? "•".repeat(shown.length) : shown}
        oninput={(e) => onInput(e.currentTarget.value)}
        onkeydown={onKey}
        placeholder={$chats.length === 0
          ? "no chat — Ctrl+N first"
          : "write… (Enter sends, Shift+Enter breaks)"}
        aria-label={$promptLabel ?? "Message"}
      ></textarea>
    </div>
    <div class="side">
      {#if $gauge}
        <div
          class="ctx"
          role="meter"
          aria-valuenow={gaugePct}
          aria-valuemin={0}
          aria-valuemax={100}
          title="context {$gauge.used} / {$gauge.window}"
        >
          <span class="bar"><span class="fill" style="width: {gaugePct}%"></span></span>
          <span class="num">{gaugePct}%</span>
        </div>
      {/if}
      {#if sending}
        <button class="act stop" onclick={() => void stop()} title="stop (Ctrl+C)">stop ■</button>
      {:else}
        <button class="act send" onclick={() => void commit()} title="send (Enter)">send ↵</button>
      {/if}
    </div>
  </div>
</footer>

<style>
  .composer {
    padding: 0.45rem 0.6rem 0.55rem;
  }
  .notice {
    margin: 0 0 0.35rem;
    color: var(--cry-candle);
    font-size: 0.8rem;
  }
  .pill {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem 0.6rem;
    align-items: baseline;
    border: 1px solid var(--cry-candle-dark);
    border-radius: 6px;
    padding: 0.3rem 0.5rem;
    margin-bottom: 0.4rem;
    background: var(--cry-crypt);
  }
  .pill .q {
    color: var(--cry-candle);
  }
  .opts {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem 0.5rem;
  }
  .opt {
    color: var(--cry-bone);
    border: 1px solid var(--cry-umber);
    border-radius: 999px;
    padding: 0 0.55rem;
    font-size: 0.8rem;
  }
  .row {
    display: flex;
    gap: 0.6rem;
    align-items: stretch;
  }
  .field {
    flex: 1;
    min-width: 0;
  }
  .prompt {
    display: block;
    color: var(--cry-sand);
    font-size: 0.75rem;
    margin-bottom: 0.15rem;
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    resize: vertical;
    background: var(--cry-void);
    color: var(--cry-bone);
    border: 1px solid var(--cry-umber);
    border-radius: 6px;
    font: inherit;
    padding: 0.4rem 0.5rem;
  }
  textarea.masked {
    letter-spacing: 0.1em;
  }
  .side {
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    align-items: flex-end;
    gap: 0.35rem;
  }
  .ctx {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-family: var(--cry-font-mono);
    font-size: 0.72rem;
    color: var(--cry-sand);
  }
  .ctx .bar {
    display: inline-block;
    width: 4.5rem;
    height: 0.4rem;
    background: var(--cry-crypt);
    border: 1px solid var(--cry-umber);
    border-radius: 3px;
    overflow: hidden;
  }
  .ctx .fill {
    display: block;
    height: 100%;
    background: var(--cry-candle-dark);
  }
  .act {
    border: 1px solid var(--cry-umber);
    border-radius: 6px;
    padding: 0.3rem 0.8rem;
    font-family: var(--cry-font-title);
  }
  .act.send {
    color: var(--cry-pumpkin);
    border-color: var(--cry-pumpkin-dark);
  }
  .act.stop {
    color: var(--cry-blood);
    border-color: var(--cry-dried);
  }
</style>
