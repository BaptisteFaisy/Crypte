//! Commandes chat §1 (méta) : compaction, modèle, thinking, usage, jauge.
//!
//! `chat_compact` attend le résumé (le contrat rend `CompactResult`
//! directement, pas d'ACK) ; le reste est synchrone.

use crate::compact::{without_pinned, COMPACT_INSTRUCTION, COMPACT_MIN};
use crate::core::{Core, NOT_CONNECTED};
use crate::drain;
use crate::dto::{CompactResult, CtxGauge, DayTallyDto, UsageReport};
use crate::events::Event;
use crate::slash::{effort_values, valid_effort};

impl Core {
    /// `chat_compact` : résume l'historique (agents.md exclu du résumé
    /// méta, conservé en tête — pinné? + résumé + 4). Rend `{before, after}`.
    pub async fn chat_compact(&self, index: usize) -> Result<CompactResult, String> {
        let plan = {
            let mut inner = self.lock();
            guard_compact(&mut inner, index)?
        };
        let (provider, pinned, history, model, effort) = plan;
        let mut messages = vec![(String::from("system"), String::from(COMPACT_INSTRUCTION))];
        messages.extend(without_pinned(&history, pinned).iter().cloned());
        let reply = provider
            .complete_full(&messages, &model, effort.as_deref())
            .await;
        let failed = reply
            .as_ref()
            .err()
            .map(|err| format!("compaction failed: {err:#}"));
        let emit = |event: Event| self.emit(event);
        let mut inner = self.lock();
        match reply {
            Ok(done) => {
                inner
                    .almanac
                    .record(done.prompt_tokens, done.completion_tokens);
                match drain::drain_compact(&mut inner, &emit, index, Ok(done)) {
                    Some((before, after)) => Ok(CompactResult { before, after }),
                    None => Err(String::from("chat closed during compaction")),
                }
            }
            Err(err) => {
                drain::drain_compact(&mut inner, &emit, index, Err(err));
                Err(failed.expect("erreur formatée ci-dessus"))
            }
        }
    }

    /// `chat_set_model` : modèle du chat (`null`/`reset`/`default` = défaut
    /// provider). Rend le modèle effectif.
    pub fn chat_set_model(&self, index: usize, model: Option<String>) -> Result<String, String> {
        let mut inner = self.lock();
        let Some(sess) = inner.sessions.get_mut(index) else {
            return Err(format!("chat {} not found", index + 1));
        };
        match model.as_deref() {
            None | Some("reset") | Some("default") => sess.model = None,
            Some(name) => sess.model = Some(name.to_string()),
        }
        Ok(inner.effective_model_for(index))
    }

    /// `chat_set_thinking` : effort de raisonnement global (`null`/`default`
    /// = défaut). Rend l'effort effectif.
    pub fn chat_set_thinking(&self, level: Option<String>) -> Result<String, String> {
        let mut inner = self.lock();
        match level.as_deref() {
            None | Some("default") => {
                inner.thinking = None;
            }
            Some(v) if valid_effort(v) => {
                inner.thinking = Some(v.to_string());
            }
            Some(v) => {
                let mut vals = effort_values();
                vals.push("default");
                return Err(format!("invalid effort: {v} — values: {}", vals.join(", ")));
            }
        }
        Ok(inner.thinking_text())
    }

    /// `usage_get` : tokens du chat + total session + almanach (`calls`
    /// chat/session = 0 : le registre ne compte pas les appels).
    pub fn usage_get(&self, index: usize) -> Result<UsageReport, String> {
        let mut inner = self.lock();
        // D'autres cryptes ont pu écrire : relire avant d'afficher.
        inner.almanac.refresh();
        let Some(sess) = inner.sessions.get(index) else {
            return Err(format!("chat {} not found", index + 1));
        };
        let chat = DayTallyDto {
            prompt: sess.ledger.prompt,
            completion: sess.ledger.completion,
            calls: 0,
        };
        let total = inner.sessions.total_ledger();
        let session = DayTallyDto {
            prompt: total.prompt,
            completion: total.completion,
            calls: 0,
        };
        let report = UsageReport {
            chat,
            session,
            almanac_total: DayTallyDto::from(inner.almanac.all_time()),
            almanac_days: inner.almanac.nights(),
        };
        Ok(report)
    }

    /// `ctx_gauge` : `{used, window}` du chat `index` (fenêtre d'envoi +
    /// saisie connue, sans copie).
    pub fn ctx_gauge(&self, index: usize) -> Result<CtxGauge, String> {
        let inner = self.lock();
        if inner.sessions.get(index).is_none() {
            return Err(format!("chat {} not found", index + 1));
        }
        Ok(CtxGauge {
            used: inner.context_used_for(index),
            window: inner.context_limit_for(index),
        })
    }
}

/// Plan de compaction : provider + historique + modèle + effort.
type CompactPlan = (
    std::sync::Arc<crate::provider::Provider>,
    bool,
    Vec<(String, String)>,
    String,
    Option<String>,
);

/// Gardes de compaction : chat présent, assez d'historique, connecté.
/// Pose `waiting`, rend le plan (notes poussées si refus).
fn guard_compact(inner: &mut crate::core::CoreInner, index: usize) -> Result<CompactPlan, String> {
    let Some(sess) = inner.sessions.get(index) else {
        return Err(format!("chat {} not found", index + 1));
    };
    if sess.history.len() < COMPACT_MIN {
        let msg = String::from("nothing to compact");
        note(inner, index, msg.clone());
        return Err(msg);
    }
    let Some(provider) = inner.provider.clone() else {
        note(inner, index, String::from(NOT_CONNECTED));
        return Err(String::from(NOT_CONNECTED));
    };
    inner.sessions.get_mut(index).unwrap().waiting = true;
    let sess = inner.sessions.get(index).unwrap();
    Ok((
        provider,
        sess.pinned_system,
        sess.history.clone(),
        inner.effective_model_for(index),
        inner.thinking.clone(),
    ))
}

/// Pousse une note système dans le chat `index` (chat validé).
fn note(inner: &mut crate::core::CoreInner, index: usize, msg: String) {
    inner
        .sessions
        .get_mut(index)
        .unwrap()
        .chat
        .push(crate::chat_state::Role::System, msg);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tests::test_core;

    #[test]
    fn model_et_thinking() {
        let (core, _channels, dir) = test_core("meta");
        assert_eq!(
            core.chat_set_model(0, Some(String::from("spectre-1")))
                .unwrap(),
            "spectre-1"
        );
        assert_eq!(core.chat_set_model(0, None).unwrap(), "—");
        assert!(core.chat_set_model(9, None).is_err());
        assert_eq!(
            core.chat_set_thinking(Some(String::from("high"))).unwrap(),
            "high"
        );
        assert_eq!(core.chat_set_thinking(None).unwrap(), "default");
        let bad = core
            .chat_set_thinking(Some(String::from("max")))
            .unwrap_err();
        assert!(bad.contains("invalid effort"), "{bad}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn usage_et_jauge() {
        let (core, _channels, dir) = test_core("usage");
        core.lock()
            .sessions
            .current_mut()
            .ledger
            .record(10, 20, 1.0);
        core.lock()
            .sessions
            .current_mut()
            .push_history("user", String::from("salut"));
        let report = core.usage_get(0).unwrap();
        assert_eq!(report.chat.prompt, 10);
        assert_eq!(report.chat.completion, 20);
        assert_eq!(report.session.prompt, 10);
        assert_eq!(report.almanac_total.calls, 0);
        assert!(core.usage_get(9).is_err());
        let gauge = core.ctx_gauge(0).unwrap();
        assert!(gauge.used > 0);
        assert_eq!(gauge.window, 128_000);
        assert!(core.ctx_gauge(9).is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn compact_refuse_petit_et_deconnecte() {
        let (core, _channels, dir) = test_core("compact");
        assert_eq!(core.chat_compact(9).await.unwrap_err(), "chat 10 not found");
        assert_eq!(
            core.chat_compact(0).await.unwrap_err(),
            "nothing to compact"
        );
        {
            let mut inner = core.lock();
            for i in 0..COMPACT_MIN {
                inner
                    .sessions
                    .current_mut()
                    .push_history("user", format!("m{i}"));
            }
        }
        assert_eq!(core.chat_compact(0).await.unwrap_err(), NOT_CONNECTED);
        std::fs::remove_dir_all(dir).ok();
    }
}
