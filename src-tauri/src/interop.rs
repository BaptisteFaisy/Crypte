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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd_mbox_core::{send_impl, tail_impl, SendArgs};
    use crate::mbox::format::BcastDir;

    /// CLI shell et commandes typées partagent les mêmes fichiers v2 :
    /// `send` shell relu par `mbox_tail`, `mbox_send` relu au fichier.
    #[test]
    fn cli_et_commandes_aller_retour() {
        let dir = std::env::temp_dir().join(format!("crypte-interop-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let bcast = BcastDir::depuis(dir.clone());
        bcast.init().unwrap();
        assert_eq!(
            run_cli(&dir, &["send", "--from", "cli", "--", "bonjour-cli"]),
            0
        );
        let out = tail_impl(&bcast, Some(5), None, false).unwrap();
        assert!(out.messages.iter().any(|m| m.text.contains("bonjour-cli")));
        let args = SendArgs {
            message: String::from("bonjour-cmd"),
            priority: None,
            from: Some(String::from("cmd")),
            dests: None,
            state: None,
            force: None,
            token: None,
        };
        send_impl(&bcast, &args).unwrap();
        let raw = std::fs::read_to_string(bcast.boite()).unwrap();
        assert!(
            raw.contains("bonjour-cli") && raw.contains("bonjour-cmd"),
            "{raw}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
