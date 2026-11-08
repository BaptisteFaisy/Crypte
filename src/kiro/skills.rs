//! Skills (`.kiro/skills/*/SKILL.md`, projet + global).
//!
//! Format Claude-compatible : frontmatter `name` + `description`, corps =
//! instructions injectées en tête du prochain envoi par `/skill` (transitoire,
//! annoncé). Sans `name` : nom du dossier. Les skills couvrent les prompts
//! réutilisables (pas de système séparé). À nom égal, le projet écrase le global.

use std::path::Path;

/// Skill : nom + description (liste `/skills`) + corps (injection `/skill`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
}

/// Parse un `SKILL.md` (`fallback` = nom du dossier, défaut du `name`).
pub fn parse(fallback: &str, raw: &str) -> Skill {
    let (front, body) = crate::kiro::frontmatter::split(raw);
    let mut skill = Skill {
        name: fallback.to_string(),
        description: String::new(),
        body: body.to_string(),
    };
    if let Some(front) = front {
        if let Some(name) = crate::kiro::frontmatter::value(front, "name") {
            if !name.trim().is_empty() {
                skill.name = name;
            }
        }
        if let Some(desc) = crate::kiro::frontmatter::value(front, "description") {
            skill.description = desc;
        }
    }
    skill
}

/// Charge un dossier de skills (un `SKILL.md` par sous-dossier, triés).
pub fn load_dir(dir: &Path) -> Vec<Skill> {
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        if entry.path().join("SKILL.md").is_file() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    names
        .into_iter()
        .filter_map(|name| {
            let raw = std::fs::read_to_string(dir.join(&name).join("SKILL.md")).ok()?;
            Some(parse(&name, &raw))
        })
        .collect()
}

/// Charge global puis projet (chemins injectés).
pub fn load_all_in(project: &Path, global: Option<&Path>) -> Vec<Skill> {
    let mut out: Vec<Skill> = Vec::new();
    if let Some(global) = global {
        out.extend(load_dir(&global.join("skills")));
    }
    for skill in load_dir(&project.join(".kiro/skills")) {
        if let Some(slot) = out.iter_mut().find(|each| each.name == skill.name) {
            *slot = skill;
        } else {
            out.push(skill);
        }
    }
    out
}

/// Retrouve un skill par son nom (match exact).
pub fn find<'a>(skills: &'a [Skill], name: &str) -> Option<&'a Skill> {
    skills.iter().find(|each| each.name == name)
}

/// Lignes `/skills` (`<nom> — <description>`, vide si aucun).
pub fn lines_of(skills: &[Skill]) -> Vec<String> {
    skills
        .iter()
        .map(|skill| {
            if skill.description.trim().is_empty() {
                format!("{} — (no description)", skill.name)
            } else {
                format!("{} — {}", skill.name, skill.description)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("kh-skills-{}-{name}", std::process::id()))
    }

    #[test]
    fn parse_frontmatter_nom_description_corps() {
        let skill = parse(
            "dir",
            "---\nname: terse\ndescription: Short replies.\n---\nBe brief.",
        );
        assert_eq!(skill.name, "terse");
        assert_eq!(skill.description, "Short replies.");
        assert_eq!(skill.body, "Be brief.");
        // Sans frontmatter : dossier + corps brut.
        let skill = parse("brut", "Just do it.");
        assert_eq!(skill.name, "brut");
        assert_eq!(skill.description, "");
        assert_eq!(skill.body, "Just do it.");
    }

    #[test]
    fn load_un_skill_md_par_sous_dossier_projet_ecrase() {
        let dir = tmp_dir("merge");
        let (global, project) = (dir.join("home/.kiro"), dir.join("proj"));
        std::fs::create_dir_all(global.join("skills/dup")).unwrap();
        std::fs::create_dir_all(project.join(".kiro/skills/dup")).unwrap();
        std::fs::create_dir_all(global.join("skills/solo")).unwrap();
        std::fs::create_dir_all(project.join(".kiro/skills/vide")).unwrap();
        std::fs::write(global.join("skills/solo/SKILL.md"), "global").unwrap();
        std::fs::write(global.join("skills/dup/SKILL.md"), "global dup").unwrap();
        std::fs::write(
            project.join(".kiro/skills/dup/SKILL.md"),
            "---\nname: dup\ndescription: D.\n---\nproject dup",
        )
        .unwrap();
        let skills = load_all_in(&project, Some(&global));
        assert_eq!(skills.len(), 2);
        assert_eq!(find(&skills, "solo").unwrap().body, "global");
        let dup = find(&skills, "dup").unwrap();
        assert_eq!(dup.body, "project dup");
        assert_eq!(dup.description, "D.");
        assert!(find(&skills, "vide").is_none());
        assert!(load_all_in(&dir.join("absent"), None).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
