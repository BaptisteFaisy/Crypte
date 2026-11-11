<!-- CrypteMenu — menu déroulant unique (déclencheur + popup, clavier ↑↓⤶).
     Types MenuItem/MenuEntry dans types.ts. Styles : crypte.css .cry-menu*. -->
<script lang="ts">
  import type { MenuEntry, MenuItem } from "./types";

  interface Props {
    label: string;
    items: MenuEntry[];
    open?: boolean;
    align?: "start" | "end";
    disabled?: boolean;
    menuLabel?: string;
    onpick?: (id: string) => void;
  }

  let {
    label,
    items,
    open = $bindable(false),
    align = "start",
    disabled = false,
    menuLabel,
    onpick,
  }: Props = $props();

  let root: HTMLDivElement | undefined = $state(undefined);
  let hot = $state(0);

  const pickable: MenuItem[] = $derived(
    items.filter((e): e is MenuItem => !("separator" in e)),
  );

  function toggle(): void {
    if (disabled) return;
    open = !open;
    hot = 0;
  }

  function close(): void {
    open = false;
  }

  function pick(id: string): void {
    close();
    onpick?.(id);
  }

  function step(dir: 1 | -1): void {
    if (pickable.length === 0) return;
    let n = hot;
    for (let k = 0; k < pickable.length; k += 1) {
      n = (n + dir + pickable.length) % pickable.length;
      if (!pickable[n].disabled) {
        hot = n;
        return;
      }
    }
  }

  function focusHot(): void {
    root?.querySelector<HTMLElement>(`[data-item="${hot}"]`)?.focus();
  }

  function handleMenuKey(e: KeyboardEvent): void {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      step(1);
      focusHot();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      step(-1);
      focusHot();
    } else if (e.key === "Home") {
      e.preventDefault();
      hot = 0;
      focusHot();
    } else if (e.key === "End") {
      e.preventDefault();
      hot = pickable.length - 1;
      focusHot();
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }

  function handleWindowClick(e: MouseEvent): void {
    if (open && root && !root.contains(e.target as Node)) close();
  }
</script>

<svelte:window onclick={handleWindowClick} />

<div bind:this={root} class="cry-menu">
  <button
    type="button"
    class="cry-btn cry-btn--subtle"
    aria-haspopup="menu"
    aria-expanded={open}
    {disabled}
    onclick={toggle}
  >
    {label} ▾
  </button>
  {#if open}
    <div
      class="cry-menu__popup"
      class:cry-menu__popup--end={align === "end"}
      role="menu"
      tabindex="-1"
      aria-label={menuLabel ?? label}
      onkeydown={handleMenuKey}
    >
      {#each items as entry, i (i)}
        {#if "separator" in entry}
          <hr class="cry-menu__separator" />
        {:else}
          <button
            type="button"
            class="cry-menu__item{entry.danger ? ' cry-menu__item--danger' : ''}"
            class:cry-menu__item--hot={pickable[hot] === entry}
            role="menuitem"
            data-item={pickable.indexOf(entry)}
            disabled={entry.disabled}
            onclick={() => pick(entry.id)}
          >
            {entry.label}
            {#if entry.hint}
              <span class="cry-menu__hint">{entry.hint}</span>
            {/if}
          </button>
        {/if}
      {/each}
    </div>
  {/if}
</div>
