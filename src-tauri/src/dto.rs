//! DTO des commandes : miroirs Rust de `contracts.ts` (parties §1–§3, §7).
//!
//! Sérialisation `camelCase` (args TS) / champs `snake_case` calqués sur
//! les interfaces TS. Aucun secret ne transite ici.

use serde::{Deserialize, Serialize};

use crate::almanac::DayTally as AlmanacTally;
use crate::bus::{Link as BusLinkData, LinkEnd as BusEnd};
use crate::chat_state::{Message, Role};
use crate::question::PendingQuestion as PendingQ;
use crate::sessions::{AgentStatus, ChatSession, SubAgent};
use crate::settings::Row as ProviderRow;

// ---------------------------------------------------------------- chat

/// Message affiché (`ChatMessage`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageDto {
    pub role: Role,
    pub text: String,
}

impl From<&Message> for ChatMessageDto {
    fn from(msg: &Message) -> Self {
        Self {
            role: msg.role,
            text: msg.text.clone(),
        }
    }
}

/// Question en attente (`PendingQuestion`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingQuestionDto {
    pub question: String,
    pub options: Vec<String>,
}

impl From<&PendingQ> for PendingQuestionDto {
    fn from(q: &PendingQ) -> Self {
        Self {
            question: q.question.clone(),
            options: q.options.clone(),
        }
    }
}

/// Résumé d'un chat (`ChatSummary`, index 0-based).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSummary {
    pub index: usize,
    pub name: String,
    pub model: Option<String>,
    pub sending: bool,
    pub question_pending: bool,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

impl ChatSummary {
    /// Résumé du chat `index`.
    pub fn of(index: usize, chat: &ChatSession) -> Self {
        Self {
            index,
            name: chat.name.clone(),
            model: chat.model.clone(),
            sending: chat.sending,
            question_pending: chat.pending.is_some(),
            prompt_tokens: chat.ledger.prompt,
            completion_tokens: chat.ledger.completion,
        }
    }
}

/// État complet d'un chat (`ChatState`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStateDto {
    pub index: usize,
    pub name: String,
    pub model: Option<String>,
    pub sending: bool,
    pub question_pending: bool,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub messages: Vec<ChatMessageDto>,
    pub input: String,
    pub pending_question: Option<PendingQuestionDto>,
}

impl ChatStateDto {
    /// État complet du chat `index`.
    pub fn of(index: usize, chat: &ChatSession) -> Self {
        let summary = ChatSummary::of(index, chat);
        Self {
            index: summary.index,
            name: summary.name,
            model: summary.model,
            sending: summary.sending,
            question_pending: summary.question_pending,
            prompt_tokens: summary.prompt_tokens,
            completion_tokens: summary.completion_tokens,
            messages: chat
                .chat
                .messages()
                .iter()
                .map(ChatMessageDto::from)
                .collect(),
            input: chat.chat.input.clone(),
            pending_question: chat.pending.as_ref().map(PendingQuestionDto::from),
        }
    }
}

/// Accusé d'envoi (`SendAck`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendAck {
    pub accepted: bool,
    pub reason: Option<String>,
}

impl SendAck {
    /// Envoi accepté.
    pub fn accepted() -> Self {
        Self {
            accepted: true,
            reason: None,
        }
    }

    /// Envoi refusé (`reason` = motif, ex "not connected…", "no chat").
    pub fn refused(reason: impl Into<String>) -> Self {
        Self {
            accepted: false,
            reason: Some(reason.into()),
        }
    }
}

/// Résultat de compaction (`CompactResult`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactResult {
    pub before: usize,
    pub after: usize,
}

/// Jauge de contexte (`{used, window}`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CtxGauge {
    pub used: u64,
    pub window: u64,
}

/// Compteurs journaliers (`DayTally`).
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayTallyDto {
    pub prompt: u64,
    pub completion: u64,
    pub calls: u64,
}

impl From<AlmanacTally> for DayTallyDto {
    fn from(t: AlmanacTally) -> Self {
        Self {
            prompt: t.prompt,
            completion: t.completion,
            calls: t.calls,
        }
    }
}

/// Rapport d'usage (`UsageReport`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    pub chat: DayTallyDto,
    pub session: DayTallyDto,
    pub almanac_total: DayTallyDto,
    pub almanac_days: usize,
}

// ------------------------------------------------------- agents + bus

/// Sous-agent (`SubAgentInfo`, ids : agent global 1-based, chats 0-based).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentInfo {
    pub id: usize,
    pub owner: usize,
    pub deliver: usize,
    pub status: String,
    pub prompt: String,
    pub last_text: Option<String>,
}

impl SubAgentInfo {
    /// Vue de l'agent (`owner` = chat propriétaire 0-based).
    pub fn of(owner: usize, agent: &SubAgent) -> Self {
        let status = match agent.status {
            AgentStatus::Running => "running",
            AgentStatus::Done => "done",
            AgentStatus::Failed => "failed",
        };
        Self {
            id: agent.id,
            owner,
            deliver: agent.deliver,
            status: status.to_string(),
            prompt: agent.prompt.clone(),
            last_text: agent.last_text(),
        }
    }
}

/// Extrémité de lien (`LinkEnd` : chat 0-based, agent id global).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LinkEndDto {
    Chat { id: usize },
    Agent { id: usize },
}

impl From<BusEnd> for LinkEndDto {
    fn from(end: BusEnd) -> Self {
        match end {
            BusEnd::Chat(id) => Self::Chat { id },
            BusEnd::Agent(id) => Self::Agent { id },
        }
    }
}

impl From<LinkEndDto> for BusEnd {
    fn from(end: LinkEndDto) -> Self {
        match end {
            LinkEndDto::Chat { id } => Self::Chat(id),
            LinkEndDto::Agent { id } => Self::Agent(id),
        }
    }
}

/// Lien auto (`BusLink`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BusLinkDto {
    pub from: LinkEndDto,
    pub to: LinkEndDto,
}

impl From<&BusLinkData> for BusLinkDto {
    fn from(link: &BusLinkData) -> Self {
        Self {
            from: LinkEndDto::from(link.from),
            to: LinkEndDto::from(link.to),
        }
    }
}

/// État du bus (`BusState`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BusStateDto {
    pub links: Vec<BusLinkDto>,
    pub forwarded: u64,
    pub capped: u64,
}

// ------------------------------------------------------- providers

/// Provider (`ProviderInfo`, jamais la clé).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub has_key: bool,
    pub known: bool,
}

impl From<&ProviderRow> for ProviderInfo {
    fn from(row: &ProviderRow) -> Self {
        Self {
            name: row.name.clone(),
            base_url: row.base_url.clone(),
            model: row.model.clone(),
            has_key: row.has_key,
            known: row.known,
        }
    }
}

/// État de connexion (`ConnectStatus`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectStatus {
    pub connected: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
}

/// Résultat de connexion (`ConnectResult`, erreur sans secret).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectResult {
    pub ok: bool,
    pub error: Option<String>,
    pub status: ConnectStatus,
}

// ------------------------------------------------------- bestiaire

/// Créature (`Creature`, grille 32×32).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatureDto {
    pub slug: String,
    pub name: String,
    pub lore: String,
    pub grid: Vec<String>,
}

/// Entrée de catalogue (`{slug, name, lore}`, sans grille).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatureEntry {
    pub slug: String,
    pub name: String,
    pub lore: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_end_dto_roundtrip_kinds() {
        let chat: BusEnd = LinkEndDto::Chat { id: 2 }.into();
        assert_eq!(chat, BusEnd::Chat(2));
        let agent = LinkEndDto::from(BusEnd::Agent(7));
        assert_eq!(agent, LinkEndDto::Agent { id: 7 });
        let v = serde_json::to_value(LinkEndDto::Chat { id: 1 }).unwrap();
        assert_eq!(v["kind"], "chat");
        let back: LinkEndDto = serde_json::from_value(v).unwrap();
        assert_eq!(back, LinkEndDto::Chat { id: 1 });
    }

    #[test]
    fn send_ack_shapes() {
        let ok = SendAck::accepted();
        assert!(ok.accepted && ok.reason.is_none());
        let ko = SendAck::refused("no chat");
        assert!(!ko.accepted);
        assert_eq!(ko.reason.as_deref(), Some("no chat"));
    }
}
