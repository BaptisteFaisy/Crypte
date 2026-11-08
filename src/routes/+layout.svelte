<!-- +layout.svelte — coquille Crypte : en-tête (onglets + peur),
  écran courant, barre powerline, overlays, toasts.
  Touches globales du TUI (app/keys.rs) : F5 scare, F6 shop, F9 haunt. -->
<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import "../lib/crypte.css";
  import "../lib/crypte-shell.css";
  import "../lib/crypte-chat.css";
  import "../lib/crypte-screens.css";
  import { shell } from "../lib/crypte.svelte";
  import ConnectModal from "../lib/components/ConnectModal.svelte";
  import Header from "../lib/components/Header.svelte";
  import HelpPanel from "../lib/components/HelpPanel.svelte";
  import JumpScare from "../lib/components/JumpScare.svelte";
  import QuestionModal from "../lib/components/QuestionModal.svelte";
  import StatusBar from "../lib/components/StatusBar.svelte";
  import Toasts from "../lib/components/Toasts.svelte";

  let { children } = $props();

  $effect(() => {
    shell.game = page.url.pathname === "/haunt";
  });

  $effect(() => {
    let stop: (() => void) | null = null;
    void shell.startShellListeners().then((off) => {
      stop = off;
    });
    return () => stop?.();
  });

  function inField(target: EventTarget | null): boolean {
    return (
      target instanceof HTMLInputElement ||
      target instanceof HTMLTextAreaElement ||
      target instanceof HTMLSelectElement
    );
  }

  async function onKey(event: KeyboardEvent): Promise<void> {
    if (event.key === "F5") {
      event.preventDefault();
      await shell.spook();
    } else if (event.key === "F6") {
      event.preventDefault();
      await goto("/shop");
    } else if (event.key === "F9") {
      event.preventDefault();
      const here = page.url.pathname;
      await goto(here === "/haunt" ? "/" : "/haunt");
    } else if (
      (event.key === "q" || event.key === "Q") &&
      !inField(event.target) &&
      !event.ctrlKey &&
      !event.metaKey &&
      !event.altKey
    ) {
      // `q` quitte dans le TUI ; ici : retour chat, sans voler la saisie.
      if (page.url.pathname !== "/") await goto("/");
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="crypte-app" data-testid="crypte-app">
  <Header />
  <main class="crypte-main" data-testid="crypte-main">
    {@render children()}
  </main>
  <StatusBar />
</div>

{#if shell.question}
  <QuestionModal question={shell.question} />
{/if}
{#if shell.connectOpen}
  <ConnectModal />
{/if}
{#if shell.helpOpen}
  <HelpPanel />
{/if}
{#if shell.jumpscare}
  <JumpScare apparition={shell.jumpscare} />
{/if}
<Toasts />
