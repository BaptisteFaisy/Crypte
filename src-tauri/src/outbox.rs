//! File des sorties vers la tranche core (`BackendOut` + `take_outbox`).
//!
//! Les commandes de cette tranche ne touchent pas l'état core : elles
//! poussent des sorties ici (extrait de `state.rs`, limite 500 lignes).
//! L'intégrateur draine après chaque commande et à chaque tick (pilote
//! documenté dans `README.md`).

use crate::state::ToolsBackend;

/// Sortie vers la tranche core (file drainée par l'intégrateur, voir
/// `take_outbox` + le pilote documenté dans `README.md`).
#[derive(Debug)]
pub enum BackendOut {
    /// Ligne système sur le chat.
    ChatLine { chat: usize, text: String },
    /// Texte assistant partiel (affichage seul).
    DisplayText { chat: usize, text: String },
    /// Drapeau d'envoi du chat.
    SetSending { chat: usize, sending: bool },
    /// Confirmation requise (émettre `tool_confirm`).
    Confirm {
        chat: usize,
        call: crate::views::ToolCallView,
    },
    /// Exécuter l'appel (`run_tool` natif ou `mcp.call`), puis
    /// `tool_exec_done`.
    SpawnExec {
        chat: usize,
        call: crate::tools::wire::ToolCall,
    },
    /// Compléter `wire` (+ `mcp.tooled_defs()`), puis router la
    /// réponse (`tool_route` si appels, sinon fin + `tool_clear`).
    SpawnLlm {
        chat: usize,
        wire: Vec<crate::tools::wire::WireMsg>,
    },
    /// Livraison (émettre `tool_delivery`).
    Delivered {
        chat: usize,
        delivery: crate::views::ToolDeliveryView,
    },
    /// Boucle outils terminée (libérer le chat).
    Finalize { chat: usize },
    /// Tokens au ledger du chat (étapes `/spec new`).
    LedgerAdd {
        chat: usize,
        prompt: u64,
        completion: u64,
    },
    /// Compléter `gen.messages()` (modèle/effort core), puis
    /// `spec_gen_reply_cmd`.
    SpawnGen {
        chat: usize,
        gen: crate::kiro::spec_run::SpecGen,
    },
    /// Spawner un agent réel (mécanisme `/agent`), puis
    /// `spec_agent_started_cmd` (contexte `name`/`index`/`total`/`task`).
    SpawnAgent {
        chat: usize,
        name: String,
        index: usize,
        total: usize,
        label: String,
        prompt: String,
        task: String,
    },
    /// Confirmation y/n (pas d'event contrat : ligne chat +
    /// `spec_respond`).
    AskConfirm {
        chat: usize,
        name: String,
        label: String,
        task: String,
    },
    /// Progression (émettre `spec_progress`).
    Progress {
        name: String,
        label: String,
        done: usize,
        total: usize,
    },
    /// Recherche web faite (ledger + event `web_answer` core).
    WebDone {
        chat: usize,
        backend: String,
        text: String,
        prompt: u64,
        completion: u64,
    },
    /// Bulle critique de Kiro (émettre `critic`).
    Critique { text: String },
}

impl ToolsBackend {
    /// Prend les sorties en attente (l'intégrateur draine après
    /// chaque commande de cette tranche et à chaque tick).
    pub fn take_outbox(&mut self) -> Vec<BackendOut> {
        std::mem::take(&mut self.outbox)
    }

    /// Route une réponse LLM dans la boucle outils (l'intégrateur
    /// appelle quand la réponse porte des appels + budget + tools on).
    pub fn tool_route(
        &mut self,
        chat: usize,
        history: Vec<(String, String)>,
        completion: &crate::provider::Completion,
    ) {
        let mut gate = crate::tools::flow::GateCtx {
            rules: &self.rules,
            mode: self.permissions,
            mcp: &self.mcp,
            hooks: &mut self.hooks,
        };
        let outs = self.flow.route_calls(chat, history, completion, &mut gate);
        self.push_tool_outs(outs);
    }

    /// Résout la modale y/n (commande `tool_decide`).
    pub fn tool_decide_cmd(&mut self, allow: bool) {
        let mut gate = crate::tools::flow::GateCtx {
            rules: &self.rules,
            mode: self.permissions,
            mcp: &self.mcp,
            hooks: &mut self.hooks,
        };
        let outs = self.flow.decide(allow, &mut gate);
        self.push_tool_outs(outs);
    }

    /// Exécutions terminées (l'intégrateur a run `SpawnExec`).
    pub fn tool_exec_done(&mut self, chat: usize, deliveries: Vec<crate::tools::flow::Delivery>) {
        let mut gate = crate::tools::flow::GateCtx {
            rules: &self.rules,
            mode: self.permissions,
            mcp: &self.mcp,
            hooks: &mut self.hooks,
        };
        let outs = self.flow.exec_done(chat, deliveries, &mut gate);
        self.push_tool_outs(outs);
    }

    /// Avorte la boucle outils (ex : plus de provider).
    pub fn tool_abort(&mut self, note: &str) {
        let outs = self.flow.abort_current(note);
        self.push_tool_outs(outs);
    }

    /// Oublie la boucle du chat (réponse finale ou Ctrl+C, silencieux).
    pub fn tool_clear(&mut self, chat: usize) {
        self.flow.clear(chat);
    }

    /// Démarre une génération `/spec new` (commande `spec_new`).
    pub fn spec_gen_start_cmd(
        &mut self,
        name: &str,
        idea: &str,
        chat: usize,
    ) -> Result<(), String> {
        let gen = self.specs.gen_plan(name, idea)?;
        let outs = self.specs.gen_start(chat, gen);
        self.push_spec_outs(outs);
        Ok(())
    }

    /// Étape de génération reçue (l'intégrateur a run `SpawnGen`).
    pub fn spec_gen_reply_cmd(
        &mut self,
        chat: usize,
        gen: crate::kiro::spec_run::SpecGen,
        reply: anyhow::Result<crate::provider::Completion>,
    ) {
        let outs = self.specs.gen_reply(chat, gen, reply, &mut self.hooks);
        self.push_spec_outs(outs);
    }

    /// Démarre un run `/spec run` (commande `spec_run`).
    /// Rend `(started, label)` pour le contrat.
    pub fn spec_run_start_cmd(&mut self, name: &str, chat: usize) -> (bool, String) {
        let outs = self.specs.run_start(name, chat);
        let mut started = false;
        let mut label = String::new();
        for out in &outs {
            if let crate::spec_runner::SpecOut::SpawnAgent { label: l, .. } = out {
                started = true;
                label = l.clone();
            }
        }
        self.push_spec_outs(outs);
        (started, label)
    }

    /// Agent de tâche spawné (l'intégrateur a run `SpawnAgent`).
    pub fn spec_agent_started_cmd(
        &mut self,
        name: &str,
        chat: usize,
        index: usize,
        total: usize,
        agent_id: usize,
        task: &str,
    ) {
        let outs =
            self.specs
                .agent_started(name, chat, index, total, agent_id, task, &mut self.hooks);
        self.push_spec_outs(outs);
    }

    /// Tâche finie (réponse agent livrée).
    pub fn spec_agent_done_cmd(&mut self, agent_id: usize) {
        let outs = self.specs.agent_done(agent_id, &mut self.hooks);
        self.push_spec_outs(outs);
    }

    /// Tâche échouée (agent en erreur).
    pub fn spec_agent_failed_cmd(&mut self, agent_id: usize) {
        let outs = self.specs.agent_failed(agent_id, &mut self.hooks);
        self.push_spec_outs(outs);
    }

    /// Réponse y/n (commande `spec_respond`).
    pub fn spec_respond_cmd(&mut self, text: &str) {
        let outs = self.specs.respond(text);
        self.push_spec_outs(outs);
    }

    /// File les sorties outils (présage appliqué en interne).
    fn push_tool_outs(&mut self, outs: Vec<crate::tools::flow::ToolOut>) {
        use crate::tools::flow::ToolOut;
        for out in outs {
            match out {
                ToolOut::ChatLine { chat, text } => {
                    self.outbox.push(BackendOut::ChatLine { chat, text })
                }
                ToolOut::DisplayText { chat, text } => {
                    self.outbox.push(BackendOut::DisplayText { chat, text });
                }
                ToolOut::SetSending { chat, sending } => {
                    self.outbox.push(BackendOut::SetSending { chat, sending });
                }
                ToolOut::Confirm { chat, call } => self.outbox.push(BackendOut::Confirm {
                    chat,
                    call: crate::views::ToolCallView::from(&call),
                }),
                ToolOut::SpawnExec { chat, call } => {
                    self.outbox.push(BackendOut::SpawnExec { chat, call });
                }
                ToolOut::SpawnLlm { chat, wire } => {
                    self.outbox.push(BackendOut::SpawnLlm { chat, wire });
                }
                ToolOut::Delivered { chat, delivery } => self.outbox.push(BackendOut::Delivered {
                    chat,
                    delivery: crate::views::ToolDeliveryView::from(&delivery),
                }),
                ToolOut::OmenError => self.note_omen("error"),
                ToolOut::Finalize { chat } => self.outbox.push(BackendOut::Finalize { chat }),
            }
        }
    }

    /// File les sorties specs (génération + exécution).
    fn push_spec_outs(&mut self, outs: Vec<crate::spec_runner::SpecOut>) {
        use crate::spec_runner::SpecOut;
        for out in outs {
            match out {
                SpecOut::ChatLine { chat, text } => {
                    self.outbox.push(BackendOut::ChatLine { chat, text })
                }
                SpecOut::LedgerAdd {
                    chat,
                    prompt,
                    completion,
                } => self.outbox.push(BackendOut::LedgerAdd {
                    chat,
                    prompt,
                    completion,
                }),
                SpecOut::SpawnGen { chat, gen } => {
                    self.outbox.push(BackendOut::SpawnGen { chat, gen })
                }
                run => self.push_spec_run(run),
            }
        }
    }

    /// File les sorties d'exécution (spawn, confirmation, progression).
    fn push_spec_run(&mut self, out: crate::spec_runner::SpecOut) {
        use crate::spec_runner::SpecOut;
        match out {
            SpecOut::SpawnAgent {
                chat,
                name,
                index,
                total,
                label,
                prompt,
                task,
            } => self.outbox.push(BackendOut::SpawnAgent {
                chat,
                name,
                index,
                total,
                label,
                prompt,
                task,
            }),
            SpecOut::AskConfirm {
                chat,
                name,
                label,
                task,
            } => self.outbox.push(BackendOut::AskConfirm {
                chat,
                name,
                label,
                task,
            }),
            SpecOut::Progress {
                name,
                label,
                done,
                total,
            } => self.outbox.push(BackendOut::Progress {
                name,
                label,
                done,
                total,
            }),
            other => self.push_spec_outs(vec![other]),
        }
    }
}
