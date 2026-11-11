//! Commandes mbox ops : doctor/metriques/piste/vague/fantome (§11c).
//!
//! Miroirs Tauri de `cmd_doctor` (docteur.rs), `cmd_metriques`
//! (metriques.rs), `cmd_piste` (piste.rs), `cmd_vague` (vague.rs) et
//! `cmd_fantome` (fantome.rs) : mêmes seuils, mêmes lectures, sorties
//! structurées. `vague run` bloque (threads ≤ 60 s+) : commande Tauri
//! synchrone, threadpool côté runtime (voir KEEP.md).

use crate::mbox::fantome::{cmd_fantome, racine_fantome};
use crate::mbox::fichier::{append_verrouille, epoque_fichier, hb_perime, lire_lignes, tail_n};
use crate::mbox::format::{epoque, est_message, est_suite, sanitize, BcastDir, SUJETS_BUS};
use crate::mbox::json::{metriques_json, piste_json};
use crate::mbox::vague::{cmd_vague, dossier_vague};
use crate::mbox_dto::{DetailDto, DoctorCheck, MetriquesDto, PisteSpanDto, VagueStateDto};

/// Seuil d'alerte inbox (miroir docteur.rs `INBOX_ALERTE_LIGNES`).
const INBOX_ALERTE_LIGNES: usize = 2000;
/// Lignes récentes contrôlées (miroir docteur.rs `DOCTOR_FORMAT_N`).
const DOCTOR_FORMAT_N: usize = 200;

// --- Impls pures ---

fn check(ok: bool, label: String, detail: String) -> DoctorCheck {
    DoctorCheck { ok, label, detail }
}

/// `mbox_doctor` : santé inbox + bus + ordres + hb + systemd (docteur.rs
/// `cmd_doctor`, mêmes contrôles, structurés au lieu d'imprimés).
pub fn doctor_impl(dir: &BcastDir) -> Vec<DoctorCheck> {
    let mut out = Vec::new();
    if !dir.racine().is_dir() {
        out.push(check(
            false,
            "dossier broadcast".to_string(),
            format!("dossier absent : {}", dir.racine().display()),
        ));
        return out;
    }
    let readonly = std::fs::metadata(dir.racine()).is_ok_and(|m| m.permissions().readonly());
    out.push(check(
        !readonly,
        "dossier inscriptible".to_string(),
        if readonly {
            "dossier non inscriptible".to_string()
        } else {
            "inscriptible".to_string()
        },
    ));
    doctor_inbox(dir, &mut out);
    doctor_canaux(dir, &mut out);
    out.push(check(
        true,
        "metriques".to_string(),
        crate::mbox::metriques::resume(dir),
    ));
    out.push(check(
        true,
        "piste".to_string(),
        crate::mbox::piste::resume(dir),
    ));
    match append_verrouille(&dir.tailles(), &[format!("{} doctor ok", epoque())]) {
        Ok(()) => out.push(check(
            true,
            "écriture verrouillée".to_string(),
            "ok".to_string(),
        )),
        Err(_) => out.push(check(
            false,
            "écriture verrouillée".to_string(),
            "écriture verrouillée impossible".to_string(),
        )),
    }
    doctor_systemd(dir, &mut out);
    out
}

/// Contrôle inbox : volume + format (docteur.rs:51-79 `verif_inbox`).
fn doctor_inbox(dir: &BcastDir, out: &mut Vec<DoctorCheck>) {
    let lignes = lire_lignes(&dir.boite());
    let octets = std::fs::metadata(dir.boite()).map(|m| m.len()).unwrap_or(0);
    out.push(check(
        true,
        "inbox".to_string(),
        format!("{} lignes {octets} octets", lignes.len()),
    ));
    out.push(check(
        lignes.len() <= INBOX_ALERTE_LIGNES,
        "inbox volume".to_string(),
        if lignes.len() > INBOX_ALERTE_LIGNES {
            "inbox > 2000 lignes sur 7j : chasser les spammeurs".to_string()
        } else {
            "sous le seuil 2000".to_string()
        },
    ));
    let valide = |l: &String| {
        l.is_empty()
            || est_suite(l)
            || (est_message(l)
                && l.split_once(" : ")
                    .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty()))
    };
    let mauvaises = tail_n(&dir.boite(), DOCTOR_FORMAT_N)
        .iter()
        .filter(|l| !valide(l))
        .count();
    out.push(check(
        mauvaises == 0,
        "format lignes".to_string(),
        if mauvaises > 0 {
            format!("{mauvaises} lignes hors format (200 dernières)")
        } else {
            "format des 200 dernières lignes".to_string()
        },
    ));
}

/// Contrôle bus + ordres + hb (docteur.rs:82-129 `verif_canaux`).
fn doctor_canaux(dir: &BcastDir, out: &mut Vec<DoctorCheck>) {
    let absents: Vec<&str> = SUJETS_BUS
        .iter()
        .filter(|s| !dir.bus(s).is_file())
        .copied()
        .collect();
    out.push(check(
        absents.is_empty(),
        "bus topics".to_string(),
        if absents.is_empty() {
            format!("{} topics", SUJETS_BUS.len())
        } else {
            format!("illisible : {}", absents.join(","))
        },
    ));
    if dir.ordres().is_file() {
        let n = lire_lignes(&dir.ordres())
            .iter()
            .filter(|l| !l.trim().is_empty())
            .count();
        out.push(check(true, "ordres".to_string(), format!("{n} ligne(s)")));
    } else {
        out.push(check(
            false,
            "ordres".to_string(),
            "ORDRES.log illisible".to_string(),
        ));
    }
    let Ok(entrees) = std::fs::read_dir(dir.joindre(".hb")) else {
        return;
    };
    let mut chemins: Vec<std::path::PathBuf> = entrees
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    chemins.sort();
    let now = epoque();
    let mut frais = 0;
    for chemin in chemins {
        let nom = chemin.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        if hb_perime(&chemin, now) {
            let age = epoque_fichier(&chemin)
                .map(|m| now.saturating_sub(m))
                .unwrap_or(0);
            out.push(check(
                false,
                format!("hb {nom}"),
                format!("heartbeat périmé ({age}s)"),
            ));
        } else {
            frais += 1;
        }
    }
    if frais > 0 {
        out.push(check(
            true,
            "hb frais".to_string(),
            format!("{frais} frais"),
        ));
    }
}

/// Contrôle systemd, dossier défaut seul (docteur.rs:132-150).
fn doctor_systemd(dir: &BcastDir, out: &mut Vec<DoctorCheck>) {
    if dir.boite() != BcastDir::defaut().boite() {
        return;
    }
    for svc in ["broadcast-watch.service", "agents-watch.service"] {
        match std::process::Command::new("systemctl")
            .args(["--user", "is-active", "-q", svc])
            .status()
        {
            Err(_) => break,
            Ok(st) if !st.success() => {
                out.push(check(false, svc.to_string(), "inactif".to_string()))
            }
            Ok(_) => {}
        }
    }
}

/// `mbox_metriques` : compteurs + intégrités (`metriques --json` parsé).
pub fn metriques_impl(dir: &BcastDir, agent: Option<&str>) -> Result<MetriquesDto, String> {
    serde_json::from_str(&metriques_json(dir, agent)).map_err(|_| "lecture impossible".to_string())
}

/// Normalise un filtre ordre (miroir piste.rs:139-147 `champ`, privé).
fn champ_miroir(texte: &str, n: usize) -> String {
    texte
        .replace(['|', '\n', '\r'], "_")
        .chars()
        .take(n)
        .collect::<String>()
        .trim()
        .to_string()
}

/// `mbox_piste` : spans filtrés (`piste --json` parsé, filtres normalisés
/// comme `cmd_piste` : ordre via `champ(_,80)`, agent via `sanitize`).
pub fn piste_impl(
    dir: &BcastDir,
    ordre: Option<&str>,
    agent: Option<&str>,
) -> Result<Vec<PisteSpanDto>, String> {
    let fo = ordre.map(|o| champ_miroir(o, 80));
    let fa = agent.map(sanitize);
    let brut = piste_json(dir, fo.as_deref(), fa.as_deref());
    serde_json::from_str(&brut).map_err(|_| "lecture impossible".to_string())
}

/// Compteurs file vague persistée (miroir lenient de `charge_persiste` +
/// `parse_tache`, vague.rs:118-189 : 5 champs → état compté, 3 → attente).
fn compte_vague(dir: &BcastDir) -> (usize, usize, usize, usize, usize, usize) {
    let (mut att, mut cours, mut ok, mut echec, mut bloq, mut exp) = (0, 0, 0, 0, 0, 0);
    for ligne in lire_lignes(&dossier_vague(dir).join("file.log")) {
        let champs: Vec<&str> = ligne.split('|').collect();
        match champs.len() {
            3 => att += 1,
            5 => match champs[2] {
                "attente" => att += 1,
                "cours" => cours += 1,
                "ok" => ok += 1,
                "echec" => echec += 1,
                "bloque" => bloq += 1,
                "expiree" => exp += 1,
                _ => {}
            },
            _ => {}
        }
    }
    (att, cours, ok, echec, bloq, exp)
}

/// `mbox_vague` : plan/run/lister/etat via `cmd_vague` (vague.rs:63-71).
/// `args` = argv d'appoint séparés par blancs (`--fichier F`… ; chemins à
/// espaces non supportés, comme le découpage CLI). `detail` = compteurs
/// seuls (jamais d'argv : `--token` possible, cf. `caviarder`).
pub fn vague_impl(dir: &BcastDir, op: &str, args: Option<&str>) -> Result<VagueStateDto, String> {
    if !matches!(op, "plan" | "run" | "lister" | "etat") {
        return Err("usage : vague plan|run|lister|etat".to_string());
    }
    let mut argv = vec![op.to_string()];
    if let Some(a) = args {
        argv.extend(a.split_whitespace().map(str::to_string));
    }
    let sortie = cmd_vague(dir, &argv);
    let (att, cours, ok, echec, bloq, exp) = compte_vague(dir);
    Ok(VagueStateDto {
        pending: att + bloq,
        running: cours,
        done: ok + echec + exp,
        detail: format!(
            "vague {op} : attente={att} cours={cours} ok={ok} echec={echec} \
             bloque={bloq} expiree={exp} (code {})",
            sortie.code()
        ),
    })
}

/// `mbox_fantome` : rejouer/diff/etat/purge via `cmd_fantome`
/// (fantome.rs:135-158, miroir seul, prod intacte).
pub fn fantome_impl(dir: &BcastDir, op: &str, cmd: Option<&str>) -> Result<DetailDto, String> {
    let argv: Vec<String> = match op {
        "rejouer" => match cmd {
            Some(c) if !c.trim().is_empty() => vec![op.to_string(), c.to_string()],
            _ => return Err("usage : fantome rejouer <corpus>".to_string()),
        },
        "diff" | "etat" | "purge" => vec![op.to_string()],
        _ => return Err("usage : fantome rejouer|diff|etat|purge".to_string()),
    };
    let sortie = cmd_fantome(dir, &argv);
    let ok = sortie == crate::mbox::format::Sortie::Ok;
    let detail = if op == "etat" {
        let n = std::fs::read_dir(racine_fantome(dir))
            .map(|e| e.flatten().count())
            .unwrap_or(0);
        format!("miroir : {n} entrée(s) (code {})", sortie.code())
    } else {
        format!("fantome {op} (code {})", sortie.code())
    };
    Ok(DetailDto { ok, detail })
}

// --- Args (commands.md §11c, camelCase) ---

// --- Wrappers Tauri (args plats du contrat, camelCase côté TS) ---

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_doctor() -> Result<Vec<DoctorCheck>, String> {
    Ok(doctor_impl(&BcastDir::defaut()))
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_metriques(agent: Option<String>) -> Result<MetriquesDto, String> {
    metriques_impl(&BcastDir::defaut(), agent.as_deref())
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_piste(
    ordre: Option<String>,
    agent: Option<String>,
) -> Result<Vec<PisteSpanDto>, String> {
    piste_impl(&BcastDir::defaut(), ordre.as_deref(), agent.as_deref())
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_vague(op: String, args: Option<String>) -> Result<VagueStateDto, String> {
    vague_impl(&BcastDir::defaut(), &op, args.as_deref())
}

#[tauri::command(rename_all = "camelCase")]
pub fn mbox_fantome(op: String, cmd: Option<String>) -> Result<DetailDto, String> {
    fantome_impl(&BcastDir::defaut(), &op, cmd.as_deref())
}
