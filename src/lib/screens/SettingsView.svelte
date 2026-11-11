<!-- Écran Settings : providers + connexion (commandes PLAN §3).
     Les clés ne s'affichent jamais : saisie masquée, `hasKey` seul en lecture. -->
<script lang="ts">
  import type {
    ConnectStatus,
    ProviderInfo,
  } from "../contracts";
  import { onMount } from "svelte";
  import { errText, invokeCmd } from "../tauri";

  let status = $state<ConnectStatus | null>(null);
  let providers = $state<ProviderInfo[]>([]);
  let error = $state<string | null>(null);

  let connProvider = $state("");
  let connKey = $state("");
  let connModel = $state("");
  let connBusy = $state(false);

  let newName = $state("");
  let newBaseUrl = $state("");
  let newModel = $state("");
  let keyFor = $state<string | null>(null);
  let keyValue = $state("");

  async function refresh(): Promise<void> {
    error = null;
    try {
      status = await invokeCmd<ConnectStatus>("connect_status");
      providers = await invokeCmd<ProviderInfo[]>("providers_list");
      if (!connProvider && providers.length > 0) {
        connProvider = providers[0]?.name ?? "";
      }
    } catch (e) {
      error = errText(e);
    }
  }

  onMount(() => void refresh());

  async function saveConnect(): Promise<void> {
    if (!connProvider || !connKey) return;
    connBusy = true;
    error = null;
    try {
      const res = await invokeCmd<{
        ok: boolean;
        error: string | null;
        status: ConnectStatus;
      }>("connect_save", {
        provider: connProvider,
        key: connKey,
        model: connModel || undefined,
      });
      status = res.status;
      if (!res.ok) error = res.error ?? "connection failed";
      connKey = "";
      providers = await invokeCmd<ProviderInfo[]>("providers_list");
    } catch (e) {
      error = errText(e);
    } finally {
      connBusy = false;
    }
  }

  async function connectMuse(): Promise<void> {
    error = null;
    try {
      const res = await invokeCmd<{
        ok: boolean;
        error: string | null;
        status: ConnectStatus;
      }>("connect_muse");
      status = res.status;
      if (!res.ok) error = res.error ?? "connection failed";
    } catch (e) {
      error = errText(e);
    }
  }

  async function selectProvider(name: string): Promise<void> {
    error = null;
    try {
      status = await invokeCmd<ConnectStatus>("provider_select", { name });
    } catch (e) {
      error = errText(e);
    }
  }

  async function addProvider(): Promise<void> {
    if (!newName || !newBaseUrl) return;
    error = null;
    try {
      await invokeCmd<null>("provider_add", {
        name: newName,
        baseUrl: newBaseUrl,
        model: newModel || undefined,
      });
      newName = "";
      newBaseUrl = "";
      newModel = "";
      providers = await invokeCmd<ProviderInfo[]>("providers_list");
    } catch (e) {
      error = errText(e);
    }
  }

  async function removeProvider(name: string): Promise<void> {
    error = null;
    try {
      await invokeCmd<null>("provider_remove", { name });
      providers = await invokeCmd<ProviderInfo[]>("providers_list");
    } catch (e) {
      error = errText(e);
    }
  }

  async function setKey(name: string): Promise<void> {
    if (!keyValue) return;
    error = null;
    try {
      await invokeCmd<null>("provider_set_key", { name, key: keyValue });
      keyFor = null;
      keyValue = "";
      providers = await invokeCmd<ProviderInfo[]>("providers_list");
    } catch (e) {
      error = errText(e);
    }
  }
</script>

<h1>Réglages</h1>
{#if error}<p class="err">{error}</p>{/if}

<div class="panel">
  <h2>Connexion</h2>
  {#if status}
    <p>
      {#if status.connected}
        <span class="ok">Connecté</span> — {status.provider ?? "?"} /
        {status.model ?? "modèle défaut"}
      {:else}
        <span class="err">Non connecté</span> — choisis un provider et sa clé.
      {/if}
    </p>
  {:else}
    <p class="dim">Chargement…</p>
  {/if}
  <div class="row">
    <label>
      Provider
      <select bind:value={connProvider}>
        {#each providers as p (p.name)}
          <option value={p.name}>{p.name}</option>
        {/each}
      </select>
    </label>
    <label>
      Clé API
      <input type="password" bind:value={connKey} autocomplete="off" placeholder="jamais affichée" />
    </label>
    <label>
      Modèle (optionnel)
      <input bind:value={connModel} placeholder="défaut du provider" />
    </label>
    <button class="primary" onclick={() => void saveConnect()} disabled={connBusy || !connKey}>
      Connecter
    </button>
    <button onclick={() => void connectMuse()}>Connecter via Muse</button>
  </div>
</div>

<div class="panel">
  <h2>Providers ({providers.length})</h2>
  <table class="crypt">
    <thead>
      <tr><th>Nom</th><th>Base URL</th><th>Modèle</th><th>Clé</th><th></th></tr>
    </thead>
    <tbody>
      {#each providers as p (p.name)}
        <tr>
          <td>
            <button onclick={() => void selectProvider(p.name)} title="Provider actif">
              {p.name}{p.known ? "" : " (custom)"}
            </button>
          </td>
          <td class="mono dim">{p.baseUrl}</td>
          <td class="mono">{p.model}</td>
          <td>{p.hasKey ? "•" : "—"}</td>
          <td>
            {#if keyFor === p.name}
              <span class="row">
                <input type="password" bind:value={keyValue} autocomplete="off" placeholder="nouvelle clé" />
                <button onclick={() => void setKey(p.name)} disabled={!keyValue}>OK</button>
                <button onclick={() => { keyFor = null; keyValue = ""; }}>X</button>
              </span>
            {:else}
              <span class="row">
                <button onclick={() => { keyFor = p.name; keyValue = ""; }}>Clé</button>
                {#if !p.known}
                  <button class="danger" onclick={() => void removeProvider(p.name)}>Suppr.</button>
                {/if}
              </span>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<div class="panel">
  <h2>Ajouter un provider custom</h2>
  <div class="row">
    <label>Nom <input bind:value={newName} /></label>
    <label>Base URL <input bind:value={newBaseUrl} placeholder="https://…" /></label>
    <label>Modèle <input bind:value={newModel} /></label>
    <button onclick={() => void addProvider()} disabled={!newName || !newBaseUrl}>Ajouter</button>
  </div>
</div>
