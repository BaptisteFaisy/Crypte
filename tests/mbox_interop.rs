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

/// Émet un jeton seul : rend `"nom:secret"` (sans ordre associé).
fn emet_jeton_seul(bcast: &Path, home: &Path, agent: &str) -> String {
    let (code, out, _) = rust(bcast, home, &["jeton", "new", agent]);
    assert_eq!(code, 0);
    let secret = out
        .trim()
        .strip_prefix(&format!("jeton={agent} secret="))
        .expect("émission");
    format!("{agent}:{secret}")
}

/// Écrit une file vague `V-1 bus → V-2 hb → V-3 ack KO (secret)` : rend le chemin.
fn ecrit_file_vague(bcast: &Path) -> String {
    let fichier = bcast.join("vague.txt");
    std::fs::write(
        &fichier,
        "V-1||bus -t divers --from vague-t -- coucou-vague\n\
         V-2|V-1|hb sonde-vague ok\n\
         V-3|V-2|ordre ack O-x lu --from t --token N:SECRET123\n",
    )
    .expect("file");
    fichier.to_str().expect("chemin").to_string()
}

/// Lit l'unique synthèse `vague-*.md` (panique si absente ou multiple).
fn lis_synthese(bcast: &Path) -> String {
    let mut trouves = Vec::new();
    for entree in std::fs::read_dir(bcast.join("vague")).expect("dossier") {
        let chemin = entree.expect("entrée").path();
        if chemin.extension().and_then(|e| e.to_str()) == Some("md") {
            trouves.push(std::fs::read_to_string(&chemin).expect("synthèse"));
        }
    }
    assert_eq!(trouves.len(), 1, "une synthèse");
    trouves.pop().expect("synthèse")
}

/// Extrait la valeur `sig=` d'une ligne d'ordre (panique si absente).
fn extrait_sig(ligne: &str) -> &str {
    ligne
        .split("sig=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .expect("valeur")
}

/// Relit la ligne d'un ordre dans `ORDRES.log` (panique si absente).
fn ligne_ordre(bcast: &Path, id: &str) -> String {
    std::fs::read_to_string(bcast.join("ORDRES.log"))
        .expect("journal")
        .lines()
        .find(|l| l.starts_with(id))
        .expect("ordre persisté")
        .to_string()
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
fn delegation_transfert_piste_et_ack_destinataire() {
    let (bcast, home) = bac("delegation");
    let (jeton_a, id) = emet_jeton_et_ordre(&bcast, &home, "agent-a", "mission à transférer");
    let jeton_b = emet_jeton_seul(&bcast, &home, "agent-b");
    let (code, out, _) = rust(
        &bcast,
        &home,
        &[
            "ordre",
            "deleguer",
            &id,
            "--to",
            "@agent-b",
            "--resume",
            "reprends ici",
            "--token",
            &jeton_a,
        ],
    );
    assert_eq!(code, 0);
    let id2 = out
        .trim()
        .strip_prefix("delegue=")
        .expect("transfert")
        .to_string();
    assert_ne!(id, id2);
    let ligne = ligne_ordre(&bcast, &id2);
    assert!(ligne.contains(&format!("supersede={id}")), "lien : {ligne}");
    assert!(
        !ligne.contains("sig="),
        "transfert non scellé (ack destinataire) : {ligne}"
    );
    let (code, out, _) = rust(&bcast, &home, &["ordre", "pull", "@agent-b"]);
    assert_eq!(code, 0);
    assert!(out.contains(&id2), "file destinataire : {out}");
    let (code, out, _) = rust(
        &bcast,
        &home,
        &[
            "ordre", "ack", &id2, "applique", "--from", "agent-b", "--token", &jeton_b,
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(out.trim(), format!("ack={id2} applique"));
    nettoie(&bcast);
}

#[test]
fn piste_trace_creation_ack_et_doctor() {
    let (bcast, home) = bac("piste");
    let (jeton, id) = emet_jeton_et_ordre(&bcast, &home, "agent-p", "tracer moi");
    let (code, _, _) = rust(
        &bcast,
        &home,
        &[
            "ordre", "ack", &id, "lu", "--from", "agent-p", "--token", &jeton,
        ],
    );
    assert_eq!(code, 0);
    let (code, out, _) = rust(&bcast, &home, &["piste"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("ordre_cree") && out.contains(&id),
        "création : {out}"
    );
    assert!(out.contains("ordre_ack"), "ack : {out}");
    assert!(!out.contains(&jeton), "fuite jeton : {out}");
    let (code, out, _) = rust(&bcast, &home, &["piste", "--ordre", &id]);
    assert_eq!(code, 0);
    assert!(out.contains("ordre_ack"), "filtre : {out}");
    let (code, out, _) = rust(&bcast, &home, &["doctor"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("piste 2 spans 1 ordres traces"),
        "doctor : {out}"
    );
    nettoie(&bcast);
}

#[test]
fn signature_sceau_verifie_et_shell_interop() {
    let (bcast, home) = bac("signature");
    let (_, id) = emet_jeton_et_ordre(&bcast, &home, "agent-s", "ordre scellé");
    let ligne = ligne_ordre(&bcast, &id);
    assert!(ligne.contains("sig="), "sceau : {ligne}");
    let sig = extrait_sig(&ligne);
    assert_eq!(sig.len(), 40, "HMAC hex : {sig}");
    let jeton_b = emet_jeton_seul(&bcast, &home, "agent-t");
    let (code, _, _) = rust(
        &bcast,
        &home,
        &[
            "ordre", "ack", &id, "lu", "--from", "agent-t", "--token", &jeton_b,
        ],
    );
    assert_eq!(
        code, 1,
        "sceau d'un autre jeton : refus silencieux (code 1, comme tout ACK refusé)"
    );
    let nu = "O-20000101-001|946684800|2000-01-01 00:00|ALL|0|actif|shell-doc|Ordre shell nu\n";
    let mut contenu = std::fs::read_to_string(bcast.join("ORDRES.log")).expect("journal");
    contenu.push_str(nu);
    std::fs::write(bcast.join("ORDRES.log"), contenu).expect("ajout shell");
    let (code, out, _) = rust(
        &bcast,
        &home,
        &[
            "ordre",
            "ack",
            "O-20000101-001",
            "lu",
            "--from",
            "agent-t",
            "--token",
            &jeton_b,
        ],
    );
    assert_eq!(code, 0);
    assert!(out.contains("ack=O-20000101-001 lu"), "interop : {out}");
    let (code, out, _) = rust(&bcast, &home, &["metriques"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("signatures: 1 ok 0 ko 1 sans"),
        "ligne : {out}"
    );
    nettoie(&bcast);
}

#[test]
fn vague_plan_run_et_synthese_caviardee() {
    let (bcast, home) = bac("vague");
    let f = ecrit_file_vague(&bcast);
    let (code, out, _) = rust(&bcast, &home, &["vague", "plan", "--fichier", &f]);
    assert_eq!(code, 0);
    assert!(out.contains("3 taches"), "plan : {out}");
    let (code, out, _) = rust(
        &bcast,
        &home,
        &["vague", "run", "--fichier", &f, "--paralleles", "2"],
    );
    assert_eq!(code, 1, "V-3 échoue : {out}");
    assert!(out.contains("ok=2 ko=1"), "compteurs : {out}");
    let (code, out, _) = rust(&bcast, &home, &["vague", "lister"]);
    assert_eq!(code, 0);
    assert!(out.contains("V-3 [echec]"), "file : {out}");
    assert!(!out.contains("SECRET123"), "fuite : {out}");
    let (code, out, _) = rust(&bcast, &home, &["vague", "etat"]);
    assert_eq!(code, 0);
    assert!(out.contains("ok=2 echec=1"), "état : {out}");
    let bus = std::fs::read_to_string(bcast.join("bus/divers.log")).expect("bus");
    assert!(bus.contains("coucou-vague"), "bus : {bus}");
    let texte = lis_synthese(&bcast);
    assert!(
        texte.contains("***") && !texte.contains("SECRET123"),
        "caviarde : {texte}"
    );
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
