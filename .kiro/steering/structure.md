---
inclusion: always
---

# Crypte — structure et conventions

## Règles de taille (bloquantes)

- Fichier source : **500 lignes max ET 30 fonctions max** (tests inclus ;
  compter par `grep -cE 'fn [a-zA-Z_][a-zA-Z0-9_]* *\('`).
- Fonction : **moins de 50 lignes**.
- Au-delà : découper en modules, jamais laisser grossir. Vérifier `wc -l` +
  comptage `fn` sur chaque fichier touché avant de rendre.

## Conventions

- **Zéro duplication** : une seule implémentation par logique ; extraire et
  partager les helpers (`compacted_history`, `without_pinned`, `usage_line`
  vivent dans `agents.rs`).
- **Couleurs uniquement via `theme.rs`** : jamais de `Color::Rgb` ailleurs
  (sauf fonds RGBA de `gfx.rs`/`bestiary.rs`, documentés = `theme::VOID`).
- **UI et messages en anglais.** Le code et les commentaires acceptent le
  français ; les chaînes visibles par l'utilisateur sont en anglais.
- **Effets (`fx.rs`)** : jamais sur le texte ni dans les zones d'images
  (`ImageBank::drawn`). Aucun son.
- **Kiro en balade** : sprites `assets/pixel/kiro-*.txt`, arène = sidebar +
  discussion (jamais le panneau droit ni sous la saisie) ; aucune écriture
  dans le rectangle du sprite (placeholders Kitty).
- **Pixel art** : grilles `assets/pixel/*.txt` 32×32, lettres de palette
  (voir README) ; upscale **entier** au plus proche voisin seulement
  (`bestiary::fit`), jamais de filtre lissant.
- `AGENTS.md` chargé une seule fois au premier envoi, pinné en tête de chaque
  chat + `/agent`, exclu de `/compact` (résumé méta) et de `--once` (brut).

## Archi (`src/`, carte complète)

#[[file:AGENTS.md]]

Repères : `main.rs` (CLI + bootstrap) → `app.rs` (état + boucle) →
`sessions.rs` + `sessions/` (chats + sous-agents) ; `drain.rs` route les
réponses async ; `cmds*.rs` = handlers `/…` ; `chat/` = historique + saisie ;
`hub/` + `app/esprits.rs` = hub des esprits ; `haunt/` + `app/haunt.rs` =
jeu F9 ; `kiro/` = compat Kiro ; `tools/` = outils agent ; `web/` = `/web` ;
`settings/` = fournisseurs + coffre ; `shop.rs`, `almanac.rs`, `tokens.rs`,
`provider.rs`, `theme.rs`, `bestiary.rs`, `gfx.rs`, `canvas.rs`, `fx.rs`,
`ghostty.rs`, `bus.rs`, `mouse.rs`, `scroll.rs`, `history.rs`, `question.rs`,
`permissions*.rs`, `persist.rs`, `connect.rs`, `vps.rs`, `workspace.rs`.
