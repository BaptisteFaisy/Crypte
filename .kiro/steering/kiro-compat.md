---
inclusion: fileMatch
fileMatchPattern:
  - "src/kiro/**"
---

# Crypte — moteur de compat `.kiro/`

Crypte lit les mêmes `.kiro/` que Kiro (projet `./.kiro/` + global `~/.kiro/`,
complet T1–T5) : steering (`steering.rs`), specs (`spec*.rs`, `/spec new` =
3 appels chaînés requirements → design → tasks, `run` via sous-agents),
hooks (`hook_*.rs`, trigger + regex `matcher` + `.kiroignore`, `command` =
`sh -c` async 30 s, `agent` = prompt préfixé au prochain envoi), MCP
(`mcp*.rs`, outils seuls, `mcp_<serveur>_<outil>`), agents customs
(`agents_*.rs`, format Kiro officiel `.json`/`.md`, `team/<nom>`, projet >
global, json > md ; `tools`/`mcpServers` lus et affichés, non exécutés),
skills (`skills*.rs`, `/skill` transitoire), checkpoints (`checkpoint*.rs`,
anneau 20 via `write_tracked`), mémoire (`memory*.rs`, pin après steering).

## Règles de test strictes

- Chemins **injectés uniquement** : jamais `set_var` (un seul site historique,
  `spec_bout_en_bout`, fixture sans steering ni MCP).
- `KH_KIRO_*` = overrides manuels du binaire uniquement.
- En test : global = `None` par défaut (jamais le vrai `~/.kiro`).
- Steering `always` + mémoire fusionnés au pin agents.md
  (voir `agents::load_merged_in`) ; `.kiroignore` : dernier motif qui matche
  gagne, `!` ré-inclut, `dir/` = sous-arbre.
- Specs existantes jamais écrasées ; tâche en échec = run arrêté, rien re-tenté ;
  hooks boot = single-fire, garde anti-récursion profondeur 1.
