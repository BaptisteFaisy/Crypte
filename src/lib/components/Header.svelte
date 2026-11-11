<!-- Header — en-tête : titre dégradé, onglets cliquables, jauge de peur
  à droite (miroir de draw_header, app/ui.rs). Sous ~80 colonnes, les
  touches d'onglets disparaissent mais la jauge reste entière. -->
<script lang="ts">
  import { page } from "$app/state";
  import { TABS, shell } from "../crypte.svelte";
  import FearGauge from "./FearGauge.svelte";

  let current = $derived(page.url.pathname);
</script>

<header class="crypte-header" data-testid="app-header">
  <span class="brand" aria-label="Crypt">
    <span class="crypte-title" aria-hidden="true">◈ CRYPT</span>
  </span>
  <nav class="tabs" aria-label="Écrans">
    {#each TABS as tab (tab.id)}
      {@const active =
        tab.route === "/" ? current === "/" : current.startsWith(tab.route)}
      <a
        href={tab.route}
        class="tab"
        class:active
        aria-current={active ? "page" : undefined}
        data-testid={`tab-${tab.id}`}
      >
        <span class="tab-label">{tab.label}</span>{#if tab.key}<span
            class="tab-key">{tab.key}</span
          >{/if}
      </a>
    {/each}
  </nav>
  <div class="gauge">
    <FearGauge used={shell.fearUsed} window={shell.fearWindow} />
  </div>
</header>

<style>
  .crypte-header {
    height: var(--cry-header-h);
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0 1rem;
    border-bottom: 1px solid var(--cry-umber);
    background: var(--cry-void);
    flex: none;
  }
  .brand {
    font-weight: 700;
    letter-spacing: 0.12em;
    white-space: nowrap;
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .tabs::-webkit-scrollbar {
    display: none;
  }
  .tab {
    display: inline-flex;
    align-items: baseline;
    gap: 0.5ch;
    padding: 0.15rem 0.6rem;
    border-radius: 999px;
    color: var(--cry-sand);
    text-decoration: none;
    white-space: nowrap;
  }
  .tab:hover {
    color: var(--cry-bone);
  }
  .tab.active {
    background: var(--cry-pumpkin);
    color: var(--cry-void);
    font-weight: 700;
  }
  .tab-key {
    color: var(--cry-umber);
    font-size: 0.8em;
  }
  .tab.active .tab-key {
    color: var(--cry-void);
    opacity: 0.7;
  }
  .gauge {
    margin-left: auto;
    flex: none;
  }
  @media (max-width: 45rem) {
    .tab-key {
      display: none;
    }
    .crypte-header {
      gap: 0.6rem;
      padding: 0 0.6rem;
    }
  }
</style>
