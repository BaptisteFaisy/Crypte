<!-- Sidebar — cercueils : une ligne par chat (miroir sidebar.rs).
     `▍` + `●` (mousse repos / feu-follet envoi) + nom + `…` + ` A{n}` +
     ` ?` bougie si question + `[×]` sang. Courant : barre citrouille,
     nom os gras, fond braise. Bouton `[+] new chat` citrouille/sable. -->
<script lang="ts">
  import { activeIndex, chats, closeChat, newChat, runningAgents, selectChat } from "./store";

  async function choose(index: number): Promise<void> {
    if (index !== $activeIndex) await selectChat(index);
  }
</script>

<aside class="crypte-frame coffins" aria-label="Chats">
  <div class="frame-title title">coffins</div>
  {#if $chats.length === 0}
    <p class="empty">no chat — Ctrl+N or click [+] new chat</p>
  {:else}
    <ul>
      {#each $chats as chat (chat.index)}
        {@const current = chat.index === $activeIndex}
        {@const crew = runningAgents(chat.index)}
        <li class:current>
          <button
            class="row"
            class:current
            onclick={() => void choose(chat.index)}
            aria-current={current ? "true" : undefined}
            title="chat {chat.index + 1} — {chat.name}"
          >
            <span class="bar" aria-hidden="true">▍</span>
            <span class="dot" class:sending={chat.sending} aria-hidden="true">●</span>
            <span class="name">{chat.name}</span>
            {#if crew > 0}<span class="crew">A{crew}</span>{/if}
            {#if chat.sending}<span class="mark" aria-label="sending">…</span>{/if}
            {#if chat.questionPending}<span class="mark" aria-label="question pending">?</span>{/if}
          </button>
          <button
            class="close"
            onclick={() => void closeChat(chat.index)}
            aria-label="close chat {chat.index + 1}"
            title="close (Ctrl+W)"
          >
            [×]
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  <button class="plus" onclick={() => void newChat()} title="new chat (Ctrl+N)">
    <span class="head">[+]</span>
    <span class="rest">new chat</span>
  </button>
</aside>

<style>
  .coffins {
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 0.4rem 0.5rem;
  }
  .title {
    color: var(--cry-sand);
    font-size: 0.8rem;
    padding: 0 0.2rem 0.35rem;
  }
  .empty {
    color: var(--cry-sand);
    font-size: 0.8rem;
    padding: 0.4rem 0.2rem;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow-y: auto;
  }
  li {
    display: flex;
    align-items: center;
  }
  li.current {
    background: var(--cry-crypt);
    border-radius: 4px;
  }
  .row {
    flex: 1;
    display: flex;
    align-items: baseline;
    gap: 0.35rem;
    min-width: 0;
    padding: 0.25rem 0.2rem;
    text-align: left;
  }
  .bar {
    color: var(--cry-umber);
  }
  .row.current .bar {
    color: var(--cry-pumpkin);
  }
  .dot {
    color: var(--cry-moss);
    font-size: 0.7rem;
  }
  .dot.sending {
    color: var(--cry-wisp);
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--cry-sand);
  }
  .row.current .name {
    color: var(--cry-bone);
    font-weight: 700;
  }
  .crew {
    color: var(--cry-ecto);
    font-size: 0.8rem;
  }
  .mark {
    color: var(--cry-candle);
  }
  .close {
    color: var(--cry-blood);
    font-weight: 700;
    padding: 0.25rem 0.2rem;
  }
  .plus {
    display: flex;
    gap: 0.4rem;
    padding: 0.35rem 0.2rem 0.1rem;
    text-align: left;
  }
  .plus .head {
    color: var(--cry-pumpkin);
    font-weight: 700;
  }
  .plus .rest {
    color: var(--cry-sand);
  }
</style>
