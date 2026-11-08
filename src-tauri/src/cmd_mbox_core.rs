//! Commandes mbox cœur : tail/send/bus/digest/rotate/hb (§11a).
//!
//! Miroirs Tauri de `cmd_tail`/`cmd_send`/`cmd_bus`/`cmd_hb` (cli.rs),
//! `cmd_rotate` (cli_gestion.rs) et `digest_flush` (digest.rs) : mêmes
//! gardes, mêmes formats disque, retours structurés au lieu d'imprimés.
//! Le dossier est `BcastDir::defaut()` (`BCAST_DIR` honoré).

use serde::Deserialize;

use crate::mbox::digest::digest_flush;
use crate::mbox::epoque;
use crate::mbox::fichier::{append_verrouille, lire_hb, SEUIL_HB_SECS};
use crate::mbox::format::{
    construire_ligne, decouper_corps, BcastDir, Priorite, Sortie, SUJETS_BUS,
};
use crate::mbox::json::tail_json;
use crate::mbox::metriques::{incremente, incremente_agent, incremente_motif};
use crate::mbox::outils::{expediteur, porte_jeton};
use crate::mbox::throttle::{etat_change, garde_bus, garde_inbox};
use crate::mbox_dto::{CompteDto, MboxSendResult, MboxTailOut, OkDto};

// --- Impls pures (testées sur dossiers temporaires) ---

/// Ligne v2 + continuations (cli.rs:109-115 `enveloppe`, byte-identique).
fn enveloppe(exp: &str, pri: Priorite, dest: &str, corps: &str) -> Vec<String> {
    let lignes = decouper_corps(corps);
    let premiere = lignes.first().cloned().unwrap_or_default();
    let mut sortie = vec![construire_ligne(exp, pri, dest, &premiere)];
    sortie.extend(lignes.into_iter().skip(1));
    sortie
}

/// `-p` (défaut P1, cli.rs:128-135 `resout_priorite`).
fn resout_priorite(brut: Option<&str>) -> Result<Priorite, String> {
    match brut.unwrap_or("P1") {
        "P0" => Ok(Priorite::P0),
        "P1" => Ok(Priorite::P1),
        "P2" => Ok(Priorite::P2),
        _ => Err("priorité P0|P1|P2".to_string()),
    }
}

/// `mbox_tail` (cli.rs:251-289 + `tail_json`, formes stables parsées).
pub fn tail_impl(
    dir: &BcastDir,
    n: Option<u32>,
    bus: Option<&str>,
    all: bool,
) -> Result<MboxTailOut, String> {
    let n = n.unwrap_or(20) as usize;
    if let Some(sujet) = bus {
        if !SUJETS_BUS.contains(&sujet) {
            return Err(format!("topic : {}", SUJETS_BUS.join(" ")));
        }
    }
    let brut =
        tail_json(dir, n, bus, all).ok_or_else(|| format!("topic : {}", SUJETS_BUS.join(" ")))?;
    serde_json::from_str(&brut).map_err(|_| "lecture impossible".to_string())
}

/// `--state k=v` (cli.rs:153-163 `passe_etat`) : `Ok(true)` = dedup
/// sauté, `Ok(false)` = sans état, `Err` = same-state déjà compté.
fn passe_etat(dir: &BcastDir, exp: &str, brut: Option<&str>) -> Result<bool, MboxSendResult> {
    let Some(s) = brut else { return Ok(false) };
    match s.split_once('=') {
        None => Err(MboxSendResult {
            status: "usage".to_string(),
            detail: "--state attend cle=valeur".to_string(),
        }),
        Some((cle, valeur)) if etat_change(dir, exp, cle, valeur) => Ok(true),
        Some(_) => {
            incremente(dir, "send_same");
            Err(MboxSendResult {
                status: "same-state".to_string(),
                detail: "same-state".to_string(),
            })
        }
    }
}

/// `mbox_send` (cli.rs:166-212 `cmd_send`, même ordre de gardes).
pub fn send_impl(dir: &BcastDir, args: &SendArgs) -> Result<MboxSendResult, String> {
    if args.message.trim().is_empty() {
        return Err("message vide".to_string());
    }
    let pri = resout_priorite(args.priority.as_deref())?;
    let exp = expediteur(args.from.as_deref())
        .ok_or_else(|| "identité requise (--from ou BCAST_SENDER)".to_string())?;
    let corps = args.message.clone();
    let force = args.force.unwrap_or(false);
    if force && !porte_jeton(&corps, args.token.as_deref()) {
        return Err("jeton requis ou invalide (--token \"nom:secret\")".to_string());
    }
    let mut dest = args.dests.clone().unwrap_or_default().join(" ");
    if force {
        dest = format!("{dest} [bypass-throttle]").trim_start().to_string();
    }
    let _ = dir.init();
    let saute_dedup = match passe_etat(dir, &exp, args.state.as_deref()) {
        Ok(saute) => saute,
        Err(same) if same.status == "same-state" => return Ok(same),
        Err(usage) => return Err(usage.detail),
    };
    if !force {
        if let Err(motif) = garde_inbox(dir, &exp, &corps, saute_dedup) {
            incremente_motif(dir, motif.verdict());
            return Ok(MboxSendResult {
                status: "throttled".to_string(),
                detail: motif.verdict().to_string(),
            });
        }
    }
    match append_verrouille(&dir.boite(), &enveloppe(&exp, pri, &dest, &corps)) {
        Ok(()) => {
            incremente(dir, "send_ok");
            incremente_agent(dir, &exp, "send_ok");
            Ok(MboxSendResult {
                status: "sent".to_string(),
                detail: "sent".to_string(),
            })
        }
        Err(_) => Err("écriture impossible".to_string()),
    }
}

/// `mbox_bus` (cli.rs:215-247 `cmd_bus`, P2 + `@sujet`).
pub fn bus_impl(
    dir: &BcastDir,
    topic: &str,
    message: &str,
    from: Option<&str>,
) -> Result<MboxSendResult, String> {
    if !SUJETS_BUS.contains(&topic) {
        return Err(format!("topic : {}", SUJETS_BUS.join(" ")));
    }
    if message.trim().is_empty() {
        return Err("message vide".to_string());
    }
    let exp =
        expediteur(from).ok_or_else(|| "identité requise (--from ou BCAST_SENDER)".to_string())?;
    let _ = dir.init();
    if !garde_bus(dir, &exp, topic, message) {
        incremente(dir, "bus_dup");
        return Ok(MboxSendResult {
            status: "throttled".to_string(),
            detail: "dup".to_string(),
        });
    }
    let dest = format!("@{topic}");
    match append_verrouille(
        &dir.bus(topic),
        &enveloppe(&exp, Priorite::P2, &dest, message),
    ) {
        Ok(()) => {
            incremente(dir, "bus_ok");
            incremente_agent(dir, &exp, "bus_ok");
            Ok(MboxSendResult {
                status: "sent".to_string(),
                detail: "sent".to_string(),
            })
        }
        Err(_) => Err("écriture impossible".to_string()),
    }
}

/// `mbox_digest` : flush pending → lignes P2 (cli.rs:69-73).
pub fn digest_impl(dir: &BcastDir) -> CompteDto {
    let _ = dir.init();
    CompteDto {
        digests: digest_flush(dir),
    }
}

/// `mbox_rotate` : rotation via `cmd_rotate` (cli_gestion.rs:55-85).
/// Imprime `rotated…` sur stdout backend, comme le CLI (interop `.sizes`).
pub fn rotate_impl(dir: &BcastDir, keep_days: Option<u64>) -> OkDto {
    let mut argv = Vec::new();
    if let Some(k) = keep_days {
        argv.push("--keep-days".to_string());
        argv.push(k.to_string());
    }
    let sortie = crate::mbox::cli_gestion::cmd_rotate(dir, &argv);
    OkDto {
        ok: sortie == Sortie::Ok,
    }
}

/// `mbox_hb` : écriture directe sans verrou (cli.rs:334-347 `cmd_hb`).
pub fn hb_impl(dir: &BcastDir, name: &str, state: &str) -> Result<OkDto, String> {
    if state.trim().is_empty() {
        return Err("usage : hb <nom> <état>".to_string());
    }
    let _ = dir.init();
    let contenu = format!("{} {} {}\n", epoque(), crate::mbox::maintenant(), state);
    match std::fs::write(dir.hb(name), contenu) {
        Ok(()) => {
            incremente(dir, "hb_ok");
            Ok(OkDto { ok: true })
        }
        Err(_) => Ok(OkDto { ok: false }),
    }
}

/// `mbox_hb_list` (cli.rs:293-320 `liste_hb`, structuré + `frais`).
pub fn hb_list_impl(dir: &BcastDir) -> Vec<crate::mbox_dto::HbEntry> {
    let now = epoque();
    let Ok(entrees) = std::fs::read_dir(dir.joindre(".hb")) else {
        return Vec::new();
    };
    let mut chemins: Vec<std::path::PathBuf> = entrees
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    chemins.sort();
    let mut out = Vec::new();
    for chemin in chemins {
        let mtime = std::fs::metadata(&chemin)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        let Some(m) = mtime.filter(|m| *m <= now) else {
            continue;
        };
        let nom = chemin
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        out.push(dto_hb_entry(
            &nom,
            m,
            now,
            lire_hb(&chemin).unwrap_or_default(),
        ));
    }
    out
}

/// Construit `HbEntry` depuis mtime (`frais` = âge ≤ 900 s).
fn dto_hb_entry(nom: &str, mtime: u64, now: u64, etat: String) -> crate::mbox_dto::HbEntry {
    crate::mbox_dto::HbEntry {
        nom: nom.to_string(),
        etat,
        epoque: mtime,
        frais: now.saturating_sub(mtime) <= SEUIL_HB_SECS,
    }
}

// --- Args (commands.md §11a, camelCase) ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendArgs {
    pub message: String,
    pub priority: Option<String>,
    pub from: Option<String>,
    pub dests: Option<Vec<String>>,
    pub state: Option<String>,
    pub force: Option<bool>,
    pub token: Option<String>,
}

// --- Wrappers Tauri (args plats du contrat, camelCase côté TS) ---

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_tail(
    n: Option<u32>,
    bus: Option<String>,
    all: Option<bool>,
) -> Result<MboxTailOut, String> {
    tail_impl(&BcastDir::defaut(), n, bus.as_deref(), all.unwrap_or(false))
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_send(
    message: String,
    priority: Option<String>,
    from: Option<String>,
    dests: Option<Vec<String>>,
    state: Option<String>,
    force: Option<bool>,
    token: Option<String>,
) -> Result<MboxSendResult, String> {
    let args = SendArgs {
        message,
        priority,
        from,
        dests,
        state,
        force,
        token,
    };
    send_impl(&BcastDir::defaut(), &args)
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_bus(
    topic: String,
    message: String,
    from: Option<String>,
) -> Result<MboxSendResult, String> {
    bus_impl(&BcastDir::defaut(), &topic, &message, from.as_deref())
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_digest() -> Result<CompteDto, String> {
    Ok(digest_impl(&BcastDir::defaut()))
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_rotate(keep_days: Option<u64>) -> Result<OkDto, String> {
    Ok(rotate_impl(&BcastDir::defaut(), keep_days))
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_hb(name: String, state: String) -> Result<OkDto, String> {
    hb_impl(&BcastDir::defaut(), &name, &state)
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_hb_list() -> Result<Vec<crate::mbox_dto::HbEntry>, String> {
    Ok(hb_list_impl(&BcastDir::defaut()))
}
