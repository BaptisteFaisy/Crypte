//! Interop `--mbox` : mêmes fichiers, mêmes formats, deux faces.
//!
//! Contrat : le binaire TUI garde `--mbox <cmd...>` (main.rs →
//! `outils::lance_mbox` → `cli::exec`, KEEP) ; le desktop expose les 21
//! commandes typées (§11). Les deux écrivent/lisent les mêmes fichiers v2
//! (`BCAST_DIR`, bus 4 sujets, TTL 24 h, throttle dedup 30 min + 6/h,
//! codes 0/1/2/3). Ce module = le point d'appel CLI réutilisé par les
//! tests d'aller-retour (round-trip CLI ↔ commandes, preuve d'interop).

use std::path::Path;

/// Version du protocole broadcast (miroir cli.rs `VERSION`, bcast:16).
pub const MBOX_VERSION: &str = "2.0.0";

/// Exécute une commande CLI mbox sur `dir`, rend le code shell 0/1/2/3
/// (`cli::exec`, cli.rs:58-94). Imprime sur stdout comme le binaire.
pub fn run_cli(dir: &Path, argv: &[&str]) -> i32 {
    let args: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    crate::mbox::cli::exec(&args, dir)
}
