<!-- Toasts — notifications du shell (erreurs backend, P0 mailbox…).
  Les erreurs sont des alertes (role=alert), jamais de secret dedans. -->
<script lang="ts">
  import { shell } from "../crypte.svelte";
</script>

<div class="toasts" aria-live="polite" data-testid="toasts">
  {#each shell.toasts as toast (toast.id)}
    <div
      class="toast {toast.kind}"
      role={toast.kind === "error" ? "alert" : "status"}
    >
      <span class="toast-text">{toast.text}</span>
      <button
        class="toast-close"
        aria-label="Fermer"
        onclick={() => shell.dismissToast(toast.id)}>×</button
      >
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 1rem;
    bottom: calc(var(--cry-footer-h) + 0.75rem);
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    max-width: min(28rem, calc(100vw - 2rem));
    z-index: 60;
  }
  .toast {
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
    padding: 0.55rem 0.75rem;
    border-radius: 0.375rem;
    border: 1px solid var(--cry-umber);
    background: var(--cry-ink);
    color: var(--cry-bone);
    box-shadow: 0 0.5rem 1.5rem rgba(0, 0, 0, 0.5);
  }
  .toast.error {
    border-color: var(--cry-blood);
    background: var(--cry-dried);
  }
  .toast.success {
    border-color: var(--cry-ecto-dark);
  }
  .toast-text {
    flex: 1;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
  .toast-close {
    background: transparent;
    border: none;
    color: var(--cry-sand);
    font-size: 1.1rem;
    line-height: 1;
    padding: 0 0.1rem;
  }
  .toast-close:hover {
    color: var(--cry-bone);
  }
</style>
