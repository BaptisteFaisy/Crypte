<!-- Onglet Messages : inbox (`tail`), envoi (`send`), digest. -->
<script lang="ts">
  import type {
    MboxMessage,
    MboxSendResult,
    Priorite,
  } from "../contracts";
  import { onMount } from "svelte";
  import { errText, invokeCmd } from "../tauri";

  let messages = $state<MboxMessage[]>([]);
  let error = $state<string | null>(null);
  let count = $state("30");

  let text = $state("");
  let prio = $state<Priorite>("P2");
  let from = $state("");
  let sendResult = $state<string | null>(null);

  async function refresh(): Promise<void> {
    error = null;
    try {
      const n = Number.parseInt(count, 10) || 30;
      messages = await invokeCmd<MboxMessage[]>("mbox_tail", { n });
    } catch (e) {
      error = errText(e);
    }
  }

  onMount(() => void refresh());

  async function send(): Promise<void> {
    if (!text) return;
    error = null;
    sendResult = null;
    try {
      const res = await invokeCmd<MboxSendResult>("mbox_send", {
        message: text,
        priority: prio,
        from: from || undefined,
      });
      sendResult = `${res.status} : ${res.detail}`;
      text = "";
      await refresh();
    } catch (e) {
      error = errText(e);
    }
  }

  async function digest(): Promise<void> {
    error = null;
    try {
      const res = await invokeCmd<{ digests: number }>("mbox_digest");
      sendResult = `${res.digests} digest(s) vidé(s).`;
      await refresh();
    } catch (e) {
      error = errText(e);
    }
  }
</script>

{#if error}<p class="err">{error}</p>{/if}

<div class="panel">
  <h2>Envoyer (inbox)</h2>
  <div class="row">
    <label>
      Priorité
      <select bind:value={prio}>
        <option value="P0">P0</option>
        <option value="P1">P1</option>
        <option value="P2">P2</option>
      </select>
    </label>
    <label>De <input bind:value={from} placeholder="ton nom d'agent" /></label>
  </div>
  <div class="row" style="margin-top: 0.5rem;">
    <input bind:value={text} placeholder="message…" style="flex: 1; min-width: 12rem;" />
    <button class="primary" onclick={() => void send()} disabled={!text}>Envoyer</button>
    <button onclick={() => void digest()}>Digest</button>
  </div>
  {#if sendResult}<p class="dim">{sendResult}</p>{/if}
</div>

<div class="panel">
  <div class="row">
    <h2>Inbox</h2>
    <label class="dim">n <input bind:value={count} style="width: 4rem;" /></label>
    <button onclick={() => void refresh()}>Rafraîchir</button>
  </div>
  {#if messages.length === 0}
    <p class="dim">Boîte vide.</p>
  {:else}
    {#each messages as m, i (`${i}-${m.date}-${m.expediteur}`)}
      <div class="gutter">
        <span class="mono dim">{m.date}</span>
        <strong>{m.expediteur}</strong>
        <span class={m.priorite === "P0" ? "err" : "dim"}>[{m.priorite}]</span>
        <span class="dim">→ {m.dest}</span>
        <p style="margin: 0.2rem 0;">{m.corps}</p>
      </div>
    {/each}
  {/if}
</div>
