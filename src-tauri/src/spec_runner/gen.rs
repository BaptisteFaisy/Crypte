//! Génération `/spec new` : 3 étapes chaînées (requirements → design →
//! tasks), écrites + snapshotées, sorties [`SpecOut`] (voir `spec_runner`).

use super::{push_hook_lines, write_doc, SpecOut, SpecRunner, NOT_CONNECTED};
use crate::kiro::hook_run::HooksEngine;
use crate::kiro::hooks::Trigger;
use crate::kiro::spec_run::SpecGen;
use crate::provider::Completion;

impl SpecRunner {
    /// Plan de génération (`/spec new`) : valide nom + absence.
    pub fn gen_plan(&self, name: &str, idea: &str) -> Result<SpecGen, String> {
        if !crate::kiro::spec::valid_name(name) {
            return Err(format!(
                "invalid spec name: {name} — use letters, digits, - and _"
            ));
        }
        let dir = crate::kiro::spec::spec_dir(&self.project, name);
        if ["requirements.md", "design.md", "tasks.md"]
            .iter()
            .any(|file| dir.join(file).is_file())
        {
            return Err(format!("spec {name} already exists — kept as is"));
        }
        if !self.connected {
            return Err(NOT_CONNECTED.to_string());
        }
        Ok(SpecGen::new(name, idea))
    }

    /// Démarre la génération (annonce + premier appel LLM dehors).
    pub fn gen_start(&self, chat: usize, gen: SpecGen) -> Vec<SpecOut> {
        vec![
            SpecOut::ChatLine {
                chat,
                text: format!("spec {}: generating requirements… (0/3)", gen.name),
            },
            SpecOut::SpawnGen { chat, gen },
        ]
    }

    /// Étape de génération reçue : comptée, stockée puis écrite ;
    /// échec ou vide = stop (fichiers déjà écrits gardés).
    pub fn gen_reply(
        &self,
        chat: usize,
        gen: SpecGen,
        reply: anyhow::Result<Completion>,
        hooks: &mut HooksEngine,
    ) -> Vec<SpecOut> {
        let mut outs = Vec::new();
        let done = match reply {
            Ok(done) if !done.text.trim().is_empty() => done,
            Ok(_) => {
                let what = format!("empty {}", gen.file_name());
                return self.gen_failed(chat, &gen, &what);
            }
            Err(err) => {
                let what = format!("{} failed: {err:#}", gen.step_name());
                return self.gen_failed(chat, &gen, &what);
            }
        };
        outs.push(SpecOut::LedgerAdd {
            chat,
            prompt: done.prompt_tokens,
            completion: done.completion_tokens,
        });
        self.gen_store(chat, gen, done, hooks, &mut outs);
        outs
    }

    /// Échec d'étape : note + fichiers déjà écrits gardés.
    fn gen_failed(&self, chat: usize, gen: &SpecGen, what: &str) -> Vec<SpecOut> {
        vec![SpecOut::ChatLine {
            chat,
            text: format!("spec {}: {what} — kept: {}", gen.name, gen.kept_text()),
        }]
    }

    /// Étape reçue : écrit le fichier, affiche la progression (x/3),
    /// enchaîne l'étape suivante ou conclut (résumé final).
    fn gen_store(
        &self,
        chat: usize,
        gen: SpecGen,
        done: Completion,
        hooks: &mut HooksEngine,
        outs: &mut Vec<SpecOut>,
    ) {
        let dir = crate::kiro::spec::spec_dir(&self.project, &gen.name);
        let file = gen.file_name();
        let mut gen = gen;
        gen.store(done.text.clone());
        if let Err(err) = write_doc(self.checkpoint_dir.as_deref(), &dir, file, &done.text) {
            outs.push(SpecOut::ChatLine {
                chat,
                text: format!("spec {}: cannot write {file}: {err}", gen.name),
            });
            return;
        }
        let created = dir.join(file).to_string_lossy().into_owned();
        push_hook_lines(hooks.fire(Trigger::PostFileCreate, &created, chat), outs);
        if gen.advance() {
            outs.push(SpecOut::ChatLine {
                chat,
                text: format!(
                    "spec {}: {file} written ({}/3) — generating {}…",
                    gen.name,
                    gen.step,
                    gen.step_name()
                ),
            });
            outs.push(SpecOut::SpawnGen { chat, gen });
        } else {
            let total = crate::kiro::spec::parse_tasks(&done.text).len();
            outs.push(SpecOut::ChatLine {
                chat,
                text: format!(
                    "spec {} ready: requirements + design + {total} tasks (3/3)",
                    gen.name
                ),
            });
        }
    }
}
