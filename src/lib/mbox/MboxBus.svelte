<!-- Onglet Bus : topics phone/gpu/file/divers (`tail --bus`, `bus -t`).
     Le topic `file` transporte les pièces jointes (méta seules). -->
<script lang="ts">
  import type {
    BusTopic,
    MboxMessage,
    MboxSendResult,
  } from "../contracts";
  import { onMount } from "svelte";
  import { errText, invokeCmd } from "../tauri";

  const TOPICS: BusTopic[] = ["phone", "gpu", "file", "divers"];

  let topic = $state<BusTopic>("file");
  let messages = $state<MboxMessage[]>([]);
  let error = $state<string | null>(null);
  let count = $state("30");

  let text = $state("");
  let from = $state("");
  let sendResult = $state<string | null>(null);

  async function refresh(): Promise<void> {
    error = null;
    try {
      const n = Number.parseInt(count, 10) || 30;
      messages = await invokeCmd<MboxMessage[]>("mbox_tail", { n, bus: topic });
    } catch (e) {
      error = errText(e);
    }
  }

  function pick(t: BusTopic): void {
    topic = t;
    void refresh();
  }

  onMount(() => void refresh());

  async function send(): Promise<void> {
    if (!text) return;
    error = null;
    sendResult = null;
    try {
      const res = await invokeCmd<MboxSendResult>("mbox_bus", {
        topic,
        message: text,
        from: from || undefined,
      });
      sendResult = `${res.status} : ${res.detail}`;
      text = "";
      await refresh();
    } catch (e) {
      error = errText(e);
    }
  }
</script>

{#if error}<p class="err">{error}</p>{/if}

<nav class="row" aria-label="Topics du bus">
  {#each TOPICS as t (t)}
    <button class={topic === t ? "primary" : ""} onclick={() => pick(t)}>{t}</button>
  {/each}
  <label class="dim">n <input bind:value={count} style="width: 4rem;" /></label>
  <button onclick={() => void refresh()}>Rafraîchir</button>
</nav>

<div class="panel" style="margin-top: 0.8rem;">
  <h2>Publier sur {topic}</h2>
  <div class="row">
    <label>De <input bind:value={from} placeholder="ton nom d'agent" /></label>
    <input bind:value={text} placeholder="message…" style="flex: 1; min-width: 12rem;" />
    <button class="primary" onclick={() => void send()} disabled={!text}>Publier</button>
  </div>
  {#if sendResult}<p class="dim">{sendResult}</p>{/if}
</div>

<div class="panel">
  <h2>bus/{topic}</h2>
  {#if messages.length === 0}
    <p class="dim">Topic vide.</p>
  {:else}
    {#each messages as m, i (`${i}-${m.date}-${m.expediteur}`)}
      <div class="gutter">
        <span class="mono dim">{m.date}</span>
        <strong>{m.expediteur}</strong>
        <span class={m.priorite === "P0" ? "err" : "dim"}>[{m.priorite}]</span>
        <p style="margin: 0.2rem 0;">{m.corps}</p>
      </div>
    {/each}
  {/if}
</div>
