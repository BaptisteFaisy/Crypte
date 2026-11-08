<!-- Onglet Agents : battements de cœur, métriques, piste d'audit
     (`hb`, `metriques`, `piste`). Frais = battement de moins de 900 s. -->
<script lang="ts">
  import type {
    HbEntry,
    Metriques,
    PisteSpan,
  } from "../contracts";
  import { onMount } from "svelte";
  import { errText, invokeCmd } from "../tauri";

  let heartbeats = $state<HbEntry[]>([]);
  let metriques = $state<Metriques | null>(null);
  let spans = $state<PisteSpan[]>([]);
  let error = $state<string | null>(null);

  let hbName = $state("");
  let hbState = $state("");
  let agentFilter = $state("");

  async function refresh(): Promise<void> {
    error = null;
    try {
      heartbeats = await invokeCmd<HbEntry[]>("mbox_hb_list");
      metriques = await invokeCmd<Metriques>("mbox_metriques", {
        agent: agentFilter || undefined,
      });
      spans = await invokeCmd<PisteSpan[]>("mbox_piste", {
        agent: agentFilter || undefined,
      });
    } catch (e) {
      error = errText(e);
    }
  }

  onMount(() => void refresh());

  async function beat(): Promise<void> {
    if (!hbName || !hbState) return;
    error = null;
    try {
      await invokeCmd<{ ok: boolean }>("mbox_hb", { name: hbName, state: hbState });
      hbState = "";
      await refresh();
    } catch (e) {
      error = errText(e);
    }
  }
</script>

{#if error}<p class="err">{error}</p>{/if}

<div class="panel">
  <div class="row">
    <h2>Agents en vie ({heartbeats.length})</h2>
    <label class="dim">filtre agent <input bind:value={agentFilter} placeholder="tous" /></label>
    <button onclick={() => void refresh()}>Rafraîchir</button>
  </div>
  <div class="row">
    <label>Nom <input bind:value={hbName} placeholder="ton nom d'agent" /></label>
    <label>État <input bind:value={hbState} placeholder="veille…" /></label>
    <button onclick={() => void beat()} disabled={!hbName || !hbState}>Battre</button>
  </div>
  {#if heartbeats.length === 0}
    <p class="dim">Aucun battement.</p>
  {:else}
    <table class="crypt">
      <thead>
        <tr><th>Agent</th><th>État</th><th>Époque</th><th>Frais</th></tr>
      </thead>
      <tbody>
        {#each heartbeats as h (h.nom)}
          <tr>
            <td><strong>{h.nom}</strong></td>
            <td>{h.etat}</td>
            <td class="mono dim">{h.epoque}</td>
            <td class={h.frais ? "ok" : "err"}>{h.frais ? "oui" : "non"}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<div class="panel">
  <h2>Métriques</h2>
  {#if metriques}
    <div class="row" style="align-items: flex-start; gap: 2rem;">
      <div>
        <h3>Compteurs</h3>
        <table class="crypt">
          <tbody>
            {#each Object.entries(metriques.compteurs) as [k, v] (k)}
              <tr><td class="mono">{k}</td><td class="mono">{v}</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
      <div>
        <h3>Intégrité</h3>
        <p class="mono">
          ordres ok {metriques.integrite.ordres.ok} /
          ko {metriques.integrite.ordres.ko} /
          sans {metriques.integrite.ordres.sans}
        </p>
        <table class="crypt">
          <tbody>
            {#each Object.entries(metriques.integrite.signatures) as [k, v] (k)}
              <tr><td class="mono">{k}</td><td class="mono">{v}</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>
  {:else}
    <p class="dim">Chargement…</p>
  {/if}
</div>

<div class="panel">
  <h2>Piste d'audit ({spans.length})</h2>
  {#if spans.length === 0}
    <p class="dim">Aucun span.</p>
  {:else}
    <table class="crypt">
      <thead>
        <tr><th>Run</th><th>Époque</th><th>Agent</th><th>Action</th><th>Ordre</th><th>Verdict</th></tr>
      </thead>
      <tbody>
        {#each spans as s, i (`${i}-${s.runId}-${s.epoque}`)}
          <tr>
            <td class="mono dim">{s.runId}</td>
            <td class="mono dim">{s.epoque}</td>
            <td>{s.agent}</td>
            <td class="mono">{s.action}</td>
            <td class="mono">{s.ordreId}</td>
            <td>{s.verdict}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>
