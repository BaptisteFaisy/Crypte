<!-- ConnectModal — flow /connect (connect.rs) : provider → clé → modèle.
  La clé est saisie masquée, envoyée une fois via connect_save, puis
  effacée ; elle n'est jamais affichée, logguée ni stockée côté front. -->
<script lang="ts">
  import type {
    ConnectResult,
    ConnectStatus,
    ProviderInfo,
  } from "../contracts";
  import { shell } from "../crypte.svelte";
  import { errText, invokeCmd } from "../tauri";

  let providers: ProviderInfo[] = $state([]);
  let status: ConnectStatus | null = $state(null);
  let provider = $state("");
  let key = $state("");
  let model = $state("");
  let busy = $state(false);
  let result: ConnectResult | null = $state(null);
  let loadError = $state<string | null>(null);

  async function refresh(): Promise<void> {
    try {
      status = await invokeCmd<ConnectStatus>("connect_status");
      providers = await invokeCmd<ProviderInfo[]>("providers_list");
      if (!provider && providers.length > 0) provider = providers[0].name;
      loadError = null;
    } catch (err) {
      loadError = errText(err);
    }
  }

  $effect(() => {
    void refresh();
  });

  function close(): void {
    // La clé ne survit jamais à la modale.
    key = "";
    shell.connectOpen = false;
  }

  function onKey(event: KeyboardEvent): void {
    if (event.key === "Escape") close();
  }

  async function save(event: Event): Promise<void> {
    event.preventDefault();
    if (!provider || !key) return;
    busy = true;
    try {
      result = await invokeCmd<ConnectResult>("connect_save", {
        provider,
        key,
        model: model.trim() || undefined,
      });
      status = result.status;
      if (result.ok) {
        shell.toast("success", `connecté à ${provider}`);
        close();
      }
    } catch (err) {
      shell.toast("error", errText(err));
    } finally {
      key = "";
      busy = false;
    }
  }

  async function connectClaude(): Promise<void> {
    busy = true;
    try {
      result = await invokeCmd<ConnectResult>("connect_muse");
      status = result.status;
      if (result.ok) {
        shell.toast("success", "connecté à muse");
        close();
      } else if (result.error) {
        shell.toast("error", result.error);
      }
    } catch (err) {
      shell.toast("error", errText(err));
    } finally {
      busy = false;
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div
  class="veil"
  data-testid="connect-modal"
  role="dialog"
  aria-modal="true"
  aria-label="Connexion"
>
  <div class="card">
    <h2>connect</h2>
    {#if loadError}
      <p class="error" role="alert">{loadError}</p>
    {:else if status?.connected}
      <p class="connected">
        connected to {status.provider} ({status.model})
      </p>
    {:else}
      <p class="hint">not connected — pick a provider, paste a key.</p>
    {/if}
    <form class="form" onsubmit={save}>
      <label>
        provider
        <select
          class="crypte-select"
          bind:value={provider}
          data-testid="connect-provider"
        >
          {#each providers as p (p.name)}
            <option value={p.name}>
              {p.name}{p.hasKey ? " (key saved)" : ""}
            </option>
          {/each}
        </select>
      </label>
      <label>
        api key
        <input
          class="crypte-input"
          type="password"
          autocomplete="off"
          spellcheck={false}
          placeholder="sk-…"
          bind:value={key}
          data-testid="connect-key"
        />
      </label>
      <label>
        model (optional)
        <input
          class="crypte-input"
          autocomplete="off"
          spellcheck={false}
          placeholder="default"
          bind:value={model}
          data-testid="connect-model"
        />
      </label>
      {#if result && !result.ok && result.error}
        <p class="error" role="alert">{result.error}</p>
      {/if}
      <div class="actions">
        <button
          class="crypte-btn"
          type="submit"
          disabled={busy || !provider || !key}
          data-testid="connect-save"
        >
          {busy ? "…" : "save"}
        </button>
        <button
          class="crypte-btn ghost"
          type="button"
          disabled={busy}
          onclick={connectClaude}
          data-testid="connect-Muse"
        >
          muse
        </button>
        <button
          class="crypte-btn ghost"
          type="button"
          onclick={close}
        >
          close
        </button>
      </div>
    </form>
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
    width: min(28rem, 100%);
    background: var(--cry-crypt);
    border: 1px solid var(--cry-umber);
    border-radius: 0.5rem;
    padding: 1rem 1.25rem;
  }
  h2 {
    margin: 0 0 0.5rem;
    font-size: 1rem;
    color: var(--cry-pumpkin);
  }
  .connected {
    color: var(--cry-ecto);
  }
  .hint {
    color: var(--cry-sand);
  }
  .error {
    color: var(--cry-blood);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    color: var(--cry-sand);
    font-size: 0.85em;
  }
  .actions {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.25rem;
  }
  .crypte-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
