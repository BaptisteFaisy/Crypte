<!-- CrypteList — liste générique unique (rôle listbox, clavier ↑↓⤶).
     Le contenu de chaque ligne vient du snippet `row` (item, index). -->
<script lang="ts" generics="T">
  import type { Snippet } from "svelte";

  interface Props {
    items: T[];
    getKey: (item: T, index: number) => string;
    selected?: string | null;
    onselect?: (item: T, index: number) => void;
    emptyText?: string;
    ariaLabel?: string;
    disabled?: boolean;
    row: Snippet<[T, number]>;
  }

  let {
    items,
    getKey,
    selected = $bindable(null),
    onselect,
    emptyText = "Rien à afficher.",
    ariaLabel = "Liste",
    disabled = false,
    row,
  }: Props = $props();

  let box: HTMLDivElement | undefined = $state(undefined);
  let active = $state(0);

  // Garde l'index actif sur la sélection quand elle change de l'extérieur.
  $effect(() => {
    const at = items.findIndex((it, i) => getKey(it, i) === selected);
    if (at >= 0) active = at;
  });

  function focusRow(i: number): void {
    box?.querySelector<HTMLElement>(`[data-row="${i}"]`)?.focus();
  }

  function pick(i: number): void {
    if (disabled || i < 0 || i >= items.length) return;
    active = i;
    selected = getKey(items[i], i);
    onselect?.(items[i], i);
  }

  function handleKey(e: KeyboardEvent): void {
    if (disabled || items.length === 0) return;
    let next: number | null = null;
    if (e.key === "ArrowDown") next = Math.min(active + 1, items.length - 1);
    else if (e.key === "ArrowUp") next = Math.max(active - 1, 0);
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = items.length - 1;
    else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      pick(active);
      return;
    }
    if (next !== null) {
      e.preventDefault();
      active = next;
      focusRow(next);
    }
  }
</script>

<div
  bind:this={box}
  class="cry-list{disabled ? ' cry-list--disabled' : ''}"
  role="listbox"
  tabindex="-1"
  aria-label={ariaLabel}
  aria-disabled={disabled || undefined}
  onkeydown={handleKey}
>
  {#if items.length === 0}
    <p class="cry-list__empty">{emptyText}</p>
  {:else}
    {#each items as item, i (getKey(item, i))}
      <div
        class="cry-list__row"
        role="option"
        data-row={i}
        tabindex={i === active ? 0 : -1}
        aria-selected={getKey(item, i) === selected}
        onclick={() => pick(i)}
        onkeydown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            pick(i);
          }
        }}
      >
        {@render row(item, i)}
      </div>
    {/each}
  {/if}
</div>
