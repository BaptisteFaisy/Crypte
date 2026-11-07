# kiro-horror-tui

Chat LLM dans une crypte hantée : mascotte fantôme Kiro immobile
(s'effraie via F5),
effets horreur (static, gouttes de sang, peur 0-100), compteur
de tokens, écran tokens dédié. Terminal **≥ 80x24** requis.

## Lancer

```sh
cargo build
KH_API_KEY=... KH_MODEL=... ./target/debug/kiro-horror-tui
```

Appel unique non-interactif (affiche la réponse + une ligne tokens) :

```sh
KH_API_KEY=... KH_MODEL=... ./target/debug/kiro-horror-tui --once "<texte>"
./target/debug/kiro-horror-tui --muse-auth --once "<texte>"
```

Contrôle sans réseau ni clé (rendu headless 80x24) :

```sh
./target/debug/kiro-horror-tui --smoke
./target/debug/kiro-horror-tui --help
```

## Muse Code

Prérequis : CLI `muse` loggée. Branchement en 1 commande :

```sh
./target/debug/kiro-horror-tui --muse-auth
```

`--muse-auth` lit la clé dans `~/.config/muse/auth.json` (champ
`providers.meta.api_key`), base défaut `https://api.meta.ai/v1`, modèle
défaut `muse-spark-1.3-contributor`. `KH_BASE_URL` / `KH_API_KEY` /
`KH_MODEL` restent prioritaires et surchargent ces défauts. Utilisable
avec le TUI et avec `--once`. La clé n'est jamais affichée ni copiée.

## Config

Variables d'environnement (prioritaires) :

| var | rôle | défaut |
| --- | ---- | ------ |
| `KH_BASE_URL` | base OpenAI-compatible | `https://api.openai.com/v1` |
| `KH_API_KEY` | clé API (**obligatoire**, jamais logguée) | — |
| `KH_MODEL` | modèle (**obligatoire**) | — |

Avec `--muse-auth`, les défauts deviennent base `https://api.meta.ai/v1`
+ modèle `muse-spark-1.3-contributor` (clé lue dans `auth.json`).

Ou fichier TOML via `--config <fichier>` (clés plates ou section
`[provider]`, surchargées par l'environnement) :

```toml
base_url = "https://api.meta.ai/v1"
api_key = "..."
model = "muse-spark-1.3-contributor"
```

## Touches

| touche | effet |
| --- | ---- |
| Entrée | envoyer le message |
| Échap / `q` (saisie vide) | quitter |
| F2 | basculer écran chat / tokens |
| F3 / F4 | chat précédent / suivant |
| Ctrl+N | nouveau chat |
| F5 | spook (fantôme effrayé 2 s, peur +) |

## Modèle

Chaque chat a son modèle (`None` = défaut du provider).
`/model` affiche le modèle effectif, `/model <nom>` le change
pour le chat courant, `/model reset` (ou `default`) revient
au défaut. La barre de statut montre le nom court (après
le dernier `/`, 14 caractères max).

## Thinking

Niveau de réflexion global (envoyé comme `reasoning_effort`
**uniquement** si `/thinking <niveau>` a été utilisé, sinon
le champ est omis du JSON). `/thinking` affiche le niveau,
`/thinking default` revient au défaut (omis). Niveaux :
`default` / `low` / `medium` / `high`.

## Sous-agents

`/agent <texte>` lance une requête annexe (sans historique mais avec
agents.md préfixé si chargé, modèle et thinking courants) ; la réponse
arrive en async préfixée `[agent#id]` et le statut est visible dans la
sidebar (`A{n}`) puis via `/agents` (`en cours` / `terminé`
/ `échoué`, tokens consommés).

## Compact

`/compact` (6 messages minimum) demande un résumé FR (< 20
lignes, agents.md **exclu** du résumé méta) et remplace l'historique
par `[agents.md pinné]` + `résumé + 4 derniers messages` ; les
messages arrivés pendant le résumé sont préservés. En cas d'échec,
l'historique reste intact.

## AGENTS.md (contexte système)

Au démarrage, le binaire charge `./AGENTS.md` puis `./AGENT.md` dans
le cwd (`--agents-file <fichier>` prime sur les deux), une seule fois
(restart pour recharger). Injecté comme message `system` en tête de
l'historique de chaque chat (nouveaux inclus, `/new` et Ctrl+N) et
préfixé aux appels `/agent`. Exclu de `/compact` (résumé méta) et de
`--once` (brut). Annonce au démarrage (`AGENTS.md chargé, N lignes,
~M tokens`) et ligne dédiée dans `/usage` (`chargé` ou `absent`).

## Modules (`src/`)

`main.rs` (CLI + bootstrap), `app.rs` (état + boucle 100 ms),
`chat.rs` (historique + saisie), `mascot.rs` (fantôme),
`fx.rs` (overlay horreur), `tokens.rs` (compteur tiktoken),
`provider.rs` (client OpenAI-compatible, timeout 60 s),
`sessions.rs` (chats multiples + sous-agents + sidebar),
`commands.rs` (parse des commandes `/…` + aide + usage),
`cmds.rs` (handlers `/model` `/thinking` `/agent` `/agents` `/compact`),
`agents.rs` (chargement agents.md + compaction pure testée).
