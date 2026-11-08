<!-- Route `/` (ChatPage front-chat) : sidebar cercueils + historique + saisie +
     colonne droite (apparition + Kiro + hub à venir), fx en fond.
     Clavier global (keys.ts) : Ctrl+N/W, Ctrl+C, PgUp/PgDn,
     Shift+↑↓, Fin, Échap. Souris : clics natifs + molette. -->
<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import ApparitionPanel from "../lib/chat/ApparitionPanel.svelte";
  import ChatInput from "../lib/chat/ChatInput.svelte";
  import ChatView from "../lib/chat/ChatView.svelte";
  import FxOverlay from "../lib/chat/FxOverlay.svelte";
  import KiroWidget from "../lib/chat/KiroWidget.svelte";
  import ToolConfirm from "../lib/chat/ToolConfirm.svelte";
  import Sidebar from "../lib/chat/Sidebar.svelte";
  import { keyAction } from "../lib/chat/keys";
  import {
    activeIndex,
    cancelConnect,
    chats,
    closeChat,
    connectFlow,
    destroyChat,
    initChat,
    newChat,
    stop,
  } from "../lib/chat/store";

  let ready = $state(false);
  let loadError = $state<string | null>(null);
  let scrollCmd = $state<{ seq: number; dir: "up" | "down" | "bottom"; page: boolean } | null>(null);
  let seq = 0;

  onMount(() => {
    initChat()
      .then(() => (ready = true))
      .catch((e: unknown) => {
        loadError = e instanceof Error ? e.message : String(e);
      });
  });

  onDestroy(destroyChat);

  function inField(target: EventTarget | null): boolean {
    return target instanceof HTMLTextAreaElement || target instanceof HTMLInputElement;
  }

  async function onKey(e: KeyboardEvent): Promise<void> {
    // La saisie gère Entrée/Échap elle-même (commit/cancel du flow).
    if (inField(e.target) && (e.key === "Enter" || e.key === "Escape")) return;
    const action = keyAction({
      key: e.key,
      ctrl: e.ctrlKey || e.metaKey,
      shift: e.shiftKey,
      inInput: false,
    });
    if (!action) return;
    if (["new-chat", "close-chat", "scroll-up", "scroll-down", "scroll-bottom"].includes(action)) {
      e.preventDefault();
    }
    await runAction(action, e);
  }

  async function runAction(
    action: NonNullable<ReturnType<typeof keyAction>>,
    e: KeyboardEvent,
  ): Promise<void> {
    switch (action) {
      case "new-chat":
        await newChat();
        break;
      case "close-chat":
        if ($chats.length > 0) await closeChat($activeIndex);
        break;
      case "stop":
        await stop();
        break;
      case "cancel-flow":
        cancelConnect();
        break;
      case "scroll-up":
      case "scroll-down":
      case "scroll-bottom":
        seq += 1;
        scrollCmd = {
          seq,
          dir: action === "scroll-bottom" ? "bottom" : action === "scroll-up" ? "up" : "down",
          page: e.key === "PageUp" || e.key === "PageDown",
        };
        break;
      case "send":
        break;
    }
  }
</script>

<svelte:window onkeydown={(e) => void onKey(e)} />

<div class="crypte-chat page">
  <FxOverlay />
  <ToolConfirm />
  {#if loadError}
    <p class="fatal" role="alert">the crypt did not wake: {loadError}</p>
  {:else if !ready}
    <p class="fatal">waking the crypt…</p>
  {:else}
    <div class="cols">
      <Sidebar />
      <main class="center">
        <ChatView {scrollCmd} />
        <ChatInput />
      </main>
      <div class="right">
        <ApparitionPanel />
        <KiroWidget />
        {#if $connectFlow}
          <p class="flow-hint">connect flow — Esc cancels</p>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .page {
    position: relative;
    height: 100%;
    min-height: 0;
  }
  .fatal {
    position: relative;
    z-index: 1;
    color: var(--cry-sand);
    padding: 2rem;
    text-align: center;
  }
  .cols {
    position: relative;
    z-index: 1;
    display: grid;
    grid-template-columns: 15rem 1fr 13rem;
    gap: 0.6rem;
    height: 100%;
    min-height: 0;
    padding: 0.6rem;
    box-sizing: border-box;
  }
  .center {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    min-height: 0;
    min-width: 0;
  }
  .center :global(.history) {
    flex: 1;
  }
  .right {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    min-height: 0;
    overflow-y: auto;
  }
  .flow-hint {
    color: var(--cry-umber);
    font-size: 0.75rem;
    margin: 0;
    text-align: center;
  }
  @media (max-width: 70rem) {
    .cols {
      grid-template-columns: 12rem 1fr;
    }
    .right {
      display: none;
    }
  }
</style>
