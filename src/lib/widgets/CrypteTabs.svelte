<!-- CrypteTabs — onglets uniques (roving tabindex, flèches).
     Le consommateur rend le panneau selon `active` (id “…-panel”). -->
<script lang="ts">
  import type { WidgetTab } from "./types";

  interface Props {
    tabs: WidgetTab[];
    active?: string;
    ariaLabel?: string;
    onchange?: (id: string) => void;
  }

  let {
    tabs,
    active = $bindable(""),
    ariaLabel = "Onglets",
    onchange,
  }: Props = $props();

  let bar: HTMLDivElement | undefined = $state(undefined);

  function usable(): WidgetTab[] {
    return tabs.filter((t) => !t.disabled);
  }

  function go(id: string): void {
    const tab = tabs.find((t) => t.id === id);
    if (!tab || tab.disabled) return;
    active = id;
    onchange?.(id);
  }

  function focusTab(id: string): void {
    bar?.querySelector<HTMLElement>(`[data-tab="${id}"]`)?.focus();
  }

  function handleKey(e: KeyboardEvent): void {
    const list = usable();
    if (list.length === 0) return;
    const at = Math.max(
      0,
      list.findIndex((t) => t.id === active),
    );
    let next: number | null = null;
    if (e.key === "ArrowRight" || e.key === "ArrowDown") {
      next = (at + 1) % list.length;
    } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
      next = (at - 1 + list.length) % list.length;
    } else if (e.key === "Home") {
      next = 0;
    } else if (e.key === "End") {
      next = list.length - 1;
    }
    if (next !== null) {
      e.preventDefault();
      go(list[next].id);
      focusTab(list[next].id);
    }
  }
</script>

<div
  bind:this={bar}
  class="cry-tabs"
  role="tablist"
  tabindex="-1"
  aria-label={ariaLabel}
  onkeydown={handleKey}
>
  {#each tabs as tab (tab.id)}
    <button
      type="button"
      class="cry-tabs__tab"
      role="tab"
      data-tab={tab.id}
      id="tab-{tab.id}"
      aria-selected={tab.id === active}
      aria-controls="{tab.id}-panel"
      tabindex={tab.id === active ? 0 : -1}
      disabled={tab.disabled}
      onclick={() => go(tab.id)}
    >
      {tab.label}
      {#if tab.badge !== undefined && tab.badge !== null}
        <span class="cry-tabs__badge">{tab.badge}</span>
      {/if}
    </button>
  {/each}
</div>
