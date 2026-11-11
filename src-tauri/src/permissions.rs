//! Permissions des outils : mode global + règles fines par outil.
//!
//! `yolo` (défaut) = exécute sans demander, `auto` = demande confirmation
//! à chaque modification. Persisté dans
//! `$HOME/.config/kiro-horror-tui/permissions` (un mot, `yolo` si absent,
//! illisible ou inconnu) ; relu au démarrage.
//!
//! Règles fines (T5) : `allow` (exécute), `ask` (confirme), `deny` (refuse
//! sans demander), par motif exact (`shell_run`) ou préfixe (`mcp_github_*`,
//! seul le `*` final compte) ; le plus long motif qui matche gagne, sinon
//! le mode global s'applique. Persistées dans
//! `$HOME/.config/kiro-horror-tui/permissions-rules` (lignes
//! `<motif> <règle>).
//!
//! Types purs uniquement : le handler `/permissions` et la porte des
//! outils (`impl App`) vivent dans [`crate::permissions_cmds`] (plafond
//! 500 lignes).

use std::path::{Path, PathBuf};

/// Nom du fichier d'état (dossier : `crate::paths::config_dir`).
const STATE_FILE: &str = "permissions";
/// Nom du fichier des règles fines (même dossier que le mode).
const RULES_FILE: &str = "permissions-rules";

/// Mode de permissions (`Yolo` par défaut : exécute sans demander).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    Auto,
    #[default]
    Yolo,
}

impl Mode {
    /// Parse insensible à la casse et aux espaces (`None` si inconnu).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "yolo" => Some(Self::Yolo),
            _ => None,
        }
    }

    /// Étiquette affichée et persistée (`auto` / `yolo`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Yolo => "yolo",
        }
    }

    /// Comportement en une phrase (affiché par `/permissions`, en anglais).
    pub fn describe(self) -> &'static str {
        match self {
            Self::Auto => "asks confirmation before every change",
            Self::Yolo => "runs everything without asking",
        }
    }
}

/// Règle par outil (`allow` = exécute, `ask` = confirme, `deny` = refuse).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    Allow,
    Ask,
    Deny,
}

impl Rule {
    /// Parse insensible à la casse et aux espaces (`None` si inconnu).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "allow" => Some(Self::Allow),
            "ask" => Some(Self::Ask),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }

    /// Étiquette affichée et persistée (`allow` / `ask` / `deny`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Deny => "deny",
        }
    }
}

/// Décision effective pour un appel d'outil (règle ou défaut du mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Ask,
    Deny,
}

/// Règles par outil (motif exact ou préfixe `*`, ordre d'insertion).
#[derive(Debug, Clone, Default)]
pub struct Rules {
    pub(crate) entries: Vec<(String, Rule)>,
}

impl Rules {
    /// Pose une règle (remplace celle du même motif, sinon ajoute).
    pub fn set(&mut self, pattern: &str, rule: Rule) {
        if let Some(entry) = self.entries.iter_mut().find(|(p, _)| p == pattern) {
            entry.1 = rule;
        } else {
            self.entries.push((pattern.to_string(), rule));
        }
    }

    /// Retire la règle du motif (`permissions_set_rule` avec `null`).
    pub fn remove(&mut self, pattern: &str) {
        self.entries.retain(|(p, _)| p != pattern);
    }

    /// Décision pour `name` : règle du plus long motif qui matche (`*`
    /// final = préfixe, ex æquo = première posée), sinon défaut du mode
    /// global ([`crate::tools::needs_confirm`] : `ask` ou `allow`).
    pub fn decide(&self, mode: Mode, name: &str) -> Decision {
        let mut best: Option<(usize, Rule)> = None;
        for (pattern, rule) in &self.entries {
            let hit = match pattern.strip_suffix('*') {
                Some(prefix) => name.starts_with(prefix),
                None => name == pattern,
            };
            if hit && best.map(|(len, _)| pattern.len() > len).unwrap_or(true) {
                best = Some((pattern.len(), *rule));
            }
        }
        match best.map(|(_, rule)| rule) {
            Some(Rule::Allow) => Decision::Allow,
            Some(Rule::Ask) => Decision::Ask,
            Some(Rule::Deny) => Decision::Deny,
            None if crate::tools::needs_confirm(mode, name) => Decision::Ask,
            None => Decision::Allow,
        }
    }
}

/// État T5 porté par [`App`] : règles fines + chemins résolus (env et
/// maison lus une fois, à la construction ; les tests écrivent les champs
/// directement, sans toucher à l'environnement).
pub struct T5State {
    pub rules: Rules,
    pub rules_path: Option<PathBuf>,
    pub kiro_project: PathBuf,
    pub kiro_global: Option<PathBuf>,
    pub checkpoint_dir: Option<PathBuf>,
}

impl T5State {
    /// Résout tout (règles + dirs Kiro) depuis env/maison.
    pub fn load() -> Self {
        Self {
            rules: rules_path()
                .map(|p| load_rules_from(&p))
                .unwrap_or_default(),
            rules_path: rules_path(),
            kiro_project: crate::kiro::project_dir(),
            kiro_global: crate::kiro::global_dir(),
            checkpoint_dir: crate::kiro::checkpoint::dir(),
        }
    }
}

/// Chemin du fichier d'état (`None` si `HOME` manquant).
pub fn path() -> Option<PathBuf> {
    crate::paths::config_dir().map(|dir| dir.join(STATE_FILE))
}

/// Chemin du fichier des règles (`None` si `HOME` manquant).
pub fn rules_path() -> Option<PathBuf> {
    crate::paths::config_dir().map(|dir| dir.join(RULES_FILE))
}

/// Charge le mode persisté (`Yolo` si absent, illisible ou inconnu).
pub fn load() -> Mode {
    path().map(|p| load_from(&p)).unwrap_or_default()
}

/// Charge depuis `path` (`Yolo` si absent, illisible ou inconnu).
pub fn load_from(path: &Path) -> Mode {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| Mode::parse(&raw))
        .unwrap_or_default()
}

/// Charge les règles depuis `path` (vide si absent ; lignes `<motif>
/// <règle> ; blancs, `#` commentaires et malformés ignorés).
pub fn load_rules_from(path: &Path) -> Rules {
    let mut rules = Rules::default();
    let Ok(raw) = std::fs::read_to_string(path) else {
        return rules;
    };
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        if let (Some(pattern), Some(word), None) = (words.next(), words.next(), words.next()) {
            if let Some(rule) = Rule::parse(word) {
                rules.set(pattern, rule);
            }
        }
    }
    rules
}

/// Persiste `mode` dans `path` (crée le dossier parent si besoin).
pub fn save_to(path: &Path, mode: Mode) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, format!("{}\n", mode.label()))
}

/// Persiste `rules` dans `path` (crée le dossier parent si besoin).
pub fn save_rules_to(path: &Path, rules: &Rules) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = String::new();
    for (pattern, rule) in &rules.entries {
        out.push_str(&format!("{pattern} {}\n", rule.label()));
    }
    std::fs::write(path, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire unique par test (pas de dep `tempfile`).
    fn tmp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kh-perms-{}-{name}", std::process::id()))
    }

    #[test]
    fn parse_accepte_yolo_auto_insensible_casse() {
        assert_eq!(Mode::parse("yolo"), Some(Mode::Yolo));
        assert_eq!(Mode::parse("  AUTO\n"), Some(Mode::Auto));
        assert_eq!(Mode::parse("turbo"), None);
        assert_eq!(Mode::parse(""), None);
        assert_eq!(Mode::default(), Mode::Yolo);
        assert_eq!(Mode::Yolo.label(), "yolo");
        assert_eq!(Mode::Auto.label(), "auto");
        assert_eq!(Rule::parse("ALLOW"), Some(Rule::Allow));
        assert_eq!(Rule::parse("ask"), Some(Rule::Ask));
        assert_eq!(Rule::parse("deny "), Some(Rule::Deny));
        assert_eq!(Rule::parse("maybe"), None);
        assert_eq!(Rule::Deny.label(), "deny");
    }

    #[test]
    fn roundtrip_fichier_et_inconnu_vaut_yolo() {
        let dir = tmp_dir("roundtrip");
        let path = dir.join("sous").join("permissions");
        save_to(&path, Mode::Auto).unwrap();
        assert_eq!(load_from(&path), Mode::Auto);
        assert_eq!(load_from(&dir.join("absent")), Mode::Yolo);
        std::fs::write(&path, "turbo\n").unwrap();
        assert_eq!(load_from(&path), Mode::Yolo);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn regles_match_decide_et_fichier() {
        let mut rules = Rules::default();
        assert!(rules.entries.is_empty());
        // Sans règle : défaut du mode (`needs_confirm` inchangé).
        assert_eq!(rules.decide(Mode::Auto, "shell_run"), Decision::Ask);
        assert_eq!(rules.decide(Mode::Yolo, "shell_run"), Decision::Allow);
        assert_eq!(rules.decide(Mode::Auto, "browser_read"), Decision::Allow);
        rules.set("shell_run", Rule::Deny);
        rules.set("mcp_*", Rule::Deny);
        rules.set("mcp_github_*", Rule::Allow);
        assert!(!rules.entries.is_empty());
        // L'explicite bat le mode, le préfixe long bat le court.
        assert_eq!(rules.decide(Mode::Yolo, "shell_run"), Decision::Deny);
        assert_eq!(rules.decide(Mode::Auto, "mcp_github_read"), Decision::Allow);
        assert_eq!(rules.decide(Mode::Auto, "mcp_other_x"), Decision::Deny);
        assert_eq!(rules.decide(Mode::Auto, "shell_exec"), Decision::Ask);
        // Reposer remplace (pas de doublon).
        rules.set("shell_run", Rule::Allow);
        assert_eq!(rules.decide(Mode::Auto, "shell_run"), Decision::Allow);
        // Fichier : blancs, commentaires et malformés ignorés.
        let dir = tmp_dir("rules");
        let path = dir.join("permissions-rules");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            &path,
            "# gloss\n\nshell_run deny\nmcp_github_* allow\nbogus\nx y z\n",
        )
        .unwrap();
        let file = load_rules_from(&path);
        assert_eq!(file.decide(Mode::Yolo, "shell_run"), Decision::Deny);
        assert_eq!(file.decide(Mode::Auto, "mcp_github_read"), Decision::Allow);
        assert!(load_rules_from(&dir.join("absent")).entries.is_empty());
        let back = dir.join("copy");
        save_rules_to(&back, &file).unwrap();
        let reloaded = load_rules_from(&back);
        assert_eq!(reloaded.decide(Mode::Yolo, "shell_run"), Decision::Deny);
        assert_eq!(
            reloaded.decide(Mode::Auto, "mcp_github_read"),
            Decision::Allow
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
