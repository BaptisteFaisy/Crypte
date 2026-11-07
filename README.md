# kiro-horror-tui

Chat LLM dans une crypte hantée, pensé pour **Ghostty** : bestiaire de
33 créatures en pixel art (images réelles via le protocole graphique
Kitty), mascotte fantôme Kiro immobile (s'effraie via F5), apparitions
qui réagissent au chat, effets horreur (vignette à la bougie, brume,
static, gouttes de sang, éclair, peur 0-100), compteur de tokens, écran
grimoire dédié. Terminal **≥ 80x24** requis.

![Le bestiaire de la crypte](docs/bestiaire.png)

## Lancer

Dans Ghostty, habillé (thème + shader, compile la release au besoin) :

```sh
KH_API_KEY=... KH_MODEL=... ghostty/crypte
ghostty/crypte --muse-auth
ghostty/crypte --sans-shader --muse-auth
```

Dans le terminal courant :

```sh
cargo build
KH_API_KEY=... KH_MODEL=... ./target/debug/kiro-horror-tui
```

Appel unique non-interactif (affiche la réponse + une ligne tokens) :

```sh
KH_API_KEY=... KH_MODEL=... ./target/debug/kiro-horror-tui --once "<texte>"
./target/debug/kiro-horror-tui --muse-auth --once "<texte>"
```

Contrôle sans réseau ni clé (rendu headless 80x24), aperçus :

```sh
./target/debug/kiro-horror-tui --smoke
./target/debug/kiro-horror-tui --help
./target/debug/kiro-horror-tui --planche docs/bestiaire.png   # PNG du bestiaire
./target/debug/kiro-horror-tui --apercu /tmp/crypte.ans       # 3 écrans en ANSI
cat /tmp/crypte.ans                                           # à lire dans Ghostty
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
| F2 | basculer chat / grimoire (tokens + âmes par chat) |
| F3 / F4 | chat précédent / suivant |
| Ctrl+N | nouveau chat |
| F5 | effroi : Kiro a peur 2 s, jump scare, éclair, peur + |
| F6 | basculer chat / bestiaire (flèches pour choisir) |

## Ghostty

Ce que la crypte utilise de Ghostty (et qui dégrade proprement ailleurs) :

- **Protocole graphique Kitty** (placeholders Unicode) : le pixel art est
  une vraie image, détectée au démarrage (`◆ kitty` dans la barre d'état).
  Ailleurs : demi-blocs truecolor (`◆ demi-blocs`).
- **Truecolor** partout : vignette à la bougie, brume, dégradés, éclair.
- **Sprites internes** : séparateurs powerline arrondis, blocs, braille
  dessinés au pixel près ; icônes Nerd Font embarquées.
- **OSC 11** (fond de fenêtre, padding compris), **OSC 2** (titre, pile
  restaurée en sortie), **OSC 9;4** (barre de progression native pendant
  qu'une réponse mijote, Ghostty seulement), **mode 2026** (rendu
  synchronisé, animations sans déchirure).
- `ghostty/crypte.conf` (palette, padding étendu, 120x36) +
  `ghostty/crypte.glsl` (vignette, flamme, grain — aucun flou : chaque
  pixel est lu à sa place).

Aucun son : ni cloche, ni notification.

## Bestiaire (pixel art)

33 créatures d'après le shot Dribbble « Halloween icons and
illustrations » (Getillustrations), **redessinées** en pixel art 32×32
(aucune image d'origine dans le dépôt), + Kiro effrayé. Une grille texte
par créature dans `assets/pixel/<slug>.txt` : 32 lignes de 32 lettres,
`.` = transparent, sinon une couleur de `src/theme.rs` :

| lettre | couleur | lettre | couleur |
| --- | --- | --- | --- |
| `W` | os | `S` | sable |
| `U` | terre | `K` | encre |
| `O` / `o` | citrouille / ombre | `R` / `r` | sang / séché |
| `G` / `g` | ectoplasme / ombre | `P` / `p` | spectre / ombre |
| `Y` / `y` | bougie / ombre | `N` | toile de jute |

Rendu net garanti : agrandissement **entier** au plus proche voisin, sur
un canevas de la taille pixel exacte de la zone (1 pixel image = 1 pixel
écran en Kitty, = 1 demi-cellule en demi-blocs). Le panneau droit montre
Kiro et une apparition : chaudron pendant une réponse, araignée pendant
les sous-agents, main de la tombe sur erreur, cercueil après `/compact`,
rotation lente sinon.

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

`/agent [@<n>] <texte>` lance une requête annexe (sans historique mais
avec agents.md préfixé si chargé, modèle et thinking courants) ; la
réponse arrive en async préfixée `[agent#id]` (ou `[agent#id de Chat k]`
si livrée ailleurs via `@<n>`) et le statut est visible dans la sidebar
(`A{n}`) puis via `/agents` (`en cours` / `terminé` / `échoué`, tokens
consommés). `/ask #<id> <texte>` poursuit la conversation avec un agent
(échange gardé, 20 messages max, tête `system` conservée) ; refusé si
l'agent est en cours. Chaque appel LLM reste déclenché par le user :
pas de boucles automatiques entre agents.

## Dialogues entre chats

`/tell <n> <texte>` injecte le texte dans le chat n (préfixé `[de Chat k]`)
et déclenche sa réponse (modèle du chat cible, thinking global) ; refusé
si le chat cible est en cours d'envoi, inexistant, ou si c'est le chat
courant. Combiné à `/agent @<n>`, ça fait se parler chats et sous-agents :
délégation (chat 1 → agent → réponse livrée au chat 2), relance (`/ask`),
concertation (`/tell` + réponse du chat cible).

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
`drain.rs` (routage des réponses async vers les chats),
`chat.rs` (historique + saisie), `mascot.rs` (fantôme),
`fx.rs` (overlay horreur), `tokens.rs` (compteur tiktoken),
`provider.rs` (client OpenAI-compatible, timeout 60 s),
`sessions.rs` (chats multiples + sous-agents + sidebar),
`commands.rs` (parse des commandes `/…` + aide + usage),
`cmds.rs` (handlers `/model` `/thinking` `/agent` `/agents` `/ask` `/tell` `/compact`),
`agents.rs` (chargement agents.md + compaction pure testée),
`app/ui.rs` (mise en page : en-tête, panneaux, barre powerline),
`theme.rs` (palette unique), `bestiary.rs` (sprites + mise à l'échelle
entière), `gfx.rs` (images Kitty / demi-blocs), `apparition.rs`
(créature du panneau droit), `galerie.rs` (écran bestiaire F6),
`ghostty.rs` (OSC, progression, rendu synchronisé).
