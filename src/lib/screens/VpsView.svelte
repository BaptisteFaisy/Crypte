<!-- Écran VPS : profils + opérations distantes (commandes PLAN §10).
     V1 = comportement TUI (ssh BatchMode, tmux `crypte`) ; résultats async
     via event `crypte-ui` / `vps_outcome`. -->
<script lang="ts">
  import type {
    CrypteUiEvent,
    VpsAck,
    VpsProfile,
    VpsState,
  } from "../contracts";
  import { onDestroy, onMount } from "svelte";
  import { errText, invokeCmd, listenCrypte } from "../tauri";

  let vps = $state<VpsState | null>(null);
  let error = $state<string | null>(null);
  let target = $state("");
  let log = $state<string[]>([]);
  let unlisten: (() => void) | null = null;

  let fName = $state("");
  let fHost = $state("");
  let fUser = $state("baptiste");
  let fPort = $state("22");
  let fDir = $state("");

  function pushLog(line: string): void {
    log = [...log.slice(-99), line];
  }

  async function refresh(): Promise<void> {
    error = null;
    try {
      vps = await invokeCmd<VpsState>("vps_list");
      if (!target && vps.profiles.length > 0) {
        target = vps.profiles[0]?.name ?? "";
      }
    } catch (e) {
      error = errText(e);
    }
  }

  onMount(() => {
    void refresh();
    void listenCrypte("crypte-ui", (ev) => {
      if (ev.kind !== "vps_outcome") return;
      pushLog(`[${ev.target} ${ev.op}] ${ev.ok ? "OK" : "KO"} : ${ev.text}`);
      void refresh();
    }).then((u) => {
      unlisten = u;
    });
  });

  onDestroy(() => unlisten?.());

  async function runOp(cmd: string, withTarget: boolean): Promise<void> {
    error = null;
    try {
      const ack = await invokeCmd<VpsAck>(
        cmd,
        withTarget ? { target } : target ? { target } : {},
      );
      pushLog(`[${cmd}] ${ack.accepted ? "accepté" : "refusé"} : ${ack.detail}`);
    } catch (e) {
      error = errText(e);
    }
  }

  async function addProfile(): Promise<void> {
    if (!fName || !fHost) return;
    error = null;
    try {
      const profile: VpsProfile = {
        name: fName,
        host: fHost,
        user: fUser || "baptiste",
        port: Number.parseInt(fPort, 10) || 22,
        key: null,
        dir: fDir || "/home/baptiste/crypte",
        dst: `${fUser || "baptiste"}@${fHost}`,
      };
      await invokeCmd<null>("vps_add", { profile });
      fName = "";
      fHost = "";
      fDir = "";
      await refresh();
    } catch (e) {
      error = errText(e);
    }
  }

  async function removeProfile(name: string): Promise<void> {
    error = null;
    try {
      await invokeCmd<null>("vps_remove", { name });
      if (target === name) target = "";
      await refresh();
    } catch (e) {
      error = errText(e);
    }
  }
</script>

<h1>VPS distant</h1>
{#if error}<p class="err">{error}</p>{/if}

<div class="panel">
  <h2>Profils</h2>
  {#if vps}
    <table class="crypt">
      <thead>
        <tr><th>Nom</th><th>Destination</th><th>Dossier</th><th>Statut</th><th></th></tr>
      </thead>
      <tbody>
        {#each vps.profiles as p (p.name)}
          <tr>
            <td>
              <button onclick={() => { target = p.name; }} title="Cibler">
                {p.name}{vps.current === p.name ? " (actif)" : ""}
              </button>
            </td>
            <td class="mono dim">{p.dst}</td>
            <td class="mono dim">{p.dir}</td>
            <td>{vps.statusByName[p.name] ?? "—"}</td>
            <td><button class="danger" onclick={() => void removeProfile(p.name)}>Suppr.</button></td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="dim">Chargement…</p>
  {/if}
</div>

<div class="panel">
  <h2>Opérations</h2>
  <div class="row">
    <label>
      Cible
      <input bind:value={target} placeholder="nom ou user@host" />
    </label>
    <button class="primary" onclick={() => void runOp("vps_launch", true)} disabled={!target}>Lancer</button>
    <button onclick={() => void runOp("vps_status", false)}>Statut</button>
    <button onclick={() => void runOp("vps_install", true)} disabled={!target}>Installer</button>
    <button class="danger" onclick={() => void runOp("vps_stop", false)}>Arrêter</button>
  </div>
</div>

<div class="panel">
  <h2>Ajouter un profil</h2>
  <div class="row">
    <label>Nom <input bind:value={fName} /></label>
    <label>Hôte <input bind:value={fHost} placeholder="203.0.113.7" /></label>
    <label>User <input bind:value={fUser} /></label>
    <label>Port <input bind:value={fPort} /></label>
    <label>Dossier <input bind:value={fDir} placeholder="/home/baptiste/crypte" /></label>
    <button onclick={() => void addProfile()} disabled={!fName || !fHost}>Ajouter</button>
  </div>
</div>

<div class="panel">
  <h2>Journal</h2>
  {#if log.length === 0}
    <p class="dim">Aucune opération pour l'instant.</p>
  {:else}
    {#each log as line, i (`${i}-${line}`)}
      <p class="mono gutter">{line}</p>
    {/each}
  {/if}
</div>
