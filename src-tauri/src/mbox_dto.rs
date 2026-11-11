//! DTO partagés mbox — miroirs de `contracts.ts` (dossier plan).
//!
//! Règles : aucun secret ici (sauf `JetonCreated.secret`, montré 1x) ;
//! désérialisation depuis les formes stables `mbox/json.rs`
//! (clés snake), sérialisation vers le frontend en camelCase
//! (`rename` + `alias`, jamais de double struct).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

// --- Mailbox v2 (contrats §11, formes json.rs) ---

/// Message vu 5 clés (`tail --json`, json.rs `rend_message`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MboxMessage {
    pub date: String,
    pub expediteur: String,
    pub priorite: String,
    pub dest: String,
    pub corps: String,
}

/// `tail --all` : inbox + 4 sujets (json.rs `tail_json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MboxTailAll {
    pub inbox: Vec<MboxMessage>,
    pub bus: BTreeMap<String, Vec<MboxMessage>>,
}

/// Sortie `mbox_tail` : liste OU objet all (contracts.ts union).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MboxTailOut {
    List(Vec<MboxMessage>),
    All(MboxTailAll),
}

/// Résultat d'envoi (`sent`/`throttled`/`same-state`, cli.rs `cmd_send`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MboxSendResult {
    pub status: String,
    pub detail: String,
}

/// Ordre 10 clés stables (json.rs `rend_ordre`, contracts.ts `Ordre`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrdreDto {
    pub id: String,
    pub to: String,
    pub ttl: u64,
    pub statut: String,
    pub src: String,
    pub texte: String,
    pub prio: Option<String>,
    pub supersede: Option<String>,
    pub hash: Option<String>,
    pub sig: Option<String>,
}

/// Verdict d'ACK (`ack.rs`, `revoque` réservé système : refusé ici).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerdictDto {
    Lu,
    Applique,
    Refuse,
}

/// Verdict DTO → texte journal (`ordre ack`, cli_gestion.rs).
pub fn verdict_texte(v: VerdictDto) -> &'static str {
    match v {
        VerdictDto::Lu => "lu",
        VerdictDto::Applique => "applique",
        VerdictDto::Refuse => "refuse",
    }
}

/// Entrée heartbeat (`hb list`, cli.rs `liste_hb` + `frais` < 900 s).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HbEntry {
    pub nom: String,
    pub etat: String,
    pub epoque: u64,
    pub frais: bool,
}

/// Un contrôle `doctor` (docteur.rs, structuré au lieu d'imprimé).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub ok: bool,
    pub label: String,
    pub detail: String,
}

/// Compteurs + intégrité (`metriques --json`, json.rs `rend_metriques`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetriquesDto {
    pub compteurs: BTreeMap<String, u64>,
    pub agents: BTreeMap<String, BTreeMap<String, u64>>,
    pub integrite: IntegriteDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegriteDto {
    pub ordres: TripletDto,
    pub signatures: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TripletDto {
    pub ok: usize,
    pub ko: usize,
    pub sans: usize,
}

/// Span de piste (`piste --json`, json.rs `rend_span`).
/// `rename` (émission camel) + `alias` (lecture snake stable).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PisteSpanDto {
    #[serde(rename = "runId", alias = "run_id")]
    pub run_id: String,
    pub epoque: u64,
    pub agent: String,
    pub action: String,
    #[serde(rename = "ordreId", alias = "ordre_id")]
    pub ordre_id: String,
    pub verdict: String,
}

/// Jeton créé : secret montré 1x (jeton.rs, jamais loggué/stocké).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JetonCreated {
    pub nom: String,
    pub secret: String,
}

/// Fiche jeton sans secret (autorite.rs `lister`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JetonInfo {
    pub nom: String,
    pub revoque: bool,
}

/// État vague (vague.rs `cmd_etat`, compteurs file persistée).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VagueStateDto {
    pub pending: usize,
    pub running: usize,
    pub done: usize,
    pub detail: String,
}

// --- Compteurs génériques (contrats §11) ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompteDto {
    pub digests: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OkDto {
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdDto {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurgesDto {
    pub purged: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetailDto {
    pub ok: bool,
    pub detail: String,
}
