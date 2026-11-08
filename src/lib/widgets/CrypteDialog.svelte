<!-- CrypteDialog — dialogue modal unique (Échap, focus piégé, retour focus).
     Styles : crypte.css .cry-dialog*. -->
<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    open?: boolean;
    title: string;
    wide?: boolean;
    /** Faux = pas de fermeture Échap/clic fond (choix forcé). */
    dismissable?: boolean;
    onclose?: () => void;
    children: Snippet;
    footer?: Snippet;
  }

  let {
    open = $bindable(false),
    title,
    wide = false,
    dismissable = true,
    onclose,
    children,
    footer,
  }: Props = $props();

  let dlg: HTMLDivElement | undefined = $state(undefined);
  let previousFocus: HTMLElement | null = null;

  function close(): void {
    open = false;
    onclose?.();
  }

  // Focus à l'ouverture, restauration à la fermeture.
  $effect(() => {
    if (open) {
      previousFocus = document.activeElement as HTMLElement | null;
      dlg?.focus();
    } else {
      previousFocus?.focus?.();
      previousFocus = null;
    }
  });

  function focusables(): HTMLElement[] {
    if (!dlg) return [];
    const els = dlg.querySelectorAll<HTMLElement>(
      'button, [href], input, textarea, select, [tabindex]:not([tabindex="-1"])',
    );
    return [...els].filter((el) => !el.hasAttribute("disabled"));
  }

  function handleKey(e: KeyboardEvent): void {
    if (!open) return;
    if (e.key === "Escape" && dismissable) {
      e.preventDefault();
      close();
      return;
    }
    // Piège Tab : boucle sur les éléments focalisables du dialogue.
    if (e.key === "Tab") {
      const els = focusables();
      if (els.length === 0) {
        e.preventDefault();
        return;
      }
      const first = els[0];
      const last = els[els.length - 1];
      const at = document.activeElement;
      if (e.shiftKey && (at === first || at === dlg)) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && at === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }
</script>

<svelte:window onkeydown={handleKey} />

{#if open}
  <div
    class="cry-dialog-backdrop"
    onclick={() => dismissable && close()}
    role="presentation"
  >
    <div
      bind:this={dlg}
      class="cry-dialog{wide ? ' cry-dialog--wide' : ''}"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
    >
      <h2 class="cry-dialog__title">{title}</h2>
      <div class="cry-dialog__body">
        {@render children()}
      </div>
      {#if footer}
        <div class="cry-dialog__footer">
          {@render footer()}
        </div>
      {/if}
    </div>
  </div>
{/if}
