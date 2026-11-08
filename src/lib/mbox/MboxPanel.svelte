<!-- Panneau inter-agents : mailbox broadcast v2 (commandes PLAN §11).
     Onglets : Messages (inbox) / Bus (phone gpu file divers) / Ordres (+ accusés)
     / Agents (hb, métriques, piste) / Ops (doctor, jetons, vague, fantôme). -->
<script lang="ts">
  import MboxAgents from "./MboxAgents.svelte";
  import MboxBus from "./MboxBus.svelte";
  import MboxMessages from "./MboxMessages.svelte";
  import MboxOps from "./MboxOps.svelte";
  import MboxOrdres from "./MboxOrdres.svelte";

  type Tab = "messages" | "bus" | "ordres" | "agents" | "ops";
  let tab = $state<Tab>("messages");

  const TABS: { id: Tab; label: string }[] = [
    { id: "messages", label: "Messages" },
    { id: "bus", label: "Bus" },
    { id: "ordres", label: "Ordres" },
    { id: "agents", label: "Agents" },
    { id: "ops", label: "Ops" },
  ];
</script>

<h1>Inter-agents</h1>
<nav class="row" aria-label="Panneau inter-agents">
  {#each TABS as t (t.id)}
    <button
      class={tab === t.id ? "primary" : ""}
      onclick={() => { tab = t.id; }}
      aria-current={tab === t.id ? "page" : undefined}
    >
      {t.label}
    </button>
  {/each}
</nav>

<div style="margin-top: 0.8rem;">
  {#if tab === "messages"}
    <MboxMessages />
  {:else if tab === "bus"}
    <MboxBus />
  {:else if tab === "ordres"}
    <MboxOrdres />
  {:else if tab === "agents"}
    <MboxAgents />
  {:else}
    <MboxOps />
  {/if}
</div>
