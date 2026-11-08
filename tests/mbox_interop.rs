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

/// Émet un jeton puis un ordre : `(jeton "nom:secret", id ordre)`.
fn emet_jeton_et_ordre(bcast: &Path, home: &Path, agent: &str, texte: &str) -> (String, String) {
    let (code, out, _) = rust(bcast, home, &["jeton", "new", agent]);
    assert_eq!(code, 0);
    let prefixe = format!("jeton={agent} secret=");
    let secret = out
        .trim()
        .strip_prefix(prefixe.as_str())
        .expect("émission")
        .to_string();
    assert_eq!(secret.len(), 64);
    let jeton = format!("{agent}:{secret}");
    let (code, out, _) = rust(
        bcast,
        home,
        &["ordre", "new", "--token", &jeton, "--", texte],
    );
    assert_eq!(code, 0);
    let id = out
        .trim()
        .strip_prefix("ordre=")
        .expect("création")
        .to_string();
    assert!(id.starts_with("O-"), "identifiant v2 : {id}");
    (jeton, id)
}

#[test]
fn jeton_ordre_ack_bout_en_bout() {
    let (bcast, home) = bac("ordres-ack");
    let (jeton, id) = emet_jeton_et_ordre(&bcast, &home, "agent-i", "relire le plan");
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
    nettoie(&bcast);
}

#[test]
fn ordre_revoque_et_refus_mauvais_jeton() {
    let (bcast, home) = bac("ordres-revoque");
    let (jeton, id) = emet_jeton_et_ordre(&bcast, &home, "agent-j", "brouillon à révoquer");
    let (code, out, _) = rust(&bcast, &home, &["ordre", "revoque", &id, "--token", &jeton]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), format!("revoque={id}"));
    let (code, _, _) = rust(
        &bcast,
        &home,
        &["ordre", "new", "--token", "agent-j:faux", "--", "x"],
    );
    assert_eq!(code, 1);
    nettoie(&bcast);
}

#[test]
fn metriques_persistent_et_affichent() {
    let (bcast, home) = bac("metriques");
    let (code, out, _) = rust(&bcast, &home, &["send", "--from", "t-m", "--", "un"]);
    assert_eq!((code, out.trim()), (0, "sent"));
    let (code, out, _) = rust(&bcast, &home, &["send", "--from", "t-m", "--", "un"]);
    assert_eq!((code, out.trim()), (0, "throttled:dup"));
    let (code, out, _) = rust(&bcast, &home, &["metriques"]);
    assert_eq!(code, 0);
    assert!(out.contains("send_ok=1"), "compteurs : {out}");
    assert!(out.contains("send_dup=1"), "compteurs : {out}");
    assert!(out.contains("integrite ordres:"), "intégrité : {out}");
    let brut = std::fs::read_to_string(bcast.join(".state/metriques")).expect("persisté");
    assert!(brut.contains("send_ok=1"), "fichier : {brut}");
    let (code, out, _) = rust(&bcast, &home, &["doctor"]);
    assert_eq!(code, 0);
    assert!(out.contains("metriques send="), "doctor : {out}");
    nettoie(&bcast);
}

#[test]
fn fantome_miroir_sans_ecriture_prod() {
    let (bcast, home) = bac("fantome");
    let (code, _, _) = rust(&bcast, &home, &["send", "--from", "t-f", "--", "prod"]);
    assert_eq!(code, 0);
    let avant = std::fs::read_to_string(bcast.join("BOITE-USER.md")).expect("prod");
    let (code, out, _) = rust(
        &bcast,
        &home,
        &["--fantome", "send", "--from", "t-f", "--", "ombre"],
    );
    assert_eq!(code, 0);
    assert!(out.contains("miroir seul"), "fantôme : {out}");
    let apres = std::fs::read_to_string(bcast.join("BOITE-USER.md")).expect("prod intacte");
    assert_eq!(avant, apres, "la prod ne bouge pas");
    let miroir = std::fs::read_to_string(bcast.join(".fantome/BOITE-USER.md")).expect("miroir");
    assert!(miroir.contains("ombre"), "miroir : {miroir}");
    let corpus = bcast.join("corpus.txt");
    std::fs::write(
        &corpus,
        "# essai\nbus -t gpu --from t-f -- chaud\njeton new x\n",
    )
    .expect("corpus");
    let (code, out, _) = rust(
        &bcast,
        &home,
        &["fantome", "rejouer", corpus.to_str().unwrap()],
    );
    assert_eq!(code, 0);
    assert!(out.contains("refuse"), "garde typée : {out}");
    assert_eq!(rust(&bcast, &home, &["fantome", "diff"]).0, 0);
    assert_eq!(rust(&bcast, &home, &["fantome", "purge"]).0, 0);
    assert!(!bcast.join(".fantome").exists());
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
