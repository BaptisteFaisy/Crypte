//! Powers Kiro (plugins `~/.kiro/powers/installed/*/`, global seul).
//!
//! Format Agent Plugins 1.0.0 (agent-plugins.org) : `plugin.json`
//! (manifeste) + `skills/` ; repli legacy `POWER.md`. Phase 1 :
//! lecture + activation des skills par mots-clés (le MCP embarqué
//! attend la phase 2 ; les powers cloud ne sont pas accessibles
//! localement, pas d'API publique).

use std::path::Path;

use crate::kiro::skills::Skill;

/// Manifeste `plugin.json` (champs inconnus ignorés, spec ouverte).
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Manifest {
    /// Id interne (renommer casse les installs, doc).
    #[serde(default)]
    pub name: String,
    /// Version du plugin.
    #[serde(default)]
    pub version: String,
    /// Description affichée par `/powers`.
    #[serde(default)]
    pub description: String,
    /// Déclencheurs d'activation.
    #[serde(default)]
    pub keywords: Vec<String>,
}

/// Power chargé (skills + marqueurs phase 2).
#[derive(Debug, Clone)]
pub struct Power {
    /// Nom (manifeste, sinon dossier).
    pub name: String,
    /// Version (manifeste, vide en legacy).
    pub version: String,
    /// Description (manifeste ou legacy).
    pub description: String,
    /// Mots-clés d'activation.
    pub keywords: Vec<String>,
    /// Skills embarqués (`skills/*/SKILL.md`, ou corps legacy).
    pub skills: Vec<Skill>,
    /// `mcp.json` présent (phase 2, affiché seulement).
    pub has_mcp: bool,
    /// Format legacy `POWER.md` (sans `plugin.json`).
    pub legacy: bool,
}

/// Charge les powers installés (global seul, parité Kiro ; triés).
pub fn load_all(global: Option<&Path>) -> Vec<Power> {
    let Some(global) = global else {
        return Vec::new();
    };
    let root = global.join("powers/installed");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
        .into_iter()
        .filter_map(|name| load_one(&root.join(name)))
        .collect()
}

/// Charge un power (`plugin.json`, sinon `POWER.md`, sinon rien).
fn load_one(dir: &Path) -> Option<Power> {
    let fallback = dir.file_name()?.to_string_lossy().into_owned();
    let manifest = std::fs::read_to_string(dir.join("plugin.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<Manifest>(&raw).ok());
    match manifest {
        Some(manifest) => Some(Power {
            name: if manifest.name.trim().is_empty() {
                fallback
            } else {
                manifest.name.clone()
            },
            version: manifest.version.clone(),
            description: manifest.description.clone(),
            keywords: manifest
                .keywords
                .iter()
                .map(|k| k.trim().to_string())
                .collect(),
            skills: crate::kiro::skills::load_dir(&dir.join("skills")),
            has_mcp: dir.join("mcp.json").is_file(),
            legacy: false,
        }),
        None => load_legacy(dir, &fallback),
    }
}

/// Repli legacy : frontmatter `POWER.md` + corps = skill unique.
fn load_legacy(dir: &Path, fallback: &str) -> Option<Power> {
    let raw = std::fs::read_to_string(dir.join("POWER.md")).ok()?;
    let (front, body) = crate::kiro::frontmatter::split(&raw);
    let front = front.unwrap_or("");
    let description = crate::kiro::frontmatter::value(front, "description").unwrap_or_default();
    let keywords = crate::kiro::frontmatter::value(front, "keywords")
        .map(|line| split_keywords(&line))
        .unwrap_or_default();
    let skill = Skill {
        name: fallback.to_string(),
        description: description.clone(),
        body: body.to_string(),
    };
    Some(Power {
        name: fallback.to_string(),
        version: String::new(),
        description,
        keywords,
        skills: vec![skill],
        has_mcp: false,
        legacy: true,
    })
}

/// Mots-clés legacy (`[a, b]` ou `a b`, insensibles aux crochets).
fn split_keywords(line: &str) -> Vec<String> {
    line.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split([',', ' ', '\t', '\n'])
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// Vrai si un mot-clé apparaît dans la demande : mot seul = token
/// exact (pas de sous-chaîne parasite), expression = sous-chaîne,
/// casse ignorée des deux côtés.
pub fn matches(power: &Power, prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    let tokens: Vec<&str> = lower
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    power.keywords.iter().any(|keyword| {
        let keyword = keyword.trim().to_lowercase();
        if keyword.is_empty() {
            return false;
        }
        if keyword.contains(' ') {
            lower.contains(&keyword)
        } else {
            tokens.contains(&keyword.as_str())
        }
    })
}

/// Blocs à injecter (`power/skill → corps`) pour la demande.
pub fn activation_blocks(powers: &[Power], prompt: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for power in powers.iter().filter(|power| matches(power, prompt)) {
        for skill in &power.skills {
            out.push((power.name.clone(), skill.name.clone(), skill.body.clone()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Racine globale jouet (un power moderne + un legacy, unique/test).
    fn fixture(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("kh-powers-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let modern = dir.join("powers/installed/db");
        std::fs::create_dir_all(modern.join("skills/indexes")).unwrap();
        std::fs::write(
            modern.join("plugin.json"),
            r#"{"name": "db", "version": "1.0.0", "description": "Bases.",
                "keywords": ["postgres", "pull request"], "author": {"name": "x"}}"#,
        )
        .unwrap();
        std::fs::write(modern.join("mcp.json"), r#"{"mcpServers": {}}"#).unwrap();
        std::fs::write(
            modern.join("skills/indexes/SKILL.md"),
            "---\nname: indexes\ndescription: Indexe malin.\n---\nCorps index.",
        )
        .unwrap();
        let legacy = dir.join("powers/installed/vieux");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(
            legacy.join("POWER.md"),
            "---\ndescription: Vieux savoir.\nkeywords: [savoir, grimoire]\n---\nCorps vieux.",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("powers/installed/vide")).unwrap();
        dir
    }

    /// Chargement : moderne + legacy, vide ignoré, trié.
    #[test]
    fn load_moderne_legacy_et_vide() {
        let dir = fixture("load");
        let powers = load_all(Some(&dir));
        assert_eq!(
            powers.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            vec!["db", "vieux"]
        );
        assert_eq!(powers[0].skills.len(), 1);
        assert_eq!(powers[0].version, "1.0.0");
        assert!(powers[0].has_mcp);
        assert!(!powers[0].legacy);
        assert_eq!(powers[1].skills.len(), 1);
        assert!(powers[1].legacy);
        assert!(load_all(None).is_empty());
        assert!(load_all(Some(&dir.join("absent"))).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Matching : token exact, expression incluse, casse ignorée.
    #[test]
    fn matches_token_expression_casse() {
        let dir = fixture("match");
        let powers = load_all(Some(&dir));
        assert!(matches(&powers[0], "Tune POSTGRES here"));
        assert!(matches(&powers[0], "open a pull request"));
        assert!(!matches(&powers[0], "gorgeous postgresqle"));
        assert!(!matches(&powers[0], "bonjour"));
        assert!(matches(&powers[1], "quel savoir ?"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Blocs : un par skill des powers matchés, texte taggé.
    #[test]
    fn activation_blocks_un_par_skill() {
        let dir = fixture("blocs");
        let powers = load_all(Some(&dir));
        let blocks = activation_blocks(&powers, "tune postgres");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].0, "db");
        assert_eq!(blocks[0].1, "indexes");
        assert!(blocks[0].2.contains("Corps index."));
        assert!(activation_blocks(&powers, "bonjour").is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
