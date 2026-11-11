//! Sorties `--json` des lectures mbox (vague 5, lot json) : polyvalence LLM.
//! `ordre list|pull --json`, `tail|metriques|piste --json` : JSON sur stdout,
//! codes inchangés (0 + `[]`/objet, erreurs intactes), aucun secret (corps
//! publics, compteurs, spans sans secret).
//! JSON MAISON, zéro dépendance : `serde_json` est PRÉSENT au lock (1.0.154,
//! `Cargo.lock:931/1418/1550`, `Cargo.toml:30`) mais le lot impose la maison
//! (`echappe` : guillemets, backslash, `\n \r \t \b \f`, contrôles `\u00xx`,
//! UTF-8 tel quel). `Cargo.toml` intouché, sûr hors-ligne.
//! Formes stables : ordres/pull `[{"id","to","ttl":N,"statut","src","texte","prio","supersede","hash","sig"}]`
//! (`texte` = corps nu, extensions éclatées, `null` si absentes ; vide = `[]`) ; tail
//! `[{"date","expediteur","priorite","dest","corps"}]`, `--all` = `{"inbox":[…],"bus":{"phone":[…],"gpu":[…],"file":[…],"divers":[…]}}`
//! (ordres exclus → `ordre list --json` ; suites rattachées `\n`, malformées ignorées, tampon retiré) ;
//! metriques `{"compteurs":{…},"agents":{"<san>":{…}},"integrite":{"ordres":{"ok","ko","sans"},"signatures":{…}}}`
//! (`--agent` filtre `agents` seul, nombres nus) ; piste `[{"run_id","epoque":N,"agent","action","ordre_id","verdict"}]`.
//! CÂBLAGE (assembleur, budgets au 2026-11-08) : `mod.rs` (444 L) +1 L `pub mod json;` (alpha `jeton`/`journal`).
//! `--json` = drapeau sans valeur : `extrait_option` (`cli_gestion.rs:247-266`) exige une valeur → variante drapeau ici
//! (`est_drapeau_json` + `sans_json`, même patron, positionnels préservés), AVANT `decoupage` (`outils.rs:40`, inconnu = rc 2).
//! `cli_gestion.rs` (452 L) `ordre list` +3 L : `let j = est_drapeau_json(&args[1..]); let f = sans_json(&args[1..]);`
//! → `let tout = f.first().is_some_and(|a| a == "--all");` (`--all` sur épurées, sémantique `$1` gardée)
//! → `if j { println!("{}", liste_ordres_json(dir, tout)); } else { print!("{}", lister(dir, tout)); }`.
//! `cli_gestion.rs` `ordre pull` +3 L : `let j` + `let f`, `f.first()` au lieu de `args.first()` (même `usg`),
//! → `if j { println!("{}", pull_json(dir, agent)); } else { print!("{}", tirer(dir, agent)); }`.
//! `metriques.rs` (240 L) +3 L, `piste.rs` (361 L) +3 L : fantôme 2 L en tête (`let j = …;` + `let args = sans_json(args);`
//! → `let args: &[String] = &args;`, suite inchangée) puis 1 L après `decoupage` (métriques, `opt agent` brut, `sanitize`
//! dedans) ou après les `filtre_…` (piste, déjà normalisés, `champ` privé) : `if j { println!("{}", metriques_json(dir,`
//! `opt(&opts,"agent"))); return Sortie::Ok; }` (piste : `piste_json(dir, filtre_ordre.as_deref(), filtre_agent.as_deref())`).
//! `cli.rs` (492 L, +6 MAX) `cmd_tail` +3 L → 495 : même fantôme 2 L, puis après le parse de `n` (erreurs intactes) :
//! `if j { if let Some(t) = tail_json(dir, n, opt(&opts,"bus"), flag(&opts,"all")) { println!("{t}"); return Sortie::Ok; } }`
//! (`None` = topic inconnu : retombe sur l'`usg` existant, rc 2, zéro duplication du message).
//! Interop : NOUVEAU `tests/mbox_vague5.rs` (std seul) : chaque `--json` rc 0 et parsé par `python3 -m json.tool`
//! (via `Command`, stdin pipé) ; échappement `"`, `\`, emoji vérifié ; clés `to`/`ttl`/`hash` présentes ; secret
//! utilisé absent des JSON ; topics et options inconnus → rc 2.
//! Zéro duplication : `Ordre::parse_ligne`, `Extensions::extraire`, `pendants_pour`, `expirer_journal`, `rendre_vue`,
//! `tail_n`, `est_message`, `est_suite`, `priorite_ligne`, `sanitize`, `lire`/`verifie_integrite`, `compte_journal`, `piste::lire`.

use std::collections::BTreeMap;

use super::ack::pendants_pour;
use super::fichier::{reecrire_atomique, tail_n};
use super::format::{est_message, est_suite, priorite_ligne, sanitize, BcastDir, Priorite};
use super::journal::{expirer_journal, rendre_vue};
use super::metriques::{lire as lire_compteurs, verifie_integrite};
use super::ordres::{Extensions, Ordre, StatutOrdre};
use super::piste::{lire as lire_spans, Span};
use super::signature::compte_journal;
use super::{epoque, SUJETS_BUS};

/// Échappe JSON (`" \ \n \r \t \b \f`, contrôles `\u00xx`, UTF-8 tel quel).
pub(crate) fn echappe(texte: &str) -> String {
    let mut out = String::with_capacity(texte.len() + 2);
    for c in texte.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Chaîne JSON entre guillemets (`""` si vide).
fn chaine(texte: &str) -> String {
    format!("\"{}\"", echappe(texte))
}

/// Optionnelle : `"v"` échappée ou `null` (extension absente).
fn peut_etre(valeur: Option<&str>) -> String {
    valeur.map(chaine).unwrap_or_else(|| "null".to_string())
}

/// Triplet `{"ok":N,"ko":N,"sans":N}` (ordres, signatures).
fn triplet(t: (usize, usize, usize)) -> String {
    format!("{{\"ok\":{},\"ko\":{},\"sans\":{}}}", t.0, t.1, t.2)
}

/// Vrai si `--json` dans les args brutes. AVANT `decoupage` (rc 2).
pub(crate) fn est_drapeau_json(args: &[String]) -> bool {
    args.iter().any(|a| a == "--json")
}

/// Retire `--json` (lectures seules : un agent `--json` serait mangé).
pub(crate) fn sans_json(args: &[String]) -> Vec<String> {
    args.iter()
        .filter(|a| a.as_str() != "--json")
        .cloned()
        .collect()
}

/// Un ordre en objet JSON (clés stables, `ttl` numérique, texte nu).
pub(crate) fn rend_ordre(ordre: &Ordre) -> String {
    let (corps, ext) = Extensions::extraire(&ordre.texte);
    format!(
        "{{\"id\":{},\"to\":{},\"ttl\":{},\"statut\":{},\"src\":{},\
         \"texte\":{},\"prio\":{},\"supersede\":{},\"hash\":{},\"sig\":{}}}",
        chaine(&ordre.id),
        chaine(&ordre.dest),
        ordre.ttl_h,
        chaine(ordre.statut.texte()),
        chaine(&ordre.src),
        chaine(&corps),
        peut_etre(ext.prio.as_deref()),
        peut_etre(ext.supersede.as_deref()),
        peut_etre(ext.hash.as_deref()),
        peut_etre(ext.sig.as_deref())
    )
}

/// Tableau d'ordres (`[]` si vide, jamais de texte).
pub(crate) fn rend_ordres(ordres: &[Ordre]) -> String {
    let dedans: Vec<String> = ordres.iter().map(rend_ordre).collect();
    format!("[{}]", dedans.join(","))
}

/// `ordre list --json` : sélection `lister` (expire + vue), `[]` si vide.
pub(crate) fn liste_ordres_json(dir: &BcastDir, tout: bool) -> String {
    let change = expirer_journal(dir).unwrap_or(false);
    let contenu = std::fs::read_to_string(dir.ordres()).unwrap_or_default();
    if change {
        let _ = reecrire_atomique(&dir.vue_ordres(), &rendre_vue(&contenu, epoque()));
    }
    let ordres: Vec<Ordre> = contenu
        .lines()
        .filter_map(Ordre::parse_ligne)
        .filter(|o| !o.id.is_empty() && (tout || o.statut == StatutOrdre::Actif))
        .collect();
    rend_ordres(&ordres)
}

/// `ordre pull --json` : file `tirer` (`pendants_pour`), `[]` si vide.
pub(crate) fn pull_json(dir: &BcastDir, agent: &str) -> String {
    let _ = expirer_journal(dir);
    let ordres = std::fs::read_to_string(dir.ordres()).unwrap_or_default();
    let acks = std::fs::read_to_string(dir.acks()).unwrap_or_default();
    rend_ordres(&pendants_pour(&ordres, &acks, agent))
}

/// Message v2 vu pour JSON : 5 champs stables de `tail --json`.
struct LigneVue {
    date: String,
    expediteur: String,
    priorite: String,
    dest: String,
    corps: String,
}

/// Parse `- [DATE] exp … : corps {#tampon}` (`None` si pas un message).
/// Expéditeur = tête jusqu'au premier `[Pn]` (multi-mots OK), dest = suite,
/// legacy sans tag = `P1`. Priorité sur la TÊTE seule, tampon retiré.
fn parse_message(ligne: &str) -> Option<LigneVue> {
    if !est_message(ligne) {
        return None;
    }
    let date = ligne.get(3..19)?.to_string();
    let (tete, corps) = ligne.split_once(" : ")?;
    let tete = tete.split_once("] ").map(|(_, r)| r).unwrap_or("");
    let tags = ["[P0]", "[P1]", "[P2]"];
    let marque = tags.iter().filter_map(|m| tete.find(m)).min();
    let mut expediteur = tete.trim().to_string();
    let mut dest = String::new();
    if let Some(i) = marque {
        expediteur = tete[..i].trim_end().to_string();
        dest = tete[i + 4..].trim().to_string();
    }
    let priorite = match priorite_ligne(tete) {
        Priorite::P0 => "P0",
        Priorite::P1 => "P1",
        Priorite::P2 => "P2",
    }
    .to_string();
    let corps = match corps.rfind("{#") {
        Some(i) if corps[i..].starts_with("{#pid=") && corps.ends_with('}') => {
            corps[..i].trim_end().to_string()
        }
        _ => corps.to_string(),
    };
    Some(LigneVue {
        date,
        expediteur,
        priorite,
        dest,
        corps,
    })
}

/// Un message vu en objet JSON (5 clés, ordre fixe).
fn rend_message(vue: &LigneVue) -> String {
    format!(
        "{{\"date\":{},\"expediteur\":{},\"priorite\":{},\"dest\":{},\"corps\":{}}}",
        chaine(&vue.date),
        chaine(&vue.expediteur),
        chaine(&vue.priorite),
        chaine(&vue.dest),
        chaine(&vue.corps)
    )
}

/// Lignes brutes en tableau : suites rattachées (`\n`), reste ignoré.
fn rend_tail(lignes: &[String]) -> String {
    let mut vues: Vec<LigneVue> = Vec::new();
    for ligne in lignes {
        if let Some(vue) = parse_message(ligne) {
            vues.push(vue);
        } else if est_suite(ligne) {
            if let Some(dernier) = vues.last_mut() {
                dernier.corps.push('\n');
                dernier.corps.push_str(ligne.trim_start());
            }
        }
    }
    let dedans: Vec<String> = vues.iter().map(rend_message).collect();
    format!("[{}]", dedans.join(","))
}

/// `tail --json` (inbox/sujet/`--all`). `None` = topic inconnu (→ `usg`).
pub(crate) fn tail_json(dir: &BcastDir, n: usize, bus: Option<&str>, tout: bool) -> Option<String> {
    if tout {
        let mut o = format!("{{\"inbox\":{}", rend_tail(&tail_n(&dir.boite(), n)));
        o.push_str(",\"bus\":{");
        for (i, sujet) in SUJETS_BUS.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            let vus = rend_tail(&tail_n(&dir.bus(sujet), n));
            o.push_str(&format!("{}:{}", chaine(sujet), vus));
        }
        o.push_str("}}");
        return Some(o);
    }
    match bus {
        None => Some(rend_tail(&tail_n(&dir.boite(), n))),
        Some(sujet) if SUJETS_BUS.contains(&sujet) => Some(rend_tail(&tail_n(&dir.bus(sujet), n))),
        Some(_) => None,
    }
}

/// `tail --json` servi : affiche et rend vrai (`None` = topic inconnu,
/// l'appelant retombe sur son `usg` existant, zéro duplication).
pub(crate) fn affiche_tail_json(dir: &BcastDir, n: usize, bus: Option<&str>, tout: bool) -> bool {
    match tail_json(dir, n, bus, tout) {
        Some(t) => {
            println!("{t}");
            true
        }
        None => false,
    }
}

/// Compteurs + intégrité (triés, nombres nus). `filtre` = un ou tous.
fn rend_metriques(
    compteurs: &BTreeMap<String, u64>,
    filtre: Option<&str>,
    ordres: (usize, usize, usize),
    sigs: (usize, usize, usize),
) -> String {
    let mut globales = Vec::new();
    let mut agents: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (cle, valeur) in compteurs {
        match cle.strip_prefix("par_agent.") {
            Some(reste) => {
                if let Some((san, sous)) = reste.split_once('.') {
                    if filtre.is_none_or(|f| f == san) {
                        let entree = agents.entry(san).or_default();
                        entree.push(format!("{}:{valeur}", chaine(sous)));
                    }
                }
            }
            None => globales.push(format!("{}:{valeur}", chaine(cle))),
        }
    }
    let mut o = format!("{{\"compteurs\":{{{}}},\"agents\":{{", globales.join(","));
    for (i, (san, paires)) in agents.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&format!("{}:{{{}}}", chaine(san), paires.join(",")));
    }
    o.push_str(&format!(
        "}},\"integrite\":{{\"ordres\":{},\"signatures\":{}}}}}",
        triplet(ordres),
        triplet(sigs)
    ));
    o
}

/// `metriques --json` : `lire` + intégrités ; `agent` brut, `sanitize` dedans.
pub(crate) fn metriques_json(dir: &BcastDir, agent: Option<&str>) -> String {
    rend_metriques(
        &lire_compteurs(dir),
        agent.map(sanitize).as_deref(),
        verifie_integrite(dir),
        compte_journal(dir),
    )
}

/// Un span en objet JSON (clés = champs, `epoque` numérique).
pub(crate) fn rend_span(span: &Span) -> String {
    format!(
        "{{\"run_id\":{},\"epoque\":{},\"agent\":{},\"action\":{},\
         \"ordre_id\":{},\"verdict\":{}}}",
        chaine(&span.run_id),
        span.epoch,
        chaine(&span.agent),
        chaine(span.action.texte()),
        chaine(&span.ordre_id),
        chaine(&span.verdict)
    )
}

/// Spans en tableau (`[]` si vide, chronologique).
pub(crate) fn rend_piste(spans: &[Span]) -> String {
    let dedans: Vec<String> = spans.iter().map(rend_span).collect();
    format!("[{}]", dedans.join(","))
}

/// `piste --json` : filtres déjà normalisés (cf. `cmd_piste`).
pub(crate) fn piste_json(dir: &BcastDir, ordre: Option<&str>, agent: Option<&str>) -> String {
    let spans: Vec<Span> = lire_spans(dir)
        .into_iter()
        .filter(|s| ordre.is_none_or(|f| f == s.ordre_id))
        .filter(|s| agent.is_none_or(|f| f == s.agent))
        .collect();
    rend_piste(&spans)
}

// ---------------------------------------------------------------------------
// Tests unitaires intra-fichier (convention Crypte).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::ordres::empreinte;
    use super::*;

    /// Bac broadcast jetable unique (jamais le vrai `.scratch`).
    fn bac(tag: &str) -> BcastDir {
        let dir = std::env::temp_dir().join(format!("mbox-json-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(dir.join("bus")).expect("bac");
        std::fs::create_dir_all(dir.join(".state")).expect("bac");
        BcastDir::depuis(dir)
    }

    #[test]
    fn echappe_guillemets_controles_unicode() {
        assert_eq!(echappe("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(echappe("l1\nl2\r\ttab"), "l1\\nl2\\r\\ttab");
        assert_eq!(echappe("\u{08}\u{0C}\u{01}"), "\\b\\f\\u0001");
        assert_eq!(echappe("héllo 🌑"), "héllo 🌑"); // UTF-8 tel quel
        assert_eq!(chaine(""), "\"\"");
        assert_eq!(peut_etre(None), "null");
        assert_eq!(peut_etre(Some("x\"y")), "\"x\\\"y\"");
        assert_eq!(triplet((1, 2, 3)), "{\"ok\":1,\"ko\":2,\"sans\":3}");
    }

    #[test]
    fn ordres_liste_et_pull_projettent() {
        let dir = bac("ordres");
        std::fs::write(
            dir.ordres(),
            "O-1|1000|d|ALL|0|actif|s1|Faire \"X\" prio=P0 hash=abc\n\
             O-2|1000|d|ALL|0|revoque|s2|Vieux\n",
        )
        .expect("journal");
        let id = "\"id\":\"O-1\",\"to\":\"ALL\",\"ttl\":0";
        let corps = "\"texte\":\"Faire \\\"X\\\"\"";
        let tout = liste_ordres_json(&dir, true);
        assert!(tout.contains(id), "tout:{tout}");
        assert!(tout.contains("\"statut\":\"actif\""), "tout:{tout}");
        assert!(tout.contains(corps), "corps nu:{tout}");
        assert!(tout.contains("\"prio\":\"P0\""), "ext:{tout}");
        assert!(tout.contains("\"sig\":null"), "nulle:{tout}");
        assert!(tout.contains("\"id\":\"O-2\""), "revoque:{tout}");
        let actifs = liste_ordres_json(&dir, false);
        assert!(!actifs.contains("O-2"), "actifs:{actifs}");
        assert!(pull_json(&dir, "alice").contains("\"id\":\"O-1\""), "pull");
        std::fs::write(dir.acks(), "O-1|1000|alice|applique|ok\n").expect("ack");
        assert_eq!(pull_json(&dir, "alice"), "[]"); // soldé = vide
        let _ = std::fs::remove_dir_all(dir.racine());
    }

    #[test]
    fn tail_parse_continuations_et_all() {
        let dir = bac("tail");
        std::fs::write(
            dir.boite(),
            "- [2026-11-08 10:00] muse (bcast) [P0] @x : alerte \"feu\" {#pid=1 ppid=1 tty=- sess=1}\n\
             \x20 suite accentuée é\n\
             pourri sans tiret\n\
             - [2026-11-08 10:01] bob : legacy sans tag\n",
        )
        .expect("boite");
        std::fs::write(dir.bus("gpu"), "- [2026-11-08 10:02] g [P2] : chaud\n").expect("bus");
        let date = "\"date\":\"2026-11-08 10:00\"";
        let exp = "\"expediteur\":\"muse (bcast)\"";
        let alerte = "alerte \\\"feu\\\"\\nsuite accentuée é";
        let bus = "\"bus\":{\"phone\":[],\"gpu\":[{";
        let inbox = tail_json(&dir, 20, None, false).expect("inbox");
        assert!(inbox.contains(date), "inbox:{inbox}");
        assert!(inbox.contains(exp), "multi:{inbox}");
        assert!(inbox.contains("\"priorite\":\"P0\""), "prio:{inbox}");
        assert!(inbox.contains("\"dest\":\"@x\""), "dest:{inbox}");
        assert!(inbox.contains(alerte), "corps:{inbox}");
        assert!(!inbox.contains("pid="), "tampon:{inbox}");
        assert!(!inbox.contains("pourri"), "malformée:{inbox}");
        assert!(inbox.contains("\"priorite\":\"P1\""), "legacy:{inbox}");
        let gpu = tail_json(&dir, 5, Some("gpu"), false).expect("bus");
        assert!(gpu.contains("\"expediteur\":\"g\""), "bus:{gpu}");
        assert!(tail_json(&dir, 5, Some("nope"), false).is_none(), "topic");
        let tout = tail_json(&dir, 20, None, true).expect("all");
        assert!(tout.starts_with("{\"inbox\":["), "all:{tout}");
        assert!(tout.contains(bus), "bus:{tout}");
        let _ = std::fs::remove_dir_all(dir.racine());
    }

    #[test]
    fn metriques_tri_filtre_integrite() {
        let dir = bac("metriques");
        let bon = empreinte("Relire le plan");
        std::fs::write(
            dir.ordres(),
            format!(
                "O-1|1000|d|ALL|0|actif|s|Relire le plan hash={bon}\n\
                 O-2|1000|d|ALL|0|actif|s|Relire le plan hash=faux\n\
                 O-3|1000|d|ALL|0|actif|s|Ordre shell nu\n"
            ),
        )
        .expect("journal");
        let cle = format!("par_agent.{}.ack_ok", sanitize("muse"));
        let corps = format!("send_ok=3\n{cle}=1\nbus_ok=2\nmalformée\n");
        let cible = dir.joindre(".state/metriques");
        std::fs::write(&cible, corps).expect("compteurs");
        let tri = "\"compteurs\":{\"bus_ok\":2,\"send_ok\":3}";
        let ag = format!("\"{}\":{{\"ack_ok\":1}}", sanitize("muse"));
        let int = "\"ordres\":{\"ok\":1,\"ko\":1,\"sans\":1}";
        let sig = "\"signatures\":{\"ok\":0,\"ko\":0,\"sans\":3}";
        let j = metriques_json(&dir, None);
        assert!(j.contains(tri), "tri:{j}");
        assert!(j.contains(&ag), "ag:{j}");
        assert!(j.contains(int), "int:{j}");
        assert!(j.contains(sig), "sig:{j}");
        let f = metriques_json(&dir, Some("muse"));
        assert!(f.contains("\"ack_ok\":1"), "garde");
        let g = metriques_json(&dir, Some("personne"));
        assert!(g.contains("\"agents\":{}"), "vide:{g}");
        assert!(g.contains("\"compteurs\":"), "globales:{g}");
        let _ = std::fs::remove_dir_all(dir.racine());
    }

    #[test]
    fn piste_filtre_deja_normalise() {
        let dir = bac("piste");
        let spans = "r1|1000|muse|ordre_cree|O-1|ok\nr1|1001|b|ordre_ack|O-2|lu\n";
        let cible = dir.joindre("piste.log");
        std::fs::write(&cible, spans).expect("piste");
        let run = "\"run_id\":\"r1\",\"epoque\":1000";
        let verdict = "\"verdict\":\"lu\"";
        let tous = piste_json(&dir, None, None);
        assert!(tous.contains(run), "tous:{tous}");
        assert!(tous.contains("\"action\":\"ordre_ack\""), "tous:{tous}");
        let par_ordre = piste_json(&dir, Some("O-2"), None);
        assert!(!par_ordre.contains("O-1"), "ordre:{par_ordre}");
        let par_agent = piste_json(&dir, None, Some("b"));
        assert!(!par_agent.contains("O-1"), "agent:{par_agent}");
        assert!(par_agent.contains(verdict), "agent:{par_agent}");
        let _ = std::fs::remove_dir_all(dir.racine());
    }

    #[test]
    fn drapeau_preserve_les_positionnels() {
        let args = ["pull", "--json", "alice"].map(str::to_string);
        let gardes = ["pull", "alice"].map(str::to_string);
        assert!(est_drapeau_json(&args));
        assert!(!est_drapeau_json(&["list".to_string()]));
        assert_eq!(sans_json(&args), gardes);
        assert!(sans_json(&[]).is_empty());
    }
}
