//! Commandes mbox ordres + jetons (§11b).
//!
//! Miroirs Tauri de `cmd_ordre` (new/list/revoque/ack/pull, cli_gestion.rs),
//! `cmd_deleguer` (delegation.rs) et `cmd_jeton` (jeton.rs) : mêmes portes
//! à jeton, mêmes journaux, retours structurés. Écritures sensibles
//! exigent `--token` (porte + garde anti-secret, jamais de secret sorti).
//!
//! Écarts assumés (voir KEEP.md) : `prio`/`supersede` voyagent en suffixes
//! `cle=valeur` (Extensions, ordres.rs) ; `ack` prend l'expéditeur = nom du
//! porteur du jeton ; `deleguer` exige `sourceId` + `token` (trou contrat).

use std::path::Path;

use serde::Deserialize;

use crate::mbox::autorite::dossier_defaut;
use crate::mbox::autorite::{
    emettre, lister, purger, revoquer as revoquer_jeton, verifier, Statut,
};
use crate::mbox::delegation::{analyser_demande, executer, DemandeDelegation};
use crate::mbox::format::{BcastDir, Sortie, TTL_DEFAUT_HEURES};
use crate::mbox::journal::{ack, creer, revoquer};
use crate::mbox::json::{liste_ordres_json, pull_json};
use crate::mbox::metriques::{incremente, incremente_agent};
use crate::mbox::ordres::VerifieJeton;
use crate::mbox::outils::porte_jeton;
use crate::mbox::piste::{emet_jeton, Action};
use crate::mbox_dto::{
    verdict_texte, IdDto, JetonCreated, JetonInfo, OrdreDto, PurgesDto, VerdictDto,
};

// --- Impls pures ---

/// Suffixe `prio=`/`supersede=` validé (Extensions, ordres.rs `suffixe`).
fn suffixe_extensions(prio: Option<&str>, supersede: Option<&str>) -> Result<String, String> {
    let mut bouts = Vec::new();
    if let Some(p) = prio {
        if !matches!(p, "P0" | "P1" | "P2") {
            return Err("priorité P0|P1|P2".to_string());
        }
        bouts.push(format!("prio={p}"));
    }
    if let Some(s) = supersede.filter(|s| !s.trim().is_empty()) {
        if s.contains('|') || s.split_whitespace().count() != 1 || s.contains('=') {
            return Err("supersede invalide (un jeton, sans | ni =)".to_string());
        }
        bouts.push(format!("supersede={}", s.trim()));
    }
    Ok(bouts.join(" "))
}

/// `mbox_ordre_new` (cli_gestion.rs:212-248 `ordre_nouveau`).
pub fn ordre_new_impl(dir: &BcastDir, args: &OrdreNewArgs) -> Result<IdDto, String> {
    if args.text.trim().is_empty() {
        return Err("texte d'ordre vide".to_string());
    }
    let ttl = args.ttl.unwrap_or(TTL_DEFAUT_HEURES);
    let suffixe = suffixe_extensions(args.prio.as_deref(), args.supersede.as_deref())?;
    let texte = if suffixe.is_empty() {
        args.text.clone()
    } else {
        format!("{} {suffixe}", args.text)
    };
    let token = args
        .token
        .as_deref()
        .ok_or_else(|| "jeton requis (--token \"nom:secret\")".to_string())?;
    if !porte_jeton(&texte, Some(token)) {
        return Err("jeton refusé ou texte sensible (garde anti-secret)".to_string());
    }
    let _ = dir.init();
    let secret = token.rsplit_once(':').map(|(_, s)| s);
    match creer(
        dir,
        args.to.as_deref().unwrap_or("ALL"),
        ttl,
        "user-direct",
        &texte,
        secret,
    ) {
        Ok(id) => {
            incremente(dir, "ordre_new");
            emet_jeton(dir, token, Action::OrdreCree, &id, "ok");
            Ok(IdDto { id })
        }
        Err(Sortie::Usage) => Err("ordre invalide (dest/ttl/texte)".to_string()),
        Err(_) => Err("écriture impossible".to_string()),
    }
}

/// `mbox_ordre_list` : sélection `lister`, forme stable parsée.
pub fn ordre_list_impl(dir: &BcastDir, all: bool) -> Result<Vec<OrdreDto>, String> {
    serde_json::from_str(&liste_ordres_json(dir, all)).map_err(|_| "lecture impossible".to_string())
}

/// `mbox_ordre_revoke` (cli_gestion.rs:276-297, motif vide).
pub fn ordre_revoke_impl(dir: &BcastDir, id: &str, token: Option<&str>) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("usage : ordre revoque <id>".to_string());
    }
    let token = token.ok_or_else(|| "jeton requis (--token \"nom:secret\")".to_string())?;
    if !porte_jeton(id, Some(token)) {
        return Err("jeton refusé ou texte sensible (garde anti-secret)".to_string());
    }
    match revoquer(dir, id, "", token) {
        Ok(()) => {
            incremente(dir, "ordre_revoque");
            emet_jeton(dir, token, Action::OrdreRevoque, id, "revoque");
            Ok(())
        }
        Err(_) => Err("révocation impossible (ordre inactif ou jeton refusé)".to_string()),
    }
}

/// `mbox_ordre_ack` (cli_gestion.rs:302-335 ; expéditeur = porteur jeton).
pub fn ordre_ack_impl(
    dir: &BcastDir,
    id: &str,
    verdict: VerdictDto,
    motif: Option<&str>,
    token: Option<&str>,
) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("usage : ordre ack <id> <verdict>".to_string());
    }
    let token = token.ok_or_else(|| "jeton requis (--token \"nom:secret\")".to_string())?;
    let (nom, _) = token
        .rsplit_once(':')
        .filter(|(n, s)| !n.is_empty() && !s.is_empty())
        .ok_or_else(|| "jeton invalide (forme \"nom:secret\" attendue)".to_string())?;
    let texte_verdict = verdict_texte(verdict);
    let motif = motif.unwrap_or("");
    if !porte_jeton(&format!("{id} {texte_verdict} {motif}"), Some(token)) {
        return Err("jeton refusé ou texte sensible (garde anti-secret)".to_string());
    }
    match ack(dir, id, nom, texte_verdict, motif, token) {
        Ok(()) => {
            incremente(dir, "ack_ok");
            incremente_agent(dir, nom, "ack_ok");
            emet_jeton(dir, token, Action::OrdreAck, id, texte_verdict);
            Ok(())
        }
        Err(Sortie::Usage) => Err("ack refusé : motif requis pour refuse".to_string()),
        Err(_) => Err("ack refusé (ordre inactif ou signature invalide)".to_string()),
    }
}

/// `mbox_ordre_pull` : file agent, forme stable parsée.
pub fn ordre_pull_impl(dir: &BcastDir, agent: &str) -> Result<Vec<OrdreDto>, String> {
    if agent.trim().is_empty() {
        return Err("usage : ordre pull <agent>".to_string());
    }
    serde_json::from_str(&pull_json(dir, agent)).map_err(|_| "lecture impossible".to_string())
}

/// Porte jetons sur dossier injectable (miroir de `PorteDelegation`,
/// delegation.rs:218-229, qui vise le dossier défaut).
struct AuthDossier<'a> {
    dossier: &'a Path,
}

impl VerifieJeton for AuthDossier<'_> {
    fn verifier_jeton(&self, jeton: &str) -> Result<String, String> {
        let (nom, secret) = jeton
            .rsplit_once(':')
            .filter(|(n, s)| !n.is_empty() && !s.is_empty())
            .ok_or_else(|| "jeton invalide (forme \"nom:secret\" attendue)".to_string())?;
        verifier(self.dossier, nom, secret)
            .map(|fiche| fiche.nom)
            .map_err(|e| format!("bcast: {e:#}"))
    }
}

/// `mbox_ordre_delegate` (delegation.rs `cmd_deleguer` + `executer`).
/// `source` + `token` requis au contrat (G1 tranché : trouver_ordre +
/// porte l'exigent, jamais inventés).
pub fn ordre_delegate_impl(
    dir: &BcastDir,
    auth_dir: &Path,
    to: &str,
    resume: &str,
    pieces: &[String],
    source: &str,
    token: &str,
) -> Result<IdDto, String> {
    let id = (!source.trim().is_empty())
        .then_some(source)
        .ok_or_else(|| "délégation : ordre source requis".to_string())?;
    let demande: DemandeDelegation = analyser_demande(id, to, resume, pieces)?;
    let garde = format!(
        "{} {} {}",
        demande.id_source, demande.destinataire, demande.resume
    );
    if !porte_jeton(&garde, Some(token)) {
        return Err("jeton refusé ou texte sensible (garde anti-secret)".to_string());
    }
    let _ = dir.init();
    match executer(dir, &AuthDossier { dossier: auth_dir }, token, &demande) {
        Ok(id) => {
            incremente(dir, "ordre_deleguer");
            incremente_agent(dir, &demande.destinataire, "ordre_deleguer");
            Ok(IdDto { id })
        }
        Err(e) => Err(e),
    }
}

/// `mbox_jeton_new` (jeton.rs:37-64, secret montré 1x, jamais loggué).
pub fn jeton_new_impl(
    auth_dir: &Path,
    name: &str,
    ttl_hours: Option<u64>,
) -> Result<JetonCreated, String> {
    let ttl = ttl_hours.map(|h| h.to_string());
    match emettre(auth_dir, name, ttl.as_deref()) {
        Ok(emis) => Ok(JetonCreated {
            nom: emis.nom,
            secret: emis.secret,
        }),
        Err(e) => {
            let texte = format!("{e:#}");
            if texte.contains("existe déjà") {
                Err(format!("un jeton actif existe déjà pour « {name} »"))
            } else {
                Err("émission impossible".to_string())
            }
        }
    }
}

/// `mbox_jeton_revoke` (jeton.rs:67-81).
pub fn jeton_revoke_impl(auth_dir: &Path, name: &str) -> Result<(), String> {
    revoquer_jeton(auth_dir, name)
        .map_err(|_| "révocation impossible (nom inconnu ou déjà révoqué)".to_string())
}

/// `mbox_jeton_list` : actifs + révoqués, SANS secret (autorite `lister`).
/// Expirés exclus (ni actifs ni révoqués, voir KEEP.md).
pub fn jeton_list_impl(auth_dir: &Path) -> Result<Vec<JetonInfo>, String> {
    let fiches =
        lister(auth_dir, true).map_err(|_| "autorité des jetons indisponible".to_string())?;
    Ok(fiches
        .into_iter()
        .filter(|f| f.statut != Statut::Expire)
        .map(|f| JetonInfo {
            nom: f.nom,
            revoque: f.statut == Statut::Revoque,
        })
        .collect())
}

/// `mbox_jeton_purge` (jeton.rs:111-122).
pub fn jeton_purge_impl(auth_dir: &Path) -> Result<PurgesDto, String> {
    purger(auth_dir)
        .map(|n| PurgesDto { purged: n })
        .map_err(|_| "purge impossible".to_string())
}

// --- Args (commands.md §11b, camelCase) ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrdreNewArgs {
    pub to: Option<String>,
    pub text: String,
    pub ttl: Option<u64>,
    pub prio: Option<String>,
    pub supersede: Option<String>,
    pub token: Option<String>,
}

// --- Wrappers Tauri (args plats du contrat, camelCase côté TS) ---

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_ordre_new(
    to: Option<String>,
    text: String,
    ttl: Option<u64>,
    prio: Option<String>,
    supersede: Option<String>,
    token: Option<String>,
) -> Result<IdDto, String> {
    let args = OrdreNewArgs {
        to,
        text,
        ttl,
        prio,
        supersede,
        token,
    };
    ordre_new_impl(&BcastDir::defaut(), &args)
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_ordre_list(all: Option<bool>) -> Result<Vec<OrdreDto>, String> {
    ordre_list_impl(&BcastDir::defaut(), all.unwrap_or(false))
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_ordre_revoke(id: String, token: String) -> Result<(), String> {
    ordre_revoke_impl(&BcastDir::defaut(), &id, Some(&token))
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_ordre_ack(
    id: String,
    verdict: VerdictDto,
    motif: Option<String>,
    token: String,
) -> Result<(), String> {
    ordre_ack_impl(
        &BcastDir::defaut(),
        &id,
        verdict,
        motif.as_deref(),
        Some(&token),
    )
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_ordre_pull(agent: String) -> Result<Vec<OrdreDto>, String> {
    ordre_pull_impl(&BcastDir::defaut(), &agent)
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_ordre_delegate(
    to: String,
    resume: String,
    pieces: Option<Vec<String>>,
    source: String,
    token: String,
) -> Result<IdDto, String> {
    let auth = dossier_defaut().map_err(|_| "autorité des jetons indisponible".to_string())?;
    ordre_delegate_impl(
        &BcastDir::defaut(),
        &auth,
        &to,
        &resume,
        &pieces.unwrap_or_default(),
        &source,
        &token,
    )
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_jeton_new(name: String, ttl_hours: Option<u64>) -> Result<JetonCreated, String> {
    let auth = dossier_defaut().map_err(|_| "autorité des jetons indisponible".to_string())?;
    jeton_new_impl(&auth, &name, ttl_hours)
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_jeton_revoke(name: String) -> Result<(), String> {
    let auth = dossier_defaut().map_err(|_| "autorité des jetons indisponible".to_string())?;
    jeton_revoke_impl(&auth, &name)
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_jeton_list() -> Result<Vec<JetonInfo>, String> {
    let auth = dossier_defaut().map_err(|_| "autorité des jetons indisponible".to_string())?;
    jeton_list_impl(&auth)
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_jeton_purge() -> Result<PurgesDto, String> {
    let auth = dossier_defaut().map_err(|_| "autorité des jetons indisponible".to_string())?;
    jeton_purge_impl(&auth)
}
