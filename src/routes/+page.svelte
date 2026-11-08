<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  type StagedView = {
    id: number;
    name: string;
    mime: string;
    size: number;
    dataUrl: string;
  };

  type ProviderState = {
    configured: boolean;
    base_url?: string;
    model?: string;
  };

  type SendResult = {
    reply: string;
    prompt: number;
    completion: number;
    imagesUsed: number;
  };

  type Exchange = {
    text: string;
    imagesUsed: number;
    reply: string;
    prompt: number;
    completion: number;
  };

  let provider = $state<ProviderState>({ configured: false });
  let staged = $state<StagedView[]>([]);
  let exchanges = $state<Exchange[]>([]);
  let text = $state("");
  let path = $state("");
  let sending = $state(false);
  let busy = $state(false);
  let error = $state("");

  refreshProvider();
  refreshStaged();

  async function refreshProvider() {
    try {
      provider = await invoke<ProviderState>("provider_status");
    } catch (e) {
      error = String(e);
    }
  }

  async function refreshStaged() {
    try {
      staged = await invoke<StagedView[]>("staged_images");
    } catch (e) {
      error = String(e);
    }
  }

  function humanSize(bytes: number): string {
    if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${bytes} B`;
  }

  async function attachFiles(files: FileList | null) {
    if (!files || files.length === 0) return;
    busy = true;
    error = "";
    try {
      for (const file of files) {
        const dataUrl: string = await new Promise((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => resolve(String(reader.result));
          reader.onerror = () => reject(new Error(`cannot read ${file.name}`));
          reader.readAsDataURL(file);
        });
        const b64 = dataUrl.split(",", 2)[1] ?? "";
        await invoke("attach_image_bytes", { name: file.name, b64 });
      }
      await refreshStaged();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function attachPath(event: Event) {
    event.preventDefault();
    if (path.trim() === "") return;
    busy = true;
    error = "";
    try {
      await invoke("attach_image_path", { path: path.trim() });
      path = "";
      await refreshStaged();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function removeImage(id: number) {
    try {
      await invoke("remove_image", { id });
      await refreshStaged();
    } catch (e) {
      error = String(e);
    }
  }

  async function clearImages() {
    try {
      await invoke("clear_images");
      await refreshStaged();
    } catch (e) {
      error = String(e);
    }
  }

  async function send(event: Event) {
    event.preventDefault();
    if (sending || (text.trim() === "" && staged.length === 0)) return;
    sending = true;
    error = "";
    try {
      const sent = text;
      const done = await invoke<SendResult>("send_message", { text: sent });
      exchanges = [
        ...exchanges,
        {
          text: sent,
          imagesUsed: done.imagesUsed,
          reply: done.reply,
          prompt: done.prompt,
          completion: done.completion,
        },
      ];
      text = "";
      await refreshStaged();
    } catch (e) {
      error = String(e);
    } finally {
      sending = false;
    }
  }
</script>

<main>
  <header>
    <h1>crypte</h1>
    {#if provider.configured}
      <p class="status ok">{provider.base_url} · {provider.model}</p>
    {:else}
      <p class="status missing">provider missing — set KH_API_KEY (KH_BASE_URL, KH_MODEL optional), then restart</p>
    {/if}
  </header>

  {#if error !== ""}
    <p class="error" role="alert">{error}</p>
  {/if}

  <section aria-label="staged images">
    <div class="staged-row">
      {#each staged as shot (shot.id)}
        <figure>
          <img src={shot.dataUrl} alt={shot.name} />
          <button type="button" onclick={() => removeImage(shot.id)} aria-label="remove {shot.name}">×</button>
          <figcaption>{shot.name} · {humanSize(shot.size)}</figcaption>
        </figure>
      {/each}
      {#if staged.length === 0}
        <p class="hint">no image staged — pick a file or paste a path (png, jpeg, gif, webp · 5 MB max · 4 per message)</p>
      {/if}
    </div>
    <div class="attach-row">
      <label class="file">
        <input
          type="file"
          accept="image/png,image/jpeg,image/gif,image/webp"
          multiple
          disabled={busy}
          onchange={(e) => attachFiles(e.currentTarget.files)}
        />
        {busy ? "attaching…" : "choose files…"}
      </label>
      <form class="path" onsubmit={attachPath}>
        <input placeholder="/path/to/photo.png" bind:value={path} disabled={busy} aria-label="image path" />
        <button type="submit" disabled={busy}>stage path</button>
      </form>
      {#if staged.length > 0}
        <button type="button" class="ghost" onclick={clearImages}>clear all</button>
      {/if}
    </div>
  </section>

  <section aria-label="exchanges">
    {#each exchanges as x (x)}
      <article>
        <p class="you">you{#if x.imagesUsed > 0} · {x.imagesUsed} image{x.imagesUsed > 1 ? "s" : ""}{/if}</p>
        <p class="text">{x.text === "" ? "(image only)" : x.text}</p>
        <p class="crypt">the crypt</p>
        <p class="text reply">{x.reply === "" ? "(empty reply)" : x.reply}</p>
        <p class="usage">{x.prompt} + {x.completion} tokens</p>
      </article>
    {/each}
    {#if exchanges.length === 0}
      <p class="hint">no exchange yet — stage an image, write a message, send.</p>
    {/if}
  </section>

  <form class="send" onsubmit={send}>
    <textarea
      placeholder="whisper to the crypt…"
      bind:value={text}
      rows="3"
      aria-label="message"
    ></textarea>
    <button type="submit" disabled={sending || (text.trim() === "" && staged.length === 0)}>
      {sending ? "summoning…" : "send"}
    </button>
  </form>
</main>

<style>
  :root {
    color-scheme: dark;
  }

  main {
    max-width: 46rem;
    margin: 0 auto;
    padding: 1.5rem 1rem 3rem;
    font-family: ui-sans-serif, system-ui, sans-serif;
    background: #14100c;
    color: #e8dcc8;
    min-height: 100vh;
  }

  header {
    border-bottom: 1px solid #3a2c1e;
    padding-bottom: 0.75rem;
    margin-bottom: 1rem;
  }

  h1 {
    font-family: Georgia, "Times New Roman", serif;
    font-size: 1.75rem;
    letter-spacing: 0.04em;
    margin: 0;
    color: #f5edd8;
  }

  .status {
    margin: 0.25rem 0 0;
    font-size: 0.85rem;
    color: #a89a80;
  }

  .status.ok {
    color: #8fae7a;
  }

  .error {
    background: #3d1414;
    border: 1px solid #7a2a2a;
    padding: 0.6rem 0.8rem;
    font-size: 0.9rem;
  }

  .hint {
    color: #a89a80;
    font-size: 0.9rem;
    font-style: italic;
  }

  .staged-row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem;
    margin-bottom: 0.75rem;
  }

  figure {
    position: relative;
    margin: 0;
    width: 8rem;
  }

  figure img {
    display: block;
    width: 8rem;
    height: 6rem;
    object-fit: cover;
    background: #000;
  }

  figcaption {
    font-size: 0.75rem;
    color: #a89a80;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  figure button {
    position: absolute;
    top: 0;
    right: 0;
    padding: 0 0.4rem;
    font-size: 1rem;
    line-height: 1.4;
  }

  .attach-row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    align-items: center;
    margin-bottom: 1.25rem;
  }

  .file {
    cursor: pointer;
  }

  .file input {
    display: none;
  }

  .path {
    display: flex;
    gap: 0.5rem;
    flex: 1 1 16rem;
  }

  .path input {
    flex: 1;
  }

  input,
  textarea,
  button,
  .file {
    font: inherit;
    color: #f5edd8;
    background: #241a10;
    border: 1px solid #5a452c;
    padding: 0.5rem 0.7rem;
  }

  button {
    cursor: pointer;
    background: #4a2f14;
    white-space: nowrap;
  }

  button:hover:not(:disabled) {
    background: #5d3c18;
  }

  button:disabled {
    cursor: default;
    opacity: 0.55;
  }

  button.ghost {
    background: transparent;
  }

  textarea {
    width: 100%;
    resize: vertical;
  }

  article {
    border-top: 1px solid #3a2c1e;
    padding: 0.75rem 0;
  }

  .you,
  .crypt {
    font-size: 0.8rem;
    margin: 0;
    color: #d08c3a;
  }

  .text {
    margin: 0.25rem 0 0.75rem;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .usage {
    font-size: 0.8rem;
    color: #a89a80;
    margin: 0;
  }

  .send {
    display: flex;
    gap: 0.5rem;
    margin-top: 1.25rem;
    align-items: flex-end;
  }

  .send textarea {
    flex: 1;
  }
</style>
