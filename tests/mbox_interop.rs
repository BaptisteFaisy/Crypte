//! Interopérabilité Rust <-> shell broadcast v2 (std seul, convention du lot).
//!
//! Pilote le binaire (`--mbox`) et le `bcast` shell sur un `BCAST_DIR`
//! jetable : roundtrip dans les deux sens, bus + dedup, cycle jetons et
//! ordres bout en bout, codes de sortie. L'autorité des jetons est
//! confinée par `XDG_DATA_HOME` jetable (`Store::default_dir` le lit
//! en premier) : aucun test n'écrit hors de son bac.

use std::path::{Path, PathBuf};
use std::process::Command;

/// `bcast` shell de référence (interop lecture/écriture à l'identique).
const BCAST_SHELL: &str = "/home/baptiste/Bureau/.scratch/broadcast/bin/bcast";

/// Binaire sous test (construit par cargo avant les tests d'intégration).
fn binaire() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kiro-horror-tui"))
}

/// Bac jetable `(broadcast, xdg)` unique par test (jamais le vrai `.scratch`).
fn bac(tag: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("mbox-interop-{}-{tag}", std::process::id()));
    let bcast = base.join("bcast");
    let home = base.join("xdg");
    std::fs::create_dir_all(&bcast).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    (bcast, home)
}

/// Nettoyage best-effort du bac.
fn nettoie(bcast: &Path) {
    if let Some(base) = bcast.parent() {
        let _ = std::fs::remove_dir_all(base);
    }
}

/// Exécute `crypte --mbox …` confiné (code, stdout, stderr).
fn rust(bcast: &Path, home: &Path, args: &[&str]) -> (i32, String, String) {
    let argv: Vec<String> = std::iter::once("--mbox".to_string())
        .chain(args.iter().map(|s| s.to_string()))
        .collect();
    let sortie = Command::new(binaire())
        .args(&argv)
        .env("BCAST_DIR", bcast)
        .env("XDG_DATA_HOME", home)
        .output()
        .expect("binaire --mbox exécutable");
    (
        sortie.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&sortie.stdout).into_owned(),
        String::from_utf8_lossy(&sortie.stderr).into_owned(),
    )
}

/// Exécute le `bcast` shell de référence (code, stdout, stderr).
fn shell(bcast: &Path, args: &[&str]) -> (i32, String, String) {
    assert!(
        Path::new(BCAST_SHELL).is_file(),
        "shell {BCAST_SHELL} absent"
    );
    let sortie = Command::new(BCAST_SHELL)
        .args(args)
        .env("BCAST_DIR", bcast)
        .output()
        .expect("shell bcast exécutable");
    (
        sortie.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&sortie.stdout).into_owned(),
        String::from_utf8_lossy(&sortie.stderr).into_owned(),
    )
}

#[test]
fn roundtrip_rust_shell_inbox() {
    let (bcast, home) = bac("inbox");
    let (code, out, _) = rust(
        &bcast,
        &home,
        &["send", "--from", "t-rust", "--", "bonjour-shell"],
    );
    assert_eq!((code, out.trim()), (0, "sent"));
    let (code, out, _) = shell(&bcast, &["tail", "-n", "5"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("bonjour-shell") && out.contains("t-rust"),
        "shell voit Rust : {out}"
    );
    let (code, out, _) = shell(&bcast, &["send", "--from", "t-shell", "--", "bonjour-rust"]);
    assert_eq!((code, out.trim()), (0, "sent"));
    let (code, out, _) = rust(&bcast, &home, &["tail", "-n", "5"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("bonjour-rust") && out.contains("t-shell"),
        "Rust voit shell : {out}"
    );
    nettoie(&bcast);
}

#[test]
fn bus_et_dedup_partages() {
    let (bcast, home) = bac("bus");
    let (code, out, _) = rust(
        &bcast,
        &home,
        &["bus", "-t", "gpu", "--from", "t-r", "--", "chaud"],
    );
    assert_eq!((code, out.trim()), (0, "sent"));
    let (code, out, _) = rust(
        &bcast,
        &home,
        &["bus", "-t", "gpu", "--from", "t-r", "--", "chaud"],
    );
    assert_eq!((code, out.trim()), (0, "throttled:dup"));
    let (code, out, _) = shell(&bcast, &["tail", "--bus", "gpu"]);
    assert_eq!(code, 0);
    assert_eq!(
        out.lines().filter(|l| l.contains("chaud")).count(),
        1,
        "une seule ligne : {out}"
    );
    nettoie(&bcast);
}

#[test]
fn jetons_et_ordres_bout_en_bout() {
    let (bcast, home) = bac("ordres");
    let (code, out, _) = rust(&bcast, &home, &["jeton", "new", "agent-i"]);
    assert_eq!(code, 0);
    let secret = out
        .trim()
        .strip_prefix("jeton=agent-i secret=")
        .expect("émission : {out}")
        .to_string();
    assert_eq!(secret.len(), 64);
    let jeton = format!("agent-i:{secret}");
    let (code, out, _) = rust(
        &bcast,
        &home,
        &["ordre", "new", "--token", &jeton, "--", "relire le plan"],
    );
    assert_eq!(code, 0);
    let id = out
        .trim()
        .strip_prefix("ordre=")
        .expect("création : {out}")
        .to_string();
    assert!(id.starts_with("O-"), "identifiant v2 : {id}");
    let (code, out, _) = shell(&bcast, &["ordre", "list"]);
    assert_eq!(code, 1, "quirk shell : liste non vide sort 1");
    assert!(
        out.contains(&id) && out.contains("relire le plan"),
        "shell liste : {out}"
    );
    let (code, out, _) = rust(&bcast, &home, &["ordre", "pull", "agent-i"]);
    assert_eq!(code, 0);
    assert!(out.contains(&id), "file pull : {out}");
    let (code, out, _) = rust(
        &bcast,
        &home,
        &[
            "ordre", "ack", &id, "applique", "--from", "agent-i", "--token", &jeton,
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(out.trim(), format!("ack={id} applique"));
    let (code, out, _) = shell(&bcast, &["ordre", "list", "--all"]);
    assert_eq!(code, 1, "quirk shell : liste non vide sort 1");
    assert!(
        out.contains("applique") && out.contains("agent-i"),
        "ACK visible shell : {out}"
    );
    let (code, out, _) = rust(&bcast, &home, &["ordre", "revoque", &id, "--token", &jeton]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), format!("revoque={id}"));
    let (code, _, _) = rust(
        &bcast,
        &home,
        &["ordre", "new", "--token", "agent-i:faux", "--", "x"],
    );
    assert_eq!(code, 1);
    nettoie(&bcast);
}

#[test]
fn usages_et_doctor() {
    let (bcast, home) = bac("doctor");
    assert_eq!(rust(&bcast, &home, &["nope"]).0, 2);
    assert_eq!(
        rust(&bcast, &home, &["send", "--from", "t", "--", "sonde"]).0,
        0
    );
    let (code, out, _) = rust(&bcast, &home, &["doctor"]);
    assert_eq!(code, 0, "doctor : {out}");
    assert!(out.contains("OK inbox"), "doctor : {out}");
    nettoie(&bcast);
}
