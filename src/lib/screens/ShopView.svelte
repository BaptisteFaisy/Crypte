<!-- Écran Boutique : 5 articles d'ambiance (commandes PLAN §7).
     1 token consommé = 1 pièce ; solde jamais négatif (cf. shop.rs). -->
<script lang="ts">
  import type {
    CrypteUiEvent,
    ShopBuyResult,
    ShopState,
  } from "../contracts";
  import { onDestroy, onMount } from "svelte";
  import { errText, invokeCmd, listenCrypte } from "../tauri";

  let shop = $state<ShopState | null>(null);
  let error = $state<string | null>(null);
  let buying = $state<string | null>(null);
  let unlisten: (() => void) | null = null;

  async function refresh(): Promise<void> {
    error = null;
    try {
      shop = await invokeCmd<ShopState>("shop_state");
    } catch (e) {
      error = errText(e);
    }
  }

  onMount(() => {
    void refresh();
    void listenCrypte("crypte-ui", (ev) => {
      if (ev.kind !== "shop") return;
      shop = ev.state;
    }).then((u) => {
      unlisten = u;
    });
  });

  onDestroy(() => unlisten?.());

  async function buy(item: string): Promise<void> {
    buying = item;
    error = null;
    try {
      const res = await invokeCmd<ShopBuyResult>("shop_buy", { item });
      shop = res.state;
      if (!res.ok) error = res.message;
    } catch (e) {
      error = errText(e);
    } finally {
      buying = null;
    }
  }
</script>

<h1>Boutique de la crypte</h1>
{#if error}<p class="err">{error}</p>{/if}

{#if shop}
  <div class="panel">
    <div class="row">
      <span>Solde : <strong class="ok">{shop.balance}</strong> pièces</span>
      <span class="dim">gagnées {shop.earned} — dépensées {shop.spent}</span>
    </div>
    {#if shop.message}<p>{shop.message}</p>{/if}
  </div>

  <div class="panel">
    <h2>Catalogue ({shop.items.length} articles)</h2>
    <table class="crypt">
      <thead>
        <tr><th>Article</th><th>Description</th><th>Prix</th><th></th></tr>
      </thead>
      <tbody>
        {#each shop.items as item (item.name)}
          <tr>
            <td><strong>{item.name}</strong></td>
            <td class="dim">{item.lore}</td>
            <td class="mono">{item.price}</td>
            <td>
              <button
                class="primary"
                onclick={() => void buy(item.name)}
                disabled={buying !== null || shop.balance < item.price}
              >
                {buying === item.name ? "…" : "Acheter"}
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{:else}
  <p class="dim">Chargement de la boutique…</p>
{/if}
