//! Outils des sous-agents : boucle bornée, autonome, sans confirmation.
//!
//! `/agent`, `/ask` et les workflows reçoivent les mêmes outils que les
//! chats (natifs + MCP) quand `/tools on`. Différences : aucune interaction
//! n'est possible, donc les décisions `Ask`/`Deny` deviennent des refus
//! rejoués au modèle ; la boucle est bornée ([`MAX_ROUNDS`] tours,
//! [`MAX_CALLS`] appels) car personne ne surveille ; les hooks `PreToolUse`
//! ne sont pas déclenchés (tâches détachées, sans accès `App`).

use crate::kiro::mcp_client::{is_mcp_name, sanitize};
use crate::kiro::mcp_pool::McpState;
use crate::permissions::{Decision, Mode, Rules};
use crate::provider::{Completion, Provider};
use crate::tools::exec::{self, Outcome};
use crate::tools::flow::tooled_wire;
use crate::tools::wire::{ToolCall, ToolDef, WireMsg};

/// Tours LLM outillés max par réponse d'agent (les chats n'ont pas de
/// limite, l'utilisateur surveille ; un agent seul ne doit pas dépenser
/// sans fin).
pub const MAX_ROUNDS: u32 = 20;
/// Appels d'outils max par réponse d'agent (garde dépense).
pub const MAX_CALLS: usize = 60;

/// Consigne ajoutée après le guide outils (les agents ne posent pas de
/// questions en cours de route : un refus est définitif).
pub const AGENT_NOTE: &str = "Background agent: you cannot ask questions mid-run. A refused tool stays refused — work around it or report it in your reply, don't retry it.";

/// Filtre d'outils d'un agent custom (`tools`/`excludedTools`/`mcpServers`
/// du frontmatter ; vide = tout, voir [`filter_tools`]).
#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub tools: Vec<String>,
    pub excluded: Vec<String>,
    pub mcp_servers: Vec<String>,
}

/// Appel tracé pour le résumé de livraison.
#[derive(Debug, Clone)]
pub struct CallRecord {
    pub name: String,
    pub ok: bool,
    pub refused: bool,
}

/// Contexte d'exécution d'une réponse d'agent (emprunté par la boucle).
#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub provider: &'a Provider,
    pub model: &'a str,
    pub effort: Option<&'a str>,
    pub rules: &'a Rules,
    pub mode: Mode,
    pub mcp: &'a McpState,
    pub chat: usize,
}

/// Réponse d'agent, outillée ou non : schémas natifs + MCP, filtre custom,
/// boucle ([`run_loop`]) ou appel simple si `/tools off` ou filtre vide.
pub async fn agent_reply(
    ctx: Ctx<'_>,
    messages: Vec<(String, String)>,
    filter: &Filter,
    tooled: bool,
) -> anyhow::Result<Completion> {
    if !tooled {
        return ctx
            .provider
            .complete_full(&messages, ctx.model, ctx.effort)
            .await;
    }
    let all = ctx.mcp.tooled_defs().await;
    let tools = filter_tools(&all, filter);
    if tools.is_empty() {
        return ctx
            .provider
            .complete_full(&messages, ctx.model, ctx.effort)
            .await;
    }
    run_loop(ctx, messages, tools).await
}

/// Outils visibles de l'agent : `tools` non vide = allowlist (nom exact,
/// ou nom court MCP après `mcp_<srv>__`), `excluded` retirés,
/// `mcp_servers` non vide = MCP restreintes à ces serveurs (préfixe
/// `mcp_<srv>_`, approximatif si le nom contient des `_`).
pub fn filter_tools(all: &[ToolDef], filter: &Filter) -> Vec<ToolDef> {
    all.iter()
        .filter(|def| {
            let name = def.function.name.as_str();
            if !filter.tools.is_empty() && !filter.tools.iter().any(|want| name_matches(name, want))
            {
                return false;
            }
            if filter.excluded.iter().any(|ban| name_matches(name, ban)) {
                return false;
            }
            if is_mcp_name(name) && !filter.mcp_servers.is_empty() {
                return filter
                    .mcp_servers
                    .iter()
                    .any(|srv| name.starts_with(format!("mcp_{}_", sanitize(srv)).as_str()));
            }
            true
        })
        .cloned()
        .collect()
}

/// Nom exact, ou nom court MCP (`mcp_srv_tool` matche `tool`, suffixe —
/// approximatif si serveur ou outil contient des `_`).
fn name_matches(name: &str, want: &str) -> bool {
    if name == want {
        return true;
    }
    is_mcp_name(name) && name.ends_with(format!("_{want}").as_str())
}

/// Autorisé sans interaction : décision `Allow` (les `Ask`/`Deny` sont
/// refusés, l'agent ne peut pas répondre à une question).
pub fn allowed(rules: &Rules, mode: Mode, name: &str) -> bool {
    matches!(rules.decide(mode, name), Decision::Allow)
}

/// Texte rejoué au modèle pour un appel refusé (jamais réessayé).
pub fn refusal(name: &str) -> String {
    format!(
        "refused: {name} needs confirmation and agents cannot ask — work around it or report it"
    )
}

/// Résumé livré avec le texte final (`""` sans appels ; les refusés et
/// ratés sont nommés pour que le user voie ce qui n'a pas tourné).
pub fn summarize(calls: &[CallRecord]) -> String {
    if calls.is_empty() {
        return String::new();
    }
    let ok = calls.iter().filter(|c| c.ok && !c.refused).count();
    let refused: Vec<&str> = calls
        .iter()
        .filter(|c| c.refused)
        .map(|c| c.name.as_str())
        .collect();
    let failed: Vec<&str> = calls
        .iter()
        .filter(|c| !c.ok && !c.refused)
        .map(|c| c.name.as_str())
        .collect();
    let plural = if calls.len() > 1 { "s" } else { "" };
    let mut out = format!(
        "\n\n(tools: {} call{plural} — {ok} ok, {} failed, {} refused",
        calls.len(),
        failed.len(),
        refused.len()
    );
    if !failed.is_empty() {
        out.push_str(&format!(" [failed: {}]", failed.join(", ")));
    }
    if !refused.is_empty() {
        out.push_str(&format!(" [refused: {}]", refused.join(", ")));
    }
    out.push(')');
    out
}

/// Un appel : MCP via le parc, natif via l'exécuteur local.
pub async fn execute_one(mcp: &McpState, chat: usize, name: &str, args: &str) -> Outcome {
    if is_mcp_name(name) {
        mcp.call(chat, name, args).await
    } else {
        exec::run_tool(chat, name, args).await
    }
}

/// Boucle outillée d'une réponse d'agent : appels → exécute (ou refuse)
/// → rejoue, jusqu'au texte final ou aux bornes. `history` = paires
/// `(rôle, contenu)`, systèmes déjà préfixés par l'appelant. Erreur du
/// premier tour propagée ; ensuite : texte partiel livré.
pub async fn run_loop(
    ctx: Ctx<'_>,
    history: Vec<(String, String)>,
    tools: Vec<ToolDef>,
) -> anyhow::Result<Completion> {
    let mut wire = tooled_wire(history);
    wire.push(WireMsg::text(
        String::from("system"),
        String::from(AGENT_NOTE),
    ));
    let mut prompt_tokens = 0u64;
    let mut completion_tokens = 0u64;
    let mut calls: Vec<CallRecord> = Vec::new();
    let mut text = String::new();
    for _ in 0..MAX_ROUNDS {
        let Some(done) = llm_turn(ctx, &wire, &tools, &mut text, &calls).await? else {
            break;
        };
        prompt_tokens += done.prompt_tokens;
        completion_tokens += done.completion_tokens;
        push_text(&mut text, &done.text);
        if done.tool_calls.is_empty() {
            break;
        }
        wire.push(WireMsg::assistant_calls(&done.text, &done.tool_calls));
        run_calls(&done.tool_calls, &mut wire, &mut calls, ctx).await;
        if calls.len() >= MAX_CALLS {
            text.push_str(&format!("\n\n(stopped: {MAX_CALLS} tool calls max)"));
            break;
        }
    }
    text.push_str(&summarize(&calls));
    Ok(Completion {
        text,
        tool_calls: Vec::new(),
        prompt_tokens,
        completion_tokens,
    })
}

/// Un tour LLM (`None` = stop : erreur après du partiel, texte annoté ;
/// erreur à vide propagée, comme un appel simple).
async fn llm_turn(
    ctx: Ctx<'_>,
    wire: &[WireMsg],
    tools: &[ToolDef],
    text: &mut String,
    calls: &[CallRecord],
) -> anyhow::Result<Option<Completion>> {
    match ctx
        .provider
        .complete_with_tools(wire, tools, ctx.model, ctx.effort)
        .await
    {
        Ok(done) => Ok(Some(done)),
        Err(err) if text.is_empty() && calls.is_empty() => Err(err),
        Err(err) => {
            text.push_str(&format!("\n\n(interrupted: {err:#})"));
            Ok(None)
        }
    }
}

/// Ajoute un morceau de texte (séparé par une ligne vide, vide ignoré).
fn push_text(text: &mut String, chunk: &str) {
    if chunk.trim().is_empty() {
        return;
    }
    if !text.is_empty() {
        text.push_str("\n\n");
    }
    text.push_str(chunk);
}

/// Exécute les appels du tour (ou refuse), rejoue chaque résultat au fil.
async fn run_calls(
    decided: &[ToolCall],
    wire: &mut Vec<WireMsg>,
    calls: &mut Vec<CallRecord>,
    ctx: Ctx<'_>,
) {
    for call in decided {
        if calls.len() >= MAX_CALLS {
            break;
        }
        let (ok, refused, out) = if allowed(ctx.rules, ctx.mode, &call.name) {
            let out = execute_one(ctx.mcp, ctx.chat, &call.name, &call.args).await;
            (out.ok, false, out.text)
        } else {
            (false, true, refusal(&call.name))
        };
        calls.push(CallRecord {
            name: call.name.clone(),
            ok,
            refused,
        });
        wire.push(WireMsg::result(call.id.clone(), out));
    }
}

#[cfg(test)]
mod tests;
