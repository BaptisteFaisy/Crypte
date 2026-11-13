//! Registre des 95 commandes Tauri, en 4 groupes (<50 lignes/fn).
//!
//! `tauri::generate_handler!` ne se compose pas (un seul handler par
//! builder) : [`dispatch`] route sur le préfixe du nom vers le groupe
//! qui déclare la commande. Routage testé nom par nom ci-dessous
//! (tout nom inconnu tombe dans le groupe outils et rend faux).

use tauri::ipc::Invoke;

/// Route un appel vers son groupe (faux = commande inconnue).
pub fn dispatch(invoke: Invoke<tauri::Wry>) -> bool {
    match group_of(invoke.message.command()) {
        Group::Chat => handler_chat()(invoke),
        Group::BusProv => handler_bus_prov()(invoke),
        Group::Mbox => handler_mbox()(invoke),
        Group::Tools => handler_tools()(invoke),
    }
}

/// Groupe d'une commande (préfixes disjoints ; inconnu → outils).
fn group_of(cmd: &str) -> Group {
    if cmd.starts_with("mbox_") {
        return Group::Mbox;
    }
    if cmd.starts_with("chat_") || cmd == "usage_get" || cmd == "ctx_gauge" {
        return Group::Chat;
    }
    for prefix in [
        "agent_",
        "agents_",
        "tell_",
        "msg_",
        "link",
        "connect_",
        "provider",
        "bestiary_",
    ] {
        if cmd.starts_with(prefix) {
            return Group::BusProv;
        }
    }
    Group::Tools
}

/// Groupe du registre (un `generate_handler!` chacun).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Chat,
    BusProv,
    Tools,
    Mbox,
}

/// §1 : 13 commandes chat.
fn handler_chat() -> impl Fn(Invoke<tauri::Wry>) -> bool {
    tauri::generate_handler![
        crate::tauri_chat::chat_list,
        crate::tauri_chat::chat_new,
        crate::tauri_chat::chat_select,
        crate::tauri_chat::chat_close,
        crate::tauri_chat::chat_clear,
        crate::tauri_chat::chat_send,
        crate::tauri_chat::chat_stop,
        crate::tauri_chat::chat_state,
        crate::tauri_chat::chat_compact,
        crate::tauri_chat::chat_set_model,
        crate::tauri_chat::chat_set_thinking,
        crate::tauri_chat::usage_get,
        crate::tauri_chat::ctx_gauge,
    ]
}

/// §2–§3 + §7 partiel : bus, providers, bestiaire (19).
fn handler_bus_prov() -> impl Fn(Invoke<tauri::Wry>) -> bool {
    tauri::generate_handler![
        crate::tauri_more::agent_spawn,
        crate::tauri_more::agent_ask,
        crate::tauri_more::agents_list,
        crate::tauri_more::tell_send,
        crate::tauri_more::msg_send,
        crate::tauri_more::link_add,
        crate::tauri_more::link_remove,
        crate::tauri_more::links_list,
        crate::tauri_more::connect_status,
        crate::tauri_more::connect_save,
        crate::tauri_more::connect_muse,
        crate::tauri_more::providers_list,
        crate::tauri_more::provider_add,
        crate::tauri_more::provider_set_key,
        crate::tauri_more::provider_remove,
        crate::tauri_more::provider_select,
        crate::tauri_more::bestiary_get,
        crate::tauri_more::bestiary_random,
        crate::tauri_more::bestiary_list,
    ]
}

/// §4–§10 : web, outils, kiro, specs, mcp, critique, boutique,
/// apparition, hub, jeu, vps (39) + project space (2).
fn handler_tools() -> impl Fn(Invoke<tauri::Wry>) -> bool {
    tauri::generate_handler![
        crate::space::cmds::space_scan,
        crate::space::cmds::space_projects,
        crate::space::cmds::space_attach,
        crate::space::cmds::space_detach,
        crate::cmd_tools::tools_status,
        crate::cmd_tools::tools_set,
        crate::cmd_tools::tools_list,
        crate::cmd_tools::tool_decide,
        crate::cmd_tools::permissions_get,
        crate::cmd_tools::permissions_set_mode,
        crate::cmd_tools::permissions_set_rule,
        crate::cmd_tools::permissions_reset,
        crate::cmd_web::web_search,
        crate::cmd_kiro::kiro_status,
        crate::cmd_kiro::skills_list,
        crate::cmd_kiro::skill_inject,
        crate::cmd_kiro::memory_remember,
        crate::cmd_kiro::checkpoints_list,
        crate::cmd_kiro::checkpoint_rewind,
        crate::cmd_kiro::hooks_list,
        crate::cmd_spec::spec_new,
        crate::cmd_spec::spec_show,
        crate::cmd_spec::spec_run,
        crate::cmd_spec::spec_respond,
        crate::cmd_mcp::mcp_status,
        crate::cmd_mcp::mcp_refresh,
        crate::cmd_critic::critic_set,
        crate::cmd_critic::critic_smack,
        crate::cmd_shop::shop_state,
        crate::cmd_shop::shop_buy,
        crate::cmd_apparition::apparition_state,
        crate::cmd_apparition::spook,
        crate::cmd_hub::hub_snapshot,
        crate::cmd_haunt::haunt_start,
        crate::cmd_haunt::haunt_stop,
        crate::cmd_haunt::haunt_input,
        crate::cmd_vps::vps_list,
        crate::cmd_vps::vps_launch,
        crate::cmd_vps::vps_stop,
        crate::cmd_vps::vps_status,
        crate::cmd_vps::vps_add,
        crate::cmd_vps::vps_remove,
        crate::cmd_vps::vps_install,
    ]
}

/// §11 : mailbox broadcast v2 (22).
fn handler_mbox() -> impl Fn(Invoke<tauri::Wry>) -> bool {
    tauri::generate_handler![
        crate::cmd_mbox_core::mbox_tail,
        crate::cmd_mbox_core::mbox_send,
        crate::cmd_mbox_core::mbox_bus,
        crate::cmd_mbox_core::mbox_digest,
        crate::cmd_mbox_core::mbox_rotate,
        crate::cmd_mbox_core::mbox_hb,
        crate::cmd_mbox_core::mbox_hb_list,
        crate::cmd_mbox_ordres::mbox_ordre_new,
        crate::cmd_mbox_ordres::mbox_ordre_list,
        crate::cmd_mbox_ordres::mbox_ordre_revoke,
        crate::cmd_mbox_ordres::mbox_ordre_ack,
        crate::cmd_mbox_ordres::mbox_ordre_pull,
        crate::cmd_mbox_ordres::mbox_ordre_delegate,
        crate::cmd_mbox_ordres::mbox_jeton_new,
        crate::cmd_mbox_ordres::mbox_jeton_revoke,
        crate::cmd_mbox_ordres::mbox_jeton_list,
        crate::cmd_mbox_ordres::mbox_jeton_purge,
        crate::cmd_mbox_ops::mbox_doctor,
        crate::cmd_mbox_ops::mbox_metriques,
        crate::cmd_mbox_ops::mbox_piste,
        crate::cmd_mbox_ops::mbox_vague,
        crate::cmd_mbox_ops::mbox_fantome,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cmd` tombe dans `want` (panique sinon, nom par nom).
    fn check(cmd: &str, want: Group) {
        assert_eq!(group_of(cmd), want, "{cmd}");
    }

    #[test]
    fn routage_chat_et_busprov() {
        for cmd in [
            "chat_list",
            "chat_new",
            "chat_select",
            "chat_close",
            "chat_clear",
            "chat_send",
            "chat_stop",
            "chat_state",
            "chat_compact",
            "chat_set_model",
            "chat_set_thinking",
            "usage_get",
            "ctx_gauge",
        ] {
            check(cmd, Group::Chat);
        }
        for cmd in [
            "agent_spawn",
            "agent_ask",
            "agents_list",
            "tell_send",
            "msg_send",
            "link_add",
            "link_remove",
            "links_list",
            "connect_status",
            "connect_save",
            "connect_muse",
            "providers_list",
            "provider_add",
            "provider_set_key",
            "provider_remove",
            "provider_select",
            "bestiary_get",
            "bestiary_random",
            "bestiary_list",
        ] {
            check(cmd, Group::BusProv);
        }
    }

    #[test]
    fn routage_outils() {
        for cmd in [
            "tools_status",
            "tools_set",
            "tools_list",
            "tool_decide",
            "permissions_get",
            "permissions_set_mode",
            "permissions_set_rule",
            "permissions_reset",
            "web_search",
            "kiro_status",
            "skills_list",
            "skill_inject",
            "memory_remember",
            "checkpoints_list",
            "checkpoint_rewind",
            "hooks_list",
            "spec_new",
            "spec_show",
            "spec_run",
            "spec_respond",
            "mcp_status",
            "mcp_refresh",
            "critic_set",
            "critic_smack",
            "shop_state",
            "shop_buy",
            "apparition_state",
            "spook",
            "hub_snapshot",
            "haunt_start",
            "haunt_stop",
            "haunt_input",
            "vps_list",
            "vps_launch",
            "vps_stop",
            "vps_status",
            "vps_add",
            "vps_remove",
            "vps_install",
            "space_scan",
            "space_projects",
            "space_attach",
            "space_detach",
        ] {
            check(cmd, Group::Tools);
        }
    }

    #[test]
    fn routage_mbox() {
        for cmd in [
            "mbox_tail",
            "mbox_send",
            "mbox_bus",
            "mbox_digest",
            "mbox_rotate",
            "mbox_hb",
            "mbox_hb_list",
            "mbox_ordre_new",
            "mbox_ordre_list",
            "mbox_ordre_revoke",
            "mbox_ordre_ack",
            "mbox_ordre_pull",
            "mbox_ordre_delegate",
            "mbox_jeton_new",
            "mbox_jeton_revoke",
            "mbox_jeton_list",
            "mbox_jeton_purge",
            "mbox_doctor",
            "mbox_metriques",
            "mbox_piste",
            "mbox_vague",
            "mbox_fantome",
        ] {
            check(cmd, Group::Mbox);
        }
    }
}
