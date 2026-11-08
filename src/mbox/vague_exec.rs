//! Vague D6 (exécution) — ordre topo, planificateur, persistance, synthèse.
//!
//! Compagnon de `vague.rs` (qui porte la CLI, la file et le câblage) :
//! `ordre_execution` (Kahn), `execute` (threads + poll 50 ms + timeout),
//! `sauve` (file.log chmod 600), `ecrit_synthese` (APPEND caviardé).
//! Types partagés (`Tache`, `EtatTache`, `dossier_vague`, `caviarder`)
//! réutilisés depuis `super::vague`, aucune duplication.
//!
//! CÂBLAGE : `mod.rs` +1 L (`pub mod vague_exec;`, après `pub mod vague;`).

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use super::fichier::{append_verrouille, reecrire_atomique};
use super::format::{epoque, maintenant, BcastDir};
use super::vague::EtatTache::*;
use super::vague::{caviarder, dossier_vague, EtatTache, Tache};

const PAS_SCRUTE_MS: u64 = 50;

/// Ordre topo (Kahn). Refuse dépendances inconnues, auto-dep, cycles.
pub(crate) fn ordre_execution(taches: &[Tache]) -> Result<Vec<usize>, String> {
    use std::collections::HashSet;
    let index: HashMap<&str, usize> = taches
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.as_str(), i))
        .collect();
    for t in taches {
        for d in &t.apres {
            if d == &t.id {
                return Err(format!("{} depend d'elle-meme", t.id));
            }
            if !index.contains_key(d.as_str()) {
                return Err(format!("{} : dependance inconnue {d}", t.id));
            }
        }
    }
    let mut restantes: HashSet<usize> = (0..taches.len()).collect();
    let mut placees: HashSet<&str> = HashSet::new();
    let mut ordre = Vec::with_capacity(taches.len());
    while !restantes.is_empty() {
        let mut pretes: Vec<usize> = restantes
            .iter()
            .copied()
            .filter(|&i| taches[i].apres.iter().all(|d| placees.contains(d.as_str())))
            .collect();
        if pretes.is_empty() {
            return Err("cycle de dependances".to_string());
        }
        pretes.sort_unstable();
        for i in pretes {
            restantes.remove(&i);
            placees.insert(taches[i].id.as_str());
            ordre.push(i);
        }
    }
    Ok(ordre)
}

/// Vrai si la dépendance `d` a échoué (ou manque : défensif, `run` valide).
fn dep_ko(index: &HashMap<&str, usize>, etats: &[EtatTache], d: &str) -> bool {
    index
        .get(d)
        .map(|&j| matches!(etats[j], Echec | Bloque | Expiree))
        .unwrap_or(true)
}

/// Scrute fins (`Reussie`/`Echec`), timeouts (`Expiree`, retours tardifs
/// ignorés), dépendants de KO (`Bloque`). Garde : `attente` sans `cours`
/// ni lancement fige en `bloque` (impossible hors cycle, déjà exclus).
fn mettre_a_jour(
    taches: &[Tache],
    index: &HashMap<&str, usize>,
    rx: &Receiver<(usize, i32)>,
    etats: &mut [EtatTache],
    debuts: &mut [Option<Instant>],
    delai: Duration,
    lance: usize,
) {
    while let Ok((idx, rc)) = rx.try_recv() {
        if etats[idx] == Cours {
            etats[idx] = if rc == 0 { Reussie } else { Echec };
            debuts[idx] = None;
        }
    }
    let now = Instant::now();
    for (i, tache) in taches.iter().enumerate() {
        let expire = debuts[i].is_some_and(|d| now.duration_since(d) >= delai);
        if etats[i] == Cours && expire {
            etats[i] = Expiree;
            debuts[i] = None;
        }
        if etats[i] == Attente && tache.apres.iter().any(|d| dep_ko(index, etats, d)) {
            etats[i] = Bloque;
        }
    }
    let coince = lance == 0 && etats.contains(&Attente) && !etats.contains(&Cours);
    if coince {
        for e in etats.iter_mut().filter(|e| **e == Attente) {
            *e = Bloque;
        }
    }
}

/// Lance le thread `vague-<id>` : `cli::exec` puis envoi `(i, code)`.
fn lance_thread(
    racine: &Path,
    tache: &Tache,
    i: usize,
    tx: std::sync::mpsc::Sender<(usize, i32)>,
) -> bool {
    let (argv, dir) = (tache.argv.clone(), racine.to_path_buf());
    std::thread::Builder::new()
        .name(format!("vague-{}", tache.id))
        .spawn(move || {
            let _ = tx.send((i, super::cli::exec(&argv, &dir)));
        })
        .is_ok()
}

/// Vrai si toutes les dépendances de `tache` ont réussi.
fn est_prete(index: &HashMap<&str, usize>, etats: &[EtatTache], tache: &Tache) -> bool {
    tache
        .apres
        .iter()
        .all(|d| index.get(d.as_str()).is_some_and(|&j| etats[j] == Reussie))
}

/// Exécute en parallèle (poll 50 ms) ; prête = deps `ok`.
pub(crate) fn execute(
    racine: &Path,
    taches: &[Tache],
    paralleles: usize,
    delai_secs: u64,
) -> Vec<EtatTache> {
    use std::sync::mpsc;
    let mut etats = vec![Attente; taches.len()];
    let mut debuts = vec![None; taches.len()];
    let index: HashMap<&str, usize> = taches
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.as_str(), i))
        .collect();
    let (tx, rx) = mpsc::channel::<(usize, i32)>();
    let delai = Duration::from_secs(delai_secs);
    let pas = Duration::from_millis(PAS_SCRUTE_MS);
    let mut lance = 1; // 1er tour : la garde ne fige pas d'emblée.
    loop {
        mettre_a_jour(taches, &index, &rx, &mut etats, &mut debuts, delai, lance);
        let cours = etats.iter().filter(|e| **e == Cours).count();
        let mut places = paralleles.saturating_sub(cours);
        lance = 0;
        for (i, tache) in taches.iter().enumerate() {
            if places == 0 {
                break;
            }
            if etats[i] != Attente || !est_prete(&index, &etats, tache) {
                continue;
            }
            if lance_thread(racine, tache, i, tx.clone()) {
                etats[i] = Cours;
                debuts[i] = Some(Instant::now());
                places -= 1;
                lance += 1;
            } else {
                eprintln!("bcast: vague: thread impossible ({})", tache.id);
                etats[i] = Echec;
            }
        }
        if !etats.iter().any(|e| matches!(e, Attente | Cours)) {
            break;
        }
        std::thread::sleep(pas);
    }
    mettre_a_jour(taches, &index, &rx, &mut etats, &mut debuts, delai, 1);
    etats
}

/// Persiste la file (atomique verrouillé, chmod 600 : `--token` possible).
pub(crate) fn sauve(dir: &BcastDir, taches: &[Tache], etats: &[EtatTache]) {
    let lignes: Vec<String> = taches
        .iter()
        .zip(etats.iter())
        .map(|(t, e)| {
            format!(
                "{}|{}|{}|{}|{}",
                t.id,
                t.apres.join(","),
                e.texte(),
                epoque(),
                t.argv.join(" ")
            )
        })
        .collect();
    let contenu = if lignes.is_empty() {
        String::new()
    } else {
        lignes.join("\n") + "\n"
    };
    let cible = dossier_vague(dir).join("file.log");
    if reecrire_atomique(&cible, &contenu).is_err() {
        eprintln!("bcast: vague: persistance impossible");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let droits = std::fs::Permissions::from_mode(0o600);
        let _ = std::fs::set_permissions(&cible, droits);
    }
}

/// APPEND la synthèse à `vague-AAAA-MM-JJ.md` (verrouillé, caviardé).
pub(crate) fn ecrit_synthese(
    dir: &BcastDir,
    taches: &[Tache],
    etats: &[EtatTache],
    paralleles: usize,
    delai: u64,
) {
    let date = maintenant();
    let jour = date.get(..10).unwrap_or("sans-date");
    let cible = dossier_vague(dir).join(format!("vague-{jour}.md"));
    let titre = format!(
        "## vague {date} — {} taches (n={paralleles}, delai={delai}s)",
        taches.len()
    );
    let mut lignes = vec![titre];
    for (tache, etat) in taches.iter().zip(etats.iter()) {
        let cmd = caviarder(&tache.argv);
        lignes.push(format!("- [{}] {} :: {cmd}", etat.texte(), tache.id));
    }
    let compte = |voulu: EtatTache| etats.iter().filter(|e| **e == voulu).count();
    lignes.push(format!(
        "synthese: ok={} echec={} bloque={} expiree={}",
        compte(Reussie),
        compte(Echec),
        compte(Bloque),
        compte(Expiree)
    ));
    lignes.push(String::new());
    if append_verrouille(&cible, &lignes).is_err() {
        eprintln!("bcast: vague: synthese impossible");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COMPTEUR: AtomicU64 = AtomicU64::new(0);

    /// Dossier broadcast jetable unique (jamais le vrai `.scratch`).
    fn dir_temp() -> PathBuf {
        let n = COMPTEUR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("mbox-vexec-{n}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dossier de test");
        dir
    }

    /// Fabrique une tâche `metriques`.
    fn tache(id: &str, apres: &[&str]) -> Tache {
        Tache {
            id: id.to_string(),
            apres: apres.iter().map(|s| s.to_string()).collect(),
            argv: vec!["metriques".to_string()],
        }
    }

    #[test]
    fn topo_ordre_et_refus() {
        let taches = vec![tache("A", &["B"]), tache("B", &[]), tache("C", &["A", "B"])];
        let ordre = ordre_execution(&taches).expect("acyclique");
        let rang = |id: &str| ordre.iter().position(|&i| taches[i].id == id).unwrap();
        assert!(rang("B") < rang("A"));
        assert!(rang("A") < rang("C"));
        let cycle = vec![tache("A", &["B"]), tache("B", &["A"])];
        assert!(ordre_execution(&cycle).is_err());
        assert!(ordre_execution(&[tache("A", &["X"])]).is_err());
        assert!(ordre_execution(&[tache("A", &["A"])]).is_err());
    }

    #[test]
    fn scrute_fins_timeout_et_garde() {
        use std::sync::mpsc;
        let taches = vec![tache("A", &[]), tache("B", &["A"])];
        let index: HashMap<&str, usize> = taches
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.as_str(), i))
            .collect();
        let delai = Duration::from_secs(60);
        let (tx, rx) = mpsc::channel();
        let mut etats = vec![Cours, Attente];
        let mut debuts = vec![Some(Instant::now()), None];
        tx.send((0, 0)).unwrap();
        mettre_a_jour(&taches, &index, &rx, &mut etats, &mut debuts, delai, 1);
        assert_eq!(etats, vec![Reussie, Attente]);
        let mut etats = vec![Cours, Attente];
        let vieux = Instant::now() - Duration::from_secs(61);
        let mut debuts = vec![Some(vieux), None];
        mettre_a_jour(&taches, &index, &rx, &mut etats, &mut debuts, delai, 1);
        assert_eq!(etats, vec![Expiree, Bloque]);
        let mut etats = vec![Attente, Attente];
        let mut debuts = vec![None, None];
        mettre_a_jour(&taches, &index, &rx, &mut etats, &mut debuts, delai, 0);
        assert_eq!(etats, vec![Bloque, Bloque]);
    }

    #[test]
    fn sauve_et_synthese() {
        let racine = dir_temp();
        let dir = BcastDir::depuis(racine.clone());
        let argv: Vec<String> = ["ordre", "ack", "O-1", "--token", "N:S"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let taches = vec![Tache {
            id: "V-1".into(),
            apres: vec![],
            argv,
        }];
        sauve(&dir, &taches, &[Reussie]);
        ecrit_synthese(&dir, &taches, &[Reussie], 4, 60);
        let file = std::fs::read_to_string(dossier_vague(&dir).join("file.log")).expect("file");
        assert!(file.starts_with("V-1||ok|"), "file : {file}");
        let synthese = dossier_vague(&dir).join(format!("vague-{}.md", &maintenant()[..10]));
        let texte = std::fs::read_to_string(&synthese).expect("synthese");
        assert!(
            texte.contains("***") && !texte.contains("N:S"),
            "fuite : {texte}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(dossier_vague(&dir).join("file.log")).expect("file");
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&racine);
    }
}
