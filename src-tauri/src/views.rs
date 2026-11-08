//! Types de retour des commandes BACK-TOOLS (miroirs `contracts.ts`).
//!
//! Chaque struct sérialise **à la lettre** du contrat (`serde` +
//! `rename_all` là où le TS est en camelCase). Zéro duplication avec
//! la tranche core : seuls les types des §4–§6, §7 partiel, §8–§10.

use serde::Serialize;

// ---------------------------------------------------------------- outils
/// Outil listé (`tools_list`) : schéma + classe d'effet.
#[derive(Debug, Clone, Serialize)]
pub struct ToolView {
    pub name: String,
    pub description: String,
    pub class: &'static str,
}

/// Règle affichée (`permissions_get`).
#[derive(Debug, Clone, Serialize)]
pub struct PermRuleView {
    pub pattern: String,
    pub rule: String,
}

/// État permissions (`permissions_get`, `*_set_*`, `reset`).
#[derive(Debug, Clone, Serialize)]
pub struct PermissionsView {
    pub mode: String,
    pub rules: Vec<PermRuleView>,
}

/// Appel d'outil pour le frontend (`ToolCallView`, camelCase contrat).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallView {
    pub name: String,
    pub args_json: String,
}

/// Demande de confirmation (`tool_confirm`).
#[derive(Debug, Clone, Serialize)]
pub struct ToolConfirmView {
    pub chat: usize,
    pub call: ToolCallView,
}

/// Livraison d'exécution (`tool_delivery`).
#[derive(Debug, Clone, Serialize)]
pub struct ToolDeliveryView {
    pub call: ToolCallView,
    pub ok: bool,
    pub text: String,
    pub ms: u64,
}

// ---------------------------------------------------------------- web
/// Source web (`sources[n]`, 1-based à l'affichage comme le TUI).
#[derive(Debug, Clone, Serialize)]
pub struct SearchResultView {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Réponse `/web` (`web_search`).
#[derive(Debug, Clone, Serialize)]
pub struct WebAnswerView {
    pub backend: String,
    pub text: String,
    pub sources: Vec<SearchResultView>,
}

// ---------------------------------------------------------------- kiro
/// Statut `.kiro/` (`kiro_status`, camelCase contrat).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroStatusView {
    pub project_dir: String,
    pub global_dir: Option<String>,
    pub steering: usize,
    pub specs: usize,
    pub hooks: usize,
    pub mcp_servers: usize,
    pub skills: usize,
    pub text: String,
}

/// Skill listé (`skills_list`).
#[derive(Debug, Clone, Serialize)]
pub struct SkillView {
    pub name: String,
    pub description: String,
}

/// Snapshot listé (`checkpoints_list`).
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotView {
    pub name: String,
    pub created: String,
}

/// Tâche de spec (`spec_show`).
#[derive(Debug, Clone, Serialize)]
pub struct SpecTaskView {
    pub label: String,
    pub done: bool,
}

/// Spec lue (`spec_show`).
#[derive(Debug, Clone, Serialize)]
pub struct SpecView {
    pub name: String,
    pub tasks: Vec<SpecTaskView>,
    pub done: usize,
    pub total: usize,
}

/// ACK de lancement (`spec_run`).
#[derive(Debug, Clone, Serialize)]
pub struct SpecRunAck {
    pub started: bool,
    pub label: String,
}

/// Serveur MCP (`mcp_status`, `mcp_refresh`).
#[derive(Debug, Clone, Serialize)]
pub struct McpServerView {
    pub name: String,
    pub enabled: bool,
    pub tools: usize,
}

/// Statut MCP (`mcp_status`, `mcp_refresh`).
#[derive(Debug, Clone, Serialize)]
pub struct McpStatusView {
    pub servers: Vec<McpServerView>,
}

/// Fichier de hooks (`hooks_list`).
#[derive(Debug, Clone, Serialize)]
pub struct NamedHooksView {
    pub file: String,
    pub triggers: Vec<String>,
    pub count: usize,
}

/// État hooks (`hooks_list`).
#[derive(Debug, Clone, Serialize)]
pub struct HooksView {
    pub hooks: Vec<NamedHooksView>,
    pub recent: Vec<String>,
}

// ---------------------------------------------------------------- apparition
/// Créature (`apparition_state`, `bestiary_*`).
#[derive(Debug, Clone, Serialize)]
pub struct CreatureView {
    pub slug: String,
    pub name: String,
    pub lore: String,
    pub grid: Vec<String>,
}

/// Créature du panneau (`apparition_state`, `spook`).
#[derive(Debug, Clone, Serialize)]
pub struct ApparitionView {
    pub creature: CreatureView,
    pub omen: Option<String>,
}

/// Entrée de catalogue (`bestiary_list`, sans grilles).
#[derive(Debug, Clone, Serialize)]
pub struct CreatureLite {
    pub slug: String,
    pub name: String,
    pub lore: String,
}

// ---------------------------------------------------------------- boutique
/// Article (`shop_state`, `calm` renseigné si effet `calm`).
#[derive(Debug, Clone, Serialize)]
pub struct ShopItemView {
    pub name: String,
    pub lore: String,
    pub price: u64,
    pub effect: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calm: Option<u8>,
}

/// État boutique (`shop_state`, event `shop`).
#[derive(Debug, Clone, Serialize)]
pub struct ShopStateView {
    pub items: Vec<ShopItemView>,
    pub earned: u64,
    pub spent: u64,
    pub balance: u64,
    pub message: Option<String>,
}

/// Résultat d'achat (`shop_buy`).
#[derive(Debug, Clone, Serialize)]
pub struct ShopBuyResultView {
    pub ok: bool,
    pub message: String,
    pub state: ShopStateView,
}

// ---------------------------------------------------------------- hub
/// Esprit (`hub_snapshot`, clé `chat:<n>` 1-based comme la sidebar).
#[derive(Debug, Clone, Serialize)]
pub struct SpiritView {
    pub key: String,
    pub x: f32,
    pub y: f32,
    pub mood: String,
    pub bubble: Option<String>,
}

/// Orbe-message (`hub_snapshot`, positions dalles + progression).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrbView {
    pub from_x: f32,
    pub from_y: f32,
    pub to_x: f32,
    pub to_y: f32,
    pub progress: f32,
}

/// Snapshot du hub (`hub_snapshot`, rendu iso côté frontend).
#[derive(Debug, Clone, Serialize)]
pub struct HubSnapshotView {
    pub spirits: Vec<SpiritView>,
    pub orbs: Vec<OrbView>,
}

// ---------------------------------------------------------------- haunt
/// Point float (pixels monde, canevas 960×576).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct DotView {
    pub x: f32,
    pub y: f32,
}

/// Ennemi (`foes[]`, boss inclus comme `reaper`).
#[derive(Debug, Clone, Serialize)]
pub struct FoeView {
    pub x: f32,
    pub y: f32,
    pub kind: String,
}

/// Projectile (`shots[]`, `foe` = tir ennemi).
#[derive(Debug, Clone, Serialize)]
pub struct ShotView {
    pub x: f32,
    pub y: f32,
    pub foe: bool,
}

/// HUD (`hud`, `fear` = bougie consumée en %).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct HauntHudView {
    pub level: u8,
    pub score: u32,
    pub lives: u8,
    pub fear: u8,
}

/// Frame de jeu (`haunt_start`, event `haunt_frame`).
#[derive(Debug, Clone, Serialize)]
pub struct HauntFrameView {
    pub running: bool,
    pub player: DotView,
    pub foes: Vec<FoeView>,
    pub shots: Vec<ShotView>,
    pub hud: HauntHudView,
}

// ---------------------------------------------------------------- vps
/// Profil VPS (`vps_list`, `vps_add`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VpsProfileView {
    pub name: String,
    pub host: String,
    pub user: String,
    pub port: u16,
    pub key: Option<String>,
    pub dir: String,
    pub dst: String,
}

/// Profil reçu (`vps_add`, camelCase accepté comme le contrat).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpsProfileInput {
    pub name: String,
    pub host: String,
    pub user: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    pub key: Option<String>,
    #[serde(default = "default_vps_dir")]
    pub dir: String,
}

/// Port SSH par défaut (désérialisation).
fn default_ssh_port() -> u16 {
    22
}

/// Checkout distant par défaut (désérialisation).
fn default_vps_dir() -> String {
    String::from("~/crypte")
}

/// État VPS (`vps_list`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VpsStateView {
    pub profiles: Vec<VpsProfileView>,
    pub current: Option<String>,
    pub status_by_name: std::collections::HashMap<String, String>,
}

/// ACK d'opération (`vps_launch|stop|status|install`).
#[derive(Debug, Clone, Serialize)]
pub struct VpsAck {
    pub accepted: bool,
    pub detail: String,
}

// ---------------------------------------------------------------- toggles
/// Interrupteur (`tools_status`, `tools_set`, `critic_set`).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct EnabledView {
    pub enabled: bool,
}

/// Texte (`critic_smack`).
#[derive(Debug, Clone, Serialize)]
pub struct TextView {
    pub text: String,
}

// ---------------------------------------------------------------- events UI
/// Event `crypte-ui` (canal éphémère, tag `kind` snake_case).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiEvent {
    ToolConfirm {
        request: ToolConfirmView,
    },
    ToolDelivery {
        chat: usize,
        delivery: ToolDeliveryView,
    },
    SpecProgress {
        name: String,
        label: String,
        done: usize,
        total: usize,
    },
    HubTick {
        snapshot: HubSnapshotView,
    },
    Apparition {
        state: ApparitionView,
    },
    HauntFrame {
        frame: HauntFrameView,
    },
    VpsOutcome {
        target: String,
        op: String,
        ok: bool,
        text: String,
    },
    Shop {
        state: ShopStateView,
    },
    McpRefreshed {
        status: McpStatusView,
    },
    Critic {
        text: String,
    },
}

/// Nom du canal UI (contrat : `crypte-ui`).
pub const UI_CHANNEL: &str = "crypte-ui";

impl From<&crate::tools::wire::ToolCall> for ToolCallView {
    fn from(call: &crate::tools::wire::ToolCall) -> Self {
        Self {
            name: call.name.clone(),
            args_json: call.args.clone(),
        }
    }
}

impl From<&crate::tools::flow::Delivery> for ToolDeliveryView {
    fn from(delivery: &crate::tools::flow::Delivery) -> Self {
        Self {
            call: ToolCallView::from(&delivery.call),
            ok: delivery.ok,
            text: delivery.text.clone(),
            ms: delivery.ms,
        }
    }
}

/// Demande de confirmation pour l'event `tool_confirm`.
pub fn confirm_view(chat: usize, call: &crate::tools::wire::ToolCall) -> ToolConfirmView {
    ToolConfirmView {
        chat,
        call: ToolCallView::from(call),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_case_des_vues_contrat() {
        let call = ToolCallView {
            name: String::from("shell_run"),
            args_json: String::from("{}"),
        };
        let raw = serde_json::to_value(&call).unwrap();
        assert_eq!(raw["argsJson"], "{}");
        let orb = OrbView {
            from_x: 1.0,
            from_y: 2.0,
            to_x: 3.0,
            to_y: 4.0,
            progress: 0.5,
        };
        let raw = serde_json::to_value(&orb).unwrap();
        assert_eq!(raw["fromX"], 1.0);
        assert_eq!(raw["progress"], 0.5);
        let state = VpsStateView {
            profiles: Vec::new(),
            current: None,
            status_by_name: Default::default(),
        };
        let raw = serde_json::to_value(&state).unwrap();
        assert!(raw.get("statusByName").is_some());
    }

    #[test]
    fn events_ui_tagges_kind_snake_case() {
        let ev = UiEvent::Critic {
            text: String::from("Vague."),
        };
        let raw = serde_json::to_value(&ev).unwrap();
        assert_eq!(raw["kind"], "critic");
        let ev = UiEvent::ToolDelivery {
            chat: 1,
            delivery: ToolDeliveryView {
                call: ToolCallView {
                    name: String::from("shell_run"),
                    args_json: String::from("{}"),
                },
                ok: true,
                text: String::from("hi"),
                ms: 3,
            },
        };
        let raw = serde_json::to_value(&ev).unwrap();
        assert_eq!(raw["kind"], "tool_delivery");
        assert_eq!(raw["chat"], 1);
    }

    #[test]
    fn vps_input_accepte_camel_et_defauts() {
        let input: VpsProfileInput = serde_json::from_value(serde_json::json!({
            "name": "r630", "host": "h", "user": "u", "key": null
        }))
        .unwrap();
        assert_eq!(input.port, 22);
        assert_eq!(input.dir, "~/crypte");
        assert_eq!(input.key, None);
    }
}
