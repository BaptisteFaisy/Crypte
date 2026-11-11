<!-- Onglet Ops : santé, jetons, rotation, vagues, fantôme
     (`doctor`, `jeton`, `rotate`, `vague`, `fantome`).
     Règle d'or : un secret de jeton s'affiche 1x, jamais stocké ni loggué. -->
<script lang="ts">
  import type {
    DoctorCheck,
    JetonInfo,
    VagueState,
  } from "../contracts";
  import { onMount } from "svelte";
  import { errText, invokeCmd } from "../tauri";

  let checks = $state<DoctorCheck[]>([]);
  let jetons = $state<JetonInfo[]>([]);
  let vague = $state<VagueState | null>(null);
  let error = $state<string | null>(null);
  let info = $state<string | null>(null);

  let jetonName = $state("");
  let jetonSecret = $state<string | null>(null);
  let vagueOp = $state("etat");
  let vagueArgs = $state("");
  let fantomeOp = $state("etat");
  let fantomeCmd = $state("");

  async function refresh(): Promise<void> {
    error = null;
    try {
      checks = await invokeCmd<DoctorCheck[]>("mbox_doctor");
      jetons = await invokeCmd<JetonInfo[]>("mbox_jeton_list");
      vague = await invokeCmd<VagueState>("mbox_vague", { op: "etat" });
    } catch (e) {
      error = errText(e);
    }
  }

  onMount(() => void refresh());

  async function newJeton(): Promise<void> {
    if (!jetonName) return;
    error = null;
    info = null;
    jetonSecret = null;
    try {
      const res = await invokeCmd<{ nom: string; secret: string }>("mbox_jeton_new", {
        name: jetonName,
      });
      jetonSecret = res.secret;
      jetonName = "";
      jetons = await invokeCmd<JetonInfo[]>("mbox_jeton_list");
    } catch (e) {
      error = errText(e);
    }
  }

  function clearSecret(): void {
    jetonSecret = null;
  }

  async function revokeJeton(name: string): Promise<void> {
    error = null;
    try {
      await invokeCmd<null>("mbox_jeton_revoke", { name });
      jetons = await invokeCmd<JetonInfo[]>("mbox_jeton_list");
    } catch (e) {
      error = errText(e);
    }
  }

  async function purgeJetons(): Promise<void> {
    error = null;
    try {
      const res = await invokeCmd<{ purged: number }>("mbox_jeton_purge");
      info = `${res.purged} jeton(s) purgé(s).`;
      jetons = await invokeCmd<JetonInfo[]>("mbox_jeton_list");
    } catch (e) {
      error = errText(e);
    }
  }

  async function rotate(): Promise<void> {
    error = null;
    try {
      await invokeCmd<{ ok: boolean }>("mbox_rotate", {});
      info = "Rotation faite (7 j).";
      checks = await invokeCmd<DoctorCheck[]>("mbox_doctor");
    } catch (e) {
      error = errText(e);
    }
  }

  async function runVague(): Promise<void> {
    error = null;
    try {
      vague = await invokeCmd<VagueState>("mbox_vague", {
        op: vagueOp,
        args: vagueArgs || undefined,
      });
    } catch (e) {
      error = errText(e);
    }
  }

  async function runFantome(): Promise<void> {
    error = null;
    info = null;
    try {
      const res = await invokeCmd<{ ok: boolean; detail: string }>("mbox_fantome", {
        op: fantomeOp,
        cmd: fantomeCmd || undefined,
      });
      info = `Fantôme : ${res.detail}`;
    } catch (e) {
      error = errText(e);
    }
  }
</script>

{#if error}<p class="err">{error}</p>{/if}
{#if info}<p class="ok">{info}</p>{/if}

<div class="panel">
  <div class="row">
    <h2>Santé</h2>
    <button onclick={() => void refresh()}>Rafraîchir</button>
    <button onclick={() => void rotate()}>Rotation (7 j)</button>
  </div>
  {#if checks.length === 0}
    <p class="dim">Chargement…</p>
  {:else}
    <table class="crypt">
      <tbody>
        {#each checks as c (c.label)}
          <tr>
            <td class={c.ok ? "ok" : "err"}>{c.ok ? "OK" : "KO"}</td>
            <td><strong>{c.label}</strong></td>
            <td class="dim">{c.detail}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<div class="panel">
  <h2>Jetons ({jetons.length})</h2>
  <div class="row">
    <label>Nom <input bind:value={jetonName} /></label>
    <button class="primary" onclick={() => void newJeton()} disabled={!jetonName}>Créer</button>
    <button onclick={() => void purgeJetons()}>Purger</button>
  </div>
  {#if jetonSecret !== null}
    <p class="err">
      Secret (affiché 1x — copie-le, il ne reviendra pas) :
      <code>{jetonSecret}</code>
      <button onclick={clearSecret}>Effacer</button>
    </p>
  {/if}
  {#if jetons.length > 0}
    <table class="crypt">
      <tbody>
        {#each jetons as j (j.nom)}
          <tr>
            <td class="mono">{j.nom}</td>
            <td class={j.revoque ? "err" : "ok"}>{j.revoque ? "révoqué" : "actif"}</td>
            <td><button class="danger" onclick={() => void revokeJeton(j.nom)}>Révoquer</button></td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<div class="panel">
  <h2>Vague</h2>
  <div class="row">
    <select bind:value={vagueOp} aria-label="Opération vague">
      <option value="plan">plan</option>
      <option value="run">run</option>
      <option value="lister">lister</option>
      <option value="etat">etat</option>
    </select>
    <input bind:value={vagueArgs} placeholder="args…" />
    <button onclick={() => void runVague()}>Exécuter</button>
  </div>
  {#if vague}
    <p class="mono">en attente {vague.pending} — en cours {vague.running} — fait {vague.done}</p>
    <p class="dim">{vague.detail}</p>
  {/if}
</div>

<div class="panel">
  <h2>Fantôme (miroir)</h2>
  <div class="row">
    <select bind:value={fantomeOp} aria-label="Opération fantôme">
      <option value="rejouer">rejouer</option>
      <option value="diff">diff</option>
      <option value="etat">etat</option>
      <option value="purge">purge</option>
    </select>
    <input bind:value={fantomeCmd} placeholder="commande…" />
    <button onclick={() => void runFantome()}>Exécuter</button>
  </div>
</div>
