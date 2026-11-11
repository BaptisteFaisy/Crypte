//! Boucle agent outillée, version Tauri (sans `App`, sans touches).
//!
//! Reprend de `src/tools/flow.rs` + `src/permissions_cmds.rs` (TUI) la
//! machine : file séquentielle, porte `allow`/`ask`/`deny`, résolutions,
//! livraisons rejouées au transcript, relance LLM jusqu'à la réponse
//! finale (sans limite de tours, comme le TUI depuis #106).
//! Adaptation : les effets de bord sortent en [`ToolOut`]
//! (l'intégrateur les exécute : écritures chat, `spawn` exec/LLM,
//! events `tool_confirm`/`tool_delivery`), les touches y/n deviennent
//! [`ToolFlow::decide`].

use std::collections::VecDeque;

use super::exec::Outcome;
use super::wire::{ToolCall, WireMsg};
use super::{defs, GUIDE};
use crate::kiro::hook_run::{HookOut, HooksEngine};
use crate::kiro::hooks::Trigger;
use crate::kiro::mcp_pool::McpState;
use crate::permissions::{Decision, Mode, Rules};
use crate::provider::{truncate, Completion};

/// Boucle agent en cours (un seul envoi outillé à la fois).
#[derive(Debug)]
pub struct ToolRun {
    /// Chat d'origine (l'affichage et le ledger y restent).
    pub chat: usize,
    /// Transcript filaire (historique + guide + appels + résultats).
    pub wire: Vec<WireMsg>,
    /// Appels décidés, exécutés dans l'ordre.
    pub queue: VecDeque<ToolCall>,
    /// Confirmation y/n attendue (modale frontend).
    pub confirming: bool,
}

/// Livraison d'exécution (appel + résultat, affichée puis rejouée).
#[derive(Debug, Clone)]
pub struct Delivery {
    pub call: ToolCall,
    pub ok: bool,
    pub text: String,
    pub ms: u64,
}

impl Delivery {
    /// Livraison depuis un [`Outcome`] d'exécution.
    pub fn from_outcome(call: ToolCall, out: Outcome, ms: u64) -> Self {
        Self {
            call,
            ok: out.ok,
            text: out.text,
            ms,
        }
    }
}

/// Sortie de la machine pour l'intégrateur.
#[derive(Debug)]
pub enum ToolOut {
    /// Ligne système sur le chat (affichage + historique).
    ChatLine { chat: usize, text: String },
    /// Texte assistant partiel (affichage seul, hors historique).
    DisplayText { chat: usize, text: String },
    /// Drapeau d'envoi du chat.
    SetSending { chat: usize, sending: bool },
    /// Confirmation requise (modale y/n + event `tool_confirm`).
    Confirm { chat: usize, call: ToolCall },
    /// Exécuter l'appel (natif ou `mcp_*`), puis `exec_done`.
    SpawnExec { chat: usize, call: ToolCall },
    /// Relancer le LLM (outils = `mcp.tooled_defs().await`), puis
    /// router la réponse (`route_calls` si appels, sinon fin + `clear`).
    SpawnLlm { chat: usize, wire: Vec<WireMsg> },
    /// Livraison à afficher + event `tool_delivery`.
    Delivered { chat: usize, delivery: Delivery },
    /// Échec d'outil : présage erreur sur l'apparition.
    OmenError,
    /// Boucle terminée (libérer le chat).
    Finalize { chat: usize },
}

/// Contexte de porte, fourni par l'intégrateur à chaque appel.
pub struct GateCtx<'a> {
    pub rules: &'a Rules,
    pub mode: Mode,
    pub mcp: &'a McpState,
    pub hooks: &'a mut HooksEngine,
}

/// Boucle outils (zéro état global, méthodes synchrones).
#[derive(Debug, Default)]
pub struct ToolFlow {
    run: Option<ToolRun>,
}

impl ToolFlow {
    /// Boucle vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Transcript initial : fenêtre d'envoi + guide outils (transitoire).
    pub fn begin(&mut self, chat: usize, history: Vec<(String, String)>) {
        self.run = Some(ToolRun {
            chat,
            wire: tooled_wire(history),
            queue: VecDeque::new(),
            confirming: false,
        });
    }

    /// Chat de la boucle en cours, s'il y en a une.
    pub fn active_chat(&self) -> Option<usize> {
        self.run.as_ref().map(|run| run.chat)
    }

    /// Confirmation modale attendue ?
    pub fn is_confirming(&self) -> bool {
        self.run.as_ref().is_some_and(|run| run.confirming)
    }

    /// Appel en attente de confirmation, le cas échéant.
    pub fn pending_call(&self) -> Option<ToolCall> {
        let run = self.run.as_ref()?;
        if !run.confirming {
            return None;
        }
        run.queue.front().cloned()
    }

    /// Réponse avec appels : affiche le texte (hors historique), prépare
    /// la boucle (transcript + file) et avance (confirme ou exécute).
    /// `history` = fenêtre d'envoi (ignorée si boucle déjà ouverte).
    pub fn route_calls(
        &mut self,
        chat: usize,
        history: Vec<(String, String)>,
        done: &Completion,
        gate: &mut GateCtx,
    ) -> Vec<ToolOut> {
        let mut outs = vec![ToolOut::SetSending {
            chat,
            sending: true,
        }];
        if !done.text.trim().is_empty() {
            outs.push(ToolOut::DisplayText {
                chat,
                text: done.text.clone(),
            });
        }
        if self.run.is_none() {
            self.begin(chat, history);
        }
        // Frontière core/outils : mêmes champs, types distincts.
        let calls: Vec<ToolCall> = done
            .tool_calls
            .iter()
            .map(|c| ToolCall {
                id: c.id.clone(),
                name: c.name.clone(),
                args: c.args.clone(),
            })
            .collect();
        let turn = WireMsg::assistant_calls(&done.text, &calls);
        if let Some(run) = self.run.as_mut() {
            run.wire.push(turn);
            run.queue.extend(calls);
        }
        self.advance(gate, &mut outs);
        outs
    }

    /// Résolution y/n de la modale : exécute ou note le refus, avance.
    /// Sans confirmation en attente : ne fait rien.
    pub fn decide(&mut self, allow: bool, gate: &mut GateCtx) -> Vec<ToolOut> {
        let mut outs = Vec::new();
        let popped = self.run.as_mut().and_then(|run| {
            if !run.confirming {
                return None;
            }
            run.confirming = false;
            run.queue.pop_front().map(|call| (run.chat, call))
        });
        let Some((chat, call)) = popped else {
            return outs;
        };
        if allow {
            push_hook_lines(
                gate.hooks.fire(Trigger::PreToolUse, &call.name, chat),
                &mut outs,
            );
            outs.push(ToolOut::ChatLine {
                chat,
                text: format!("[tool {}]", call_line(&call)),
            });
            outs.push(ToolOut::SpawnExec { chat, call });
        } else {
            outs.push(ToolOut::ChatLine {
                chat,
                text: format!("denied {}", call_line(&call)),
            });
            let denied = Delivery {
                call,
                ok: false,
                text: String::from("denied by user"),
                ms: 0,
            };
            self.deliver(vec![denied], &mut outs);
            self.advance(gate, &mut outs);
        }
        outs
    }

    /// Exécution(s) terminée(s) : affiche, rejoue au transcript, avance.
    /// Boucle absente ou autre chat : livraison périmée, ignorée.
    pub fn exec_done(
        &mut self,
        chat: usize,
        deliveries: Vec<Delivery>,
        gate: &mut GateCtx,
    ) -> Vec<ToolOut> {
        let mut outs = Vec::new();
        if self.active_chat() != Some(chat) {
            return outs;
        }
        for delivery in &deliveries {
            push_hook_lines(
                gate.hooks
                    .fire(Trigger::PostToolUse, &delivery.call.name, chat),
                &mut outs,
            );
        }
        self.deliver(deliveries, &mut outs);
        self.advance(gate, &mut outs);
        outs
    }

    /// Oublie la boucle du chat (réponse finale ou Ctrl+C, silencieux
    /// comme le TUI : `drain_chat`/`stop_chat`).
    pub fn clear(&mut self, chat: usize) {
        if self.active_chat() == Some(chat) {
            self.run = None;
        }
    }

    /// Avorte la boucle (chat libéré + note système).
    pub fn abort_current(&mut self, note: &str) -> Vec<ToolOut> {
        let Some(run) = self.run.take() else {
            return Vec::new();
        };
        vec![
            ToolOut::SetSending {
                chat: run.chat,
                sending: false,
            },
            ToolOut::ChatLine {
                chat: run.chat,
                text: note.to_string(),
            },
            ToolOut::Finalize { chat: run.chat },
        ]
    }

    /// Avance la boucle : prochain appel (confirme, exécute ou refuse)
    /// ou relance LLM. Les refus continuent, le reste attend.
    fn advance(&mut self, gate: &mut GateCtx, outs: &mut Vec<ToolOut>) {
        loop {
            let next = self.run.as_ref().and_then(|run| run.queue.front().cloned());
            let Some(call) = next else {
                self.request_llm(outs);
                return;
            };
            if !self.gate_next(call, gate, outs) {
                return;
            }
        }
    }

    /// File vide : demande une relance LLM avec le transcript.
    fn request_llm(&self, outs: &mut Vec<ToolOut>) {
        if let Some(run) = self.run.as_ref() {
            outs.push(ToolOut::SpawnLlm {
                chat: run.chat,
                wire: run.wire.clone(),
            });
        }
    }

    /// Porte un appel (`ask`/`deny`/`allow`) ; rend faux si la boucle
    /// attend (confirmation ou exécution), vrai pour continuer.
    fn gate_next(&mut self, call: ToolCall, gate: &mut GateCtx, outs: &mut Vec<ToolOut>) -> bool {
        match gate.rules.decide(gate.mode, &call.name) {
            Decision::Ask if !gate.mcp.is_auto(&call.name) => {
                self.ask_confirm(call, outs);
                false
            }
            Decision::Deny => {
                self.deny_rule(call, outs);
                true
            }
            Decision::Ask | Decision::Allow => {
                self.allow_spawn(call, gate, outs);
                false
            }
        }
    }

    /// Demande confirmation (ligne + modale, boucle en attente).
    fn ask_confirm(&mut self, call: ToolCall, outs: &mut Vec<ToolOut>) {
        if let Some(run) = self.run.as_mut() {
            run.confirming = true;
        }
        let chat = self.active_chat().unwrap_or(0);
        outs.push(ToolOut::ChatLine {
            chat,
            text: format!("allow {}? (y/n)", call_line(&call)),
        });
        outs.push(ToolOut::Confirm { chat, call });
    }

    /// Refus par règle (livré `denied`, la boucle continue).
    fn deny_rule(&mut self, call: ToolCall, outs: &mut Vec<ToolOut>) {
        let chat = self.pop_front().unwrap_or(0);
        outs.push(ToolOut::ChatLine {
            chat,
            text: format!("denied {} (rule)", call.name),
        });
        self.deliver(
            vec![Delivery {
                call,
                ok: false,
                text: String::from("denied by permissions rule"),
                ms: 0,
            }],
            outs,
        );
    }

    /// Autorisé : hooks `PreToolUse` + exécution (boucle en attente).
    fn allow_spawn(&mut self, call: ToolCall, gate: &mut GateCtx, outs: &mut Vec<ToolOut>) {
        let chat = self.pop_front().unwrap_or(0);
        push_hook_lines(gate.hooks.fire(Trigger::PreToolUse, &call.name, chat), outs);
        outs.push(ToolOut::ChatLine {
            chat,
            text: format!("[tool {}]", call_line(&call)),
        });
        outs.push(ToolOut::SpawnExec { chat, call });
    }

    /// Dépile l'appel en tête (rend le chat d'origine).
    fn pop_front(&mut self) -> Option<usize> {
        self.run.as_mut().map(|run| {
            run.queue.pop_front();
            run.chat
        })
    }

    /// Livraisons : affiche chaque résultat, rejoue au transcript.
    fn deliver(&mut self, deliveries: Vec<Delivery>, outs: &mut Vec<ToolOut>) {
        let mut failed = false;
        let mut results = Vec::with_capacity(deliveries.len());
        for delivery in &deliveries {
            failed |= !delivery.ok;
            results.push(WireMsg::result(
                delivery.call.id.clone(),
                delivery.text.clone(),
            ));
        }
        if let Some(run) = self.run.as_mut() {
            let chat = run.chat;
            run.wire.extend(results);
            for delivery in &deliveries {
                outs.push(ToolOut::ChatLine {
                    chat,
                    text: done_line(delivery),
                });
                outs.push(ToolOut::Delivered {
                    chat,
                    delivery: delivery.clone(),
                });
            }
        }
        if failed {
            outs.push(ToolOut::OmenError);
        }
    }
}

/// Lignes hooks → sorties boucle (même texte, même chat).
fn push_hook_lines(fired: Vec<HookOut>, outs: &mut Vec<ToolOut>) {
    for out in fired {
        let HookOut::ChatLine { chat, text } = out;
        outs.push(ToolOut::ChatLine { chat, text });
    }
}

/// Fil initial outillé : historique + guide transitoire (non persisté).
pub fn tooled_wire(history: Vec<(String, String)>) -> Vec<WireMsg> {
    let mut wire: Vec<WireMsg> = history
        .into_iter()
        .map(|(role, content)| WireMsg::text(role, content))
        .collect();
    wire.push(WireMsg::text(String::from("system"), String::from(GUIDE)));
    wire
}

/// Une ligne d'appel : `nom {args…}` (args tronqués à 120 caractères).
fn call_line(call: &ToolCall) -> String {
    format!("{} {}", call.name, truncate(&call.args, 120))
}

/// Ligne de résultat : `nom → ok/FAILED, N chars, N ms` (+ extrait si échec).
fn done_line(delivery: &Delivery) -> String {
    let status = if delivery.ok { "ok" } else { "FAILED" };
    let mut line = format!(
        "[tool {} → {status}, {} chars, {} ms]",
        delivery.call.name,
        delivery.text.chars().count(),
        delivery.ms
    );
    if !delivery.ok {
        line.push_str(&format!(" {}", truncate(&delivery.text, 200)));
    }
    line
}

/// Ligne d'état `/tools` : on/off + nombre d'outils et familles.
fn tools_line(on: bool) -> String {
    if on {
        format!(
            "tools: on ({} tools: browser, desktop, shell)",
            defs().len()
        )
    } else {
        String::from("tools: off")
    }
}

/// Ligne d'état pour `tools_status` (même texte que le TUI).
pub fn status_line(on: bool) -> String {
    tools_line(on)
}

#[cfg(test)]
mod tests;
