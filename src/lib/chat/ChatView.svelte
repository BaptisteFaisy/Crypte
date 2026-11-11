<!-- ChatView — historique : étiquette de rôle + gouttière `▍` par
     message (miroir chat/mod.rs : you/os, the crypt/citrouille,
     whisper/cendre italique). Défilement molette/PgUp/Shift+↑↓ avec
     badge `↕ N lines ↓` sur le bord bas (scroll.rs) ; clic = recoller. -->
<script lang="ts">
  import type { ChatMessage, Role } from "../contracts";
  import { linesUp } from "./keys";
  import { chatState } from "./store";

  const TAGS: Record<Role, string> = {
    user: "you",
    assistant: "the crypt",
    system: "whisper",
  };

  interface Props {
    /** Commande de défilement (PgUp/PgDn/Shift+↑↓/Fin depuis la page). */
    scrollCmd?: { seq: number; dir: "up" | "down" | "bottom"; page: boolean } | null;
  }

  let { scrollCmd = null }: Props = $props();
  let pane: HTMLElement | null = $state(null);
  let stuck = $state(true);
  let up = $state(0);
  let seenSeq = 0;

  const LINE_PX = 21;

  function onScroll(): void {
    if (!pane) return;
    const max = pane.scrollHeight - pane.clientHeight;
    stuck = pane.scrollTop >= max - 2;
    up = linesUp(pane.scrollTop, max, LINE_PX);
  }

  function toBottom(): void {
    pane?.scrollTo({ top: pane.scrollHeight });
  }

  function track(msg: ChatMessage, i: number): string {
    return `${i}:${msg.role}:${msg.text.length}`;
  }

  // Nouveau message : suit le bas si on y était collé.
  $effect(() => {
    const count = $chatState?.messages.length ?? 0;
    if (count > 0 && stuck) {
      queueMicrotask(toBottom);
    }
  });

  // Ordre de la page : cran molette (3 lignes) ou page (12 lignes).
  $effect(() => {
    if (!scrollCmd || !pane || scrollCmd.seq === seenSeq) return;
    seenSeq = scrollCmd.seq;
    if (scrollCmd.dir === "bottom") {
      toBottom();
    } else {
      const step = (scrollCmd.page ? 12 : 3) * LINE_PX;
      pane.scrollBy({ top: scrollCmd.dir === "up" ? -step : step });
    }
  });
</script>

<section class="crypte-frame history" aria-label="History">
  <div
    class="crypte-scroll pane"
    bind:this={pane}
    onscroll={onScroll}
    role="log"
    aria-label="Messages"
  >
    {#if !$chatState || $chatState.messages.length === 0}
      <p class="void-msg">the crypt is silent — write below, it listens.</p>
    {:else}
      {#each $chatState.messages as msg, i (track(msg, i))}
        <article class="msg role-{msg.role}">
          <header class="tag">{TAGS[msg.role]}</header>
          {#each msg.text.split("\n") as row}
            <p class="row"><span class="gutter" aria-hidden="true">▍</span><span class="body">{row || " "}</span></p>
          {/each}
        </article>
      {/each}
    {/if}
  </div>
  {#if up > 0}
    <button class="badge" onclick={toBottom} title="back to bottom (End)">
      ↕ {up} {up === 1 ? "line" : "lines"} ↓
    </button>
  {/if}
</section>

<style>
  .history {
    position: relative;
    display: flex;
    min-height: 0;
    overflow: hidden;
  }
  .pane {
    flex: 1;
    overflow-y: auto;
    padding: 0.6rem 0.7rem 0.8rem;
  }
  .void-msg {
    color: var(--cry-sand);
    font-style: italic;
  }
  .msg {
    margin: 0 0 0.7rem;
  }
  .tag {
    font-family: var(--cry-font-title);
    font-size: 0.8rem;
    letter-spacing: 0.06em;
    margin-bottom: 0.1rem;
  }
  .role-user .tag {
    color: var(--cry-bone);
  }
  .role-assistant .tag {
    color: var(--cry-pumpkin);
  }
  .role-system .tag {
    color: var(--cry-ash);
  }
  .row {
    margin: 0;
    display: flex;
    gap: 0.45rem;
  }
  .gutter {
    flex: none;
  }
  .role-user .gutter {
    color: var(--cry-umber);
  }
  .role-assistant .gutter {
    color: var(--cry-pumpkin-dark);
  }
  .role-system .gutter {
    color: var(--cry-umber);
  }
  .body {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .role-user .body {
    color: var(--cry-bone);
  }
  .role-assistant .body {
    color: var(--cry-bone);
  }
  .role-system .body {
    color: var(--cry-sand);
    font-style: italic;
  }
  .badge {
    position: absolute;
    left: 50%;
    bottom: 0.35rem;
    transform: translateX(-50%);
    background: var(--cry-crypt);
    border: 1px solid var(--cry-umber);
    border-radius: 999px;
    color: var(--cry-candle);
    font-family: var(--cry-font-mono);
    font-size: 0.75rem;
    padding: 0.1rem 0.7rem;
  }
</style>
