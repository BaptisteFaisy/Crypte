//! Commandes §7 (boutique) : `shop_state`, `shop_buy`.
//!
//! Port des commandes d'après `shop.rs` (TUI) : les pièces gagnées
//! valent les tokens consommés de la session (almanach core), le
//! portefeuille vit dans `ToolsBackend`. Chaque achat applique son
//! effet (miroir `apply_shop_effect`) et émet l'event `shop`.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, State};

use crate::core::Core;
use crate::shop::{ShopEffect, SHOP};
use crate::state::{lock, ToolsBackend};
use crate::views::{ShopBuyResultView, ShopItemView, ShopStateView, UiEvent, UI_CHANNEL};

/// `shop_state` : catalogue + portefeuille + message.
#[tauri::command(rename_all = "camelCase")]
pub fn shop_state(
    core: State<'_, Core>,
    backend: State<'_, Mutex<ToolsBackend>>,
) -> Result<ShopStateView, String> {
    let backend = lock(&backend)?;
    Ok(shop_view(&core, &backend))
}

/// `shop_buy` : achète `item` par nom, applique l'effet, émet `shop`.
#[tauri::command(rename_all = "camelCase")]
pub fn shop_buy(
    app: AppHandle,
    core: State<'_, Core>,
    backend: State<'_, Mutex<ToolsBackend>>,
    item: String,
) -> Result<ShopBuyResultView, String> {
    let earned = core.lock().almanac.session().total();
    let mut backend = lock(&backend)?;
    let bought = backend.shop.buy_by_name(earned, &item)?;
    if let Some(effect) = bought {
        apply_effect(&mut backend, effect);
    }
    let state = shop_view(&core, &backend);
    let message = state.message.clone().unwrap_or_default();
    let ok = bought.is_some();
    let _ = app.emit(
        UI_CHANNEL,
        UiEvent::Shop {
            state: state.clone(),
        },
    );
    Ok(ShopBuyResultView { ok, message, state })
}

/// Vue contrat depuis les états (réutilisée par `shop_buy`).
fn shop_view(core: &Core, backend: &ToolsBackend) -> ShopStateView {
    let earned = core.lock().almanac.session().total();
    let spent = backend.shop.wallet().spent();
    ShopStateView {
        items: SHOP.iter().map(item_view).collect(),
        earned,
        spent,
        balance: backend.shop.wallet().balance(earned),
        message: non_empty(backend.shop.message()),
    }
}

/// Article vers la vue (`calm` renseigné si effet `calm`).
fn item_view(item: &crate::shop::ShopItem) -> ShopItemView {
    let (effect, calm) = match item.effect {
        ShopEffect::Calm(n) => (String::from("calm"), Some(n)),
        ShopEffect::Scare => (String::from("scare"), None),
        ShopEffect::Apparition => (String::from("apparition"), None),
        ShopEffect::Flash => (String::from("flash"), None),
    };
    ShopItemView {
        name: item.name.to_string(),
        lore: item.lore.to_string(),
        price: item.price,
        effect,
        calm,
    }
}

/// Applique l'effet d'un achat (peur, apparition, éclair — comme le TUI).
fn apply_effect(backend: &mut ToolsBackend, effect: ShopEffect) {
    match effect {
        ShopEffect::Calm(amount) => backend.fx.calm(amount),
        ShopEffect::Scare => backend.spook(),
        ShopEffect::Apparition => backend.apparition.advance(),
        ShopEffect::Flash => backend.fx.flash(),
    }
}

/// Vide → `None` (contrat : `message: string | null`).
fn non_empty(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_string())
}
