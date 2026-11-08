<!-- Onglet Ordres : création, liste, révocation, accusés, délégation
     (`ordre new/list/revoque/ack/pull/deleguer`). Id `O-AAAAMMJJ-NNN`. -->
<script lang="ts">
  import type {
    Ordre,
    Priorite,
    Verdict,
  } from "../contracts";
  import { onMount } from "svelte";
  import { errText, invokeCmd } from "../tauri";

  let ordres = $state<Ordre[]>([]);
  let error = $state<string | null>(null);
  let showAll = $state(false);
  let info = $state<string | null>(null);

  let nTo = $state("ALL");
  let nText = $state("");
  let nTtl = $state("24");
  let nPrio = $state<Priorite>("P1");

  let ackFor = $state<string | null>(null);
  let ackVerdict = $state<Verdict>("lu");
  let ackMotif = $state("");
  let ackToken = $state("");

  let pullAgent = $state("");

  async function refresh(): Promise<void> {
    error = null;
    try {
      ordres = await invokeCmd<Ordre[]>("mbox_ordre_list", { all: showAll });
    } catch (e) {
      error = errText(e);
    }
  }

  onMount(() => void refresh());

  function toggleAll(): void {
    showAll = !showAll;
    void refresh();
  }

  async function create(): Promise<void> {
    if (!nTo || !nText) return;
    error = null;
    info = null;
    try {
      const res = await invokeCmd<{ id: string }>("mbox_ordre_new", {
        to: nTo,
        text: nText,
        ttl: Number.parseInt(nTtl, 10) || 24,
        prio: nPrio,
      });
      info = `Ordre ${res.id} créé.`;
      nText = "";
      await refresh();
    } catch (e) {
      error = errText(e);
    }
  }

  async function revoke(id: string, token: string): Promise<void> {
    error = null;
    info = null;
    try {
      await invokeCmd<null>("mbox_ordre_revoke", { id, token });
      info = `Ordre ${id} révoqué.`;
      await refresh();
    } catch (e) {
      error = errText(e);
    }
  }

  async function ack(id: string): Promise<void> {
    if (!ackToken) return;
    if (ackVerdict === "refuse" && !ackMotif) {
      error = "Un motif est exigé pour refuser.";
      return;
    }
    error = null;
    info = null;
    try {
      await invokeCmd<null>("mbox_ordre_ack", {
        id,
        verdict: ackVerdict,
        motif: ackMotif || undefined,
        token: ackToken,
      });
      info = `Accusé ${ackVerdict} envoyé pour ${id}.`;
      ackFor = null;
      ackMotif = "";
      ackToken = "";
      await refresh();
    } catch (e) {
      error = errText(e);
    }
  }

  async function pull(): Promise<void> {
    if (!pullAgent) return;
    error = null;
    try {
      ordres = await invokeCmd<Ordre[]>("mbox_ordre_pull", { agent: pullAgent });
    } catch (e) {
      error = errText(e);
    }
  }
</script>

{#if error}<p class="err">{error}</p>{/if}
{#if info}<p class="ok">{info}</p>{/if}

<div class="panel">
  <h2>Nouvel ordre</h2>
  <div class="row">
    <label>À <input bind:value={nTo} /></label>
    <label>
      Priorité
      <select bind:value={nPrio}>
        <option value="P0">P0</option>
        <option value="P1">P1</option>
        <option value="P2">P2</option>
      </select>
    </label>
    <label>TTL (h, 0 = permanent) <input bind:value={nTtl} style="width: 4rem;" /></label>
  </div>
  <div class="row" style="margin-top: 0.5rem;">
    <input bind:value={nText} placeholder="texte de l'ordre…" style="flex: 1; min-width: 12rem;" />
    <button class="primary" onclick={() => void create()} disabled={!nTo || !nText}>Créer</button>
  </div>
</div>

<div class="panel">
  <div class="row">
    <h2>Ordres ({ordres.length})</h2>
    <button onclick={toggleAll}>{showAll ? "Actifs seuls" : "Tout l'historique"}</button>
    <label>Pull agent <input bind:value={pullAgent} placeholder="@agent" /></label>
    <button onclick={() => void pull()} disabled={!pullAgent}>Pull</button>
    <button onclick={() => void refresh()}>Rafraîchir</button>
  </div>
  {#if ordres.length === 0}
    <p class="dim">Aucun ordre.</p>
  {:else}
    <table class="crypt">
      <thead>
        <tr><th>Id</th><th>À</th><th>Statut</th><th>Texte</th><th>Accusé</th></tr>
      </thead>
      <tbody>
        {#each ordres as o (o.id)}
          <tr>
            <td class="mono">{o.id}</td>
            <td>{o.to}</td>
            <td class={o.statut === "actif" ? "ok" : "dim"}>{o.statut}</td>
            <td>{o.texte}</td>
            <td>
              {#if ackFor === o.id}
                <div class="row">
                  <select bind:value={ackVerdict}>
                    <option value="lu">lu</option>
                    <option value="applique">appliqué</option>
                    <option value="refuse">refusé</option>
                  </select>
                  <input bind:value={ackMotif} placeholder="motif (si refus)" />
                  <input type="password" bind:value={ackToken} autocomplete="off" placeholder="jeton" />
                  <button onclick={() => void ack(o.id)} disabled={!ackToken}>OK</button>
                  <button onclick={() => { ackFor = null; ackToken = ""; }}>X</button>
                </div>
              {:else}
                <div class="row">
                  <button onclick={() => { ackFor = o.id; ackVerdict = "lu"; }}>Accuser</button>
                  <button class="danger" onclick={() => { const t = prompt(`Jeton pour révoquer ${o.id} :`); if (t) void revoke(o.id, t); }}>
                    Révoquer
                  </button>
                </div>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>
