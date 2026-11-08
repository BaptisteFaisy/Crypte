---
inclusion: always
---

# Crypte — présentation produit

**kiro-horror-tui** : un chat LLM dans une crypte hantée, TUI plein écran
pensée pour **Ghostty**. Terminal ≥ 80x24 requis. UI et messages **en anglais**.

## Ce qu'est le produit

- Multi-chats LLM (client OpenAI-compatible, `--muse-auth` via le store Muse),
  chaque chat a son modèle (`/model`) ; niveau de raisonnement global (`/thinking`).
- **Hub des esprits** : chaque chat est un fantôme blanc dans une crypte
  isométrique (haut du panneau droit, affichage seul) ; les sous-agents sont
  des petits fantômes noirs ; F5 = tout le monde fuit.
- **Kiro la mascotte** : grand fantôme blanc qui hante l'écran chat et critique
  chaque prompt (même modèle que le chat, ≤ 25 mots, sarcastique) ; cliquer =
  le frapper (`/critic off` pour le reposer).
- **Apparitions** qui réagissent au chat (chaudron, araignée, main du tombeau…),
  effets horreur (vignette chaude, brouillard, éclairs), **jauge de peur** `fear`
  (% de la fenêtre de contexte), compteur global de tokens, **almanach**
  tokens/jour persisté (`/usage`).
- **Bestiaire** : 33 créatures + Kiro effrayé, pixel art 32×32 redessiné
  (vraies images via protocole graphique Kitty, `/bestiary`).
- **Boutique** (F6) : 1 token consommé = 1 pièce, 5 bonus d'ambiance.
- **Haunt** (F9) : jeu de plateformes/tir original dans le style bestiaire.
- **Sous-agents** `/agent`, bus inter-chats (`/tell`, `/msg`, `/link`),
  compaction `/compact`, recherche web `/web`, outils agent navigateur/bureau/shell.
- **Compat Kiro** : lit les mêmes `.kiro/` que Kiro (steering, specs, hooks,
  MCP, agents customs, skills, mémoire) — un projet va et vient entre les deux.

## Ce que le produit n'est pas

- Pas de son : jamais de BEL, jamais de notification.
- Pas un IDE : pas d'édition de fichiers, pas de terminal intégré.
- Le TUI démarre même sans clé ni réseau (mode déconnecté, `/connect`).

Référence : #[[file:README.md]]
