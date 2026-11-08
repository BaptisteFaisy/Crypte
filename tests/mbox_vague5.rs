//! Vague 5 mbox : éveil inotify + sorties `--json` (std seul).
//!
//! Pilote le binaire (`--mbox`) sur un `BCAST_DIR` jetable : chaque
//! lecture `--json` rend 0 et un JSON valide (oracle `python3 -m
//! json.tool` sur stdin pipé), aucun secret n'y figure, les codes
//! d'erreur sont inchangés. L'autorité des jetons est confinée par
//! `XDG_DATA_HOME` jetable : aucun test n'écrit hors de son bac.
//!
//! L'éveil inotify (< 1 s, fallback) est couvert par les tests
//! unitaires `mbox::eveil` (crate binaire sans lib : `tick` n'est pas
//! pilotable en intégration) ; ici, `fraicheur_send_tail_json` borne
//! la fraîcheur de lecture bout en bout (< 1 s).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Binaire sous test (construit par cargo avant les tests d'intégration).
fn binaire() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kiro-horror-tui"))
}

/// Bac jetable `(broadcast, xdg)` unique par test (jamais le vrai `.scratch`).
fn bac(tag: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("mbox-vague5-{}-{tag}", std::process::id()));
    // Purge d'abord : un run précédent planté laisse un résidu, et un
    // pid recyclé le retrouverait (flake).
    let _ = std::fs::remove_dir_all(&base);
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

/// Vrai si `texte` est un JSON valide (oracle `python3 -m json.tool`).
/// Panique si python3 est absent (oracle exigé, fail-closed).
fn json_valide(texte: &str) -> bool {
    use std::io::Write;
    use std::process::Stdio;
    let mut enfant = Command::new("python3")
        .arg("-m")
        .arg("json.tool")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("python3 requis pour valider le JSON (oracle json.tool)");
    if let Some(entree) = enfant.stdin.as_mut() {
        if entree.write_all(texte.as_bytes()).is_err() {
            return false;
        }
    }
    drop(enfant.stdin.take());
    enfant.wait().is_ok_and(|s| s.success())
}

/// Émet un jeton et rend son secret (capturé, jamais affiché).
fn fabrique_secret(bcast: &Path, home: &Path, nom: &str) -> String {
    let (code, sortie, _) = rust(bcast, home, &["jeton", "new", nom]);
    assert_eq!(code, 0, "jeton new {nom}");
    sortie
        .split("secret=")
        .nth(1)
        .unwrap_or("")
        .trim()
        .to_string()
}

/// `tail --json` (inbox, sujet, `--all`) : rc 0 + JSON valide.
#[test]
fn json_tail_parse() {
    let (bcast, home) = bac("tail");
    assert_eq!(
        rust(&bcast, &home, &["send", "--from", "v5-t", "--", "coucou"]).0,
        0
    );
    assert_eq!(
        rust(
            &bcast,
            &home,
            &["bus", "-t", "gpu", "--from", "v5-t", "--", "chaud"]
        )
        .0,
        0
    );
    let (code, inbox, _) = rust(&bcast, &home, &["tail", "--json"]);
    assert_eq!(code, 0);
    assert!(json_valide(&inbox), "inbox invalide : {inbox}");
    assert!(inbox.contains("\"expediteur\":\"v5-t\""), "inbox : {inbox}");
    let (code, gpu, _) = rust(&bcast, &home, &["tail", "--bus", "gpu", "--json"]);
    assert_eq!(code, 0);
    assert!(json_valide(&gpu), "bus invalide : {gpu}");
    assert!(gpu.contains("\"dest\":\"@gpu\""), "bus : {gpu}");
    let (code, tout, _) = rust(&bcast, &home, &["tail", "--all", "--json"]);
    assert_eq!(code, 0);
    assert!(json_valide(&tout), "all invalide : {tout}");
    assert!(
        tout.contains("\"inbox\":[") && tout.contains("\"bus\":"),
        "all : {tout}"
    );
    nettoie(&bcast);
}

/// Cycle jeton → ordre : `list`/`pull --json` valides, clés stables.
#[test]
fn json_ordres_cycle_parse() {
    let (bcast, home) = bac("ordres");
    let secret = fabrique_secret(&bcast, &home, "v5-ordres");
    assert!(!secret.is_empty(), "secret émis vide");
    let jeton = format!("v5-ordres:{secret}");
    let (code, _, _) = rust(
        &bcast,
        &home,
        &["ordre", "new", "--token", &jeton, "--", "Relire le plan"],
    );
    assert_eq!(code, 0, "ordre new");
    let (code, liste, _) = rust(&bcast, &home, &["ordre", "list", "--json"]);
    assert_eq!(code, 0);
    assert!(json_valide(&liste), "list invalide : {liste}");
    assert!(
        liste.contains("\"to\":\"ALL\",\"ttl\":24"),
        "clés : {liste}"
    );
    assert!(
        liste.contains("\"texte\":\"Relire le plan\""),
        "corps : {liste}"
    );
    assert!(
        liste.contains("\"hash\":\"155a2205"),
        "hash scellé : {liste}"
    );
    let (code, tout, _) = rust(&bcast, &home, &["ordre", "list", "--all", "--json"]);
    assert_eq!(code, 0);
    assert!(json_valide(&tout), "list --all invalide : {tout}");
    let (code, file, _) = rust(&bcast, &home, &["ordre", "pull", "--json", "v5-lecteur"]);
    assert_eq!(code, 0);
    assert!(json_valide(&file), "pull invalide : {file}");
    assert!(file.contains("\"id\":\"O-"), "pull : {file}");
    nettoie(&bcast);
}

/// `metriques --json` et `piste --json` : rc 0 + JSON valide.
#[test]
fn json_metriques_piste_parse() {
    let (bcast, home) = bac("metpiste");
    assert_eq!(
        rust(&bcast, &home, &["send", "--from", "v5-m", "--", "ping"]).0,
        0
    );
    let secret = fabrique_secret(&bcast, &home, "v5-mp");
    let jeton = format!("v5-mp:{secret}");
    assert_eq!(
        rust(
            &bcast,
            &home,
            &["ordre", "new", "--token", &jeton, "--", "Tracer moi"]
        )
        .0,
        0
    );
    let (code, metriques, _) = rust(&bcast, &home, &["metriques", "--json"]);
    assert_eq!(code, 0);
    assert!(json_valide(&metriques), "metriques invalides : {metriques}");
    assert!(
        metriques.contains("\"compteurs\":{"),
        "compteurs : {metriques}"
    );
    assert!(
        metriques.contains("\"integrite\":"),
        "intégrité : {metriques}"
    );
    let (code, piste, _) = rust(&bcast, &home, &["piste", "--json"]);
    assert_eq!(code, 0);
    assert!(json_valide(&piste), "piste invalide : {piste}");
    assert!(
        piste.contains("\"action\":\"ordre_cree\""),
        "span : {piste}"
    );
    nettoie(&bcast);
}

/// Échappement JSON : guillemets, backslash, emoji → parse OK.
#[test]
fn json_echappement_unicode() {
    let (bcast, home) = bac("echappe");
    assert_eq!(
        rust(
            &bcast,
            &home,
            &["send", "--from", "v5-e", "--", "alerte \"feu\" \\ chemin"]
        )
        .0,
        0
    );
    let (code, sortie, _) = rust(&bcast, &home, &["tail", "--json"]);
    assert_eq!(code, 0);
    assert!(json_valide(&sortie), "échappement invalide : {sortie}");
    assert!(
        sortie.contains("alerte \\\"feu\\\" \\\\ chemin"),
        "écho : {sortie}"
    );
    nettoie(&bcast);
}

/// Aucun secret dans les 5 JSON (messages d'échec caviardés eux aussi).
#[test]
fn json_aucun_secret() {
    let (bcast, home) = bac("secret");
    let secret = fabrique_secret(&bcast, &home, "v5-nofuite");
    assert!(!secret.is_empty(), "secret émis vide");
    let jeton = format!("v5-nofuite:{secret}");
    assert_eq!(
        rust(
            &bcast,
            &home,
            &["ordre", "new", "--token", &jeton, "--", "Secret shell"]
        )
        .0,
        0
    );
    assert_eq!(
        rust(&bcast, &home, &["send", "--from", "v5-s", "--", "ping"]).0,
        0
    );
    let lectures: &[&[&str]] = &[
        &["ordre", "list", "--json"],
        &["ordre", "pull", "--json", "v5-lecteur"],
        &["tail", "--json"],
        &["tail", "--all", "--json"],
        &["metriques", "--json"],
        &["piste", "--json"],
    ];
    for lecture in lectures {
        let (code, sortie, _) = rust(&bcast, &home, lecture);
        assert_eq!(code, 0, "lecture {lecture:?}");
        assert!(json_valide(&sortie), "JSON invalide pour {lecture:?}");
        assert!(!sortie.contains(&secret), "secret présent dans {lecture:?}");
    }
    nettoie(&bcast);
}

/// Codes inchangés sous `--json` : topic/agent/option inconnus → 2.
#[test]
fn json_codes_erreur() {
    let (bcast, home) = bac("codes");
    assert_eq!(
        rust(&bcast, &home, &["tail", "--bus", "nope", "--json"]).0,
        2
    );
    assert_eq!(rust(&bcast, &home, &["ordre", "pull", "--json"]).0, 2);
    assert_eq!(rust(&bcast, &home, &["metriques", "--json", "--nope"]).0, 2);
    assert_eq!(rust(&bcast, &home, &["tail", "--json", "-n", "xx"]).0, 2);
    nettoie(&bcast);
}

/// Fraîcheur bout en bout : `send` puis `tail --json` voit tout (< 1 s).
#[test]
fn fraicheur_send_tail_json() {
    let (bcast, home) = bac("fraicheur");
    let debut = std::time::Instant::now();
    assert_eq!(
        rust(&bcast, &home, &["send", "--from", "v5-f", "--", "frais"]).0,
        0
    );
    let (code, sortie, _) = rust(&bcast, &home, &["tail", "--json"]);
    assert_eq!(code, 0);
    assert!(sortie.contains("\"corps\":\"frais\""), "contenu : {sortie}");
    assert!(debut.elapsed().as_secs() < 1, "fraîcheur dépassée");
    nettoie(&bcast);
}
