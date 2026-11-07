# kiro-horror-tui — contexte agents

Chat LLM dans une crypte hantée (TUI ratatui) pensé pour Ghostty :
multi-chats, mascotte fantôme Kiro (F5), bestiaire pixel art (F6, images
Kitty), effets horreur, compteur tokens, sous-agents `/agent`,
compaction `/compact`. UI et messages **en anglais**.

## Commandes

```sh
cargo build                      # debug, 0 warning exigé
cargo test                       # tous les tests doivent être verts
cargo fmt --check                # formatage propre exigé
./target/debug/kiro-horror-tui --smoke   # headless 80x24, sans réseau ni clé
./target/debug/kiro-horror-tui --planche docs/bestiaire.png  # après retouche d'un sprite
./target/debug/kiro-horror-tui --apercu /tmp/crypte.ans      # 3 écrans en ANSI
./target/debug/kiro-horror-tui --muse-auth --once "Reply with exactly: OK"
cargo build --release            # met à jour `kiro-horror-tui` (symlink)
```

## Archi (`src/`)

`main.rs` (CLI + bootstrap), `app.rs` (état + boucle 10 ms, tick 100 ms),
`app/ctors.rs` (constructeurs),
`bus.rs` (adressage, files, liens auto, plafond 8 relais),
`mouse.rs` (clics : [+] = nouveau chat, lignes sidebar = choisir,
onglets = écrans),
`drain.rs` (routage réponses async + pompes + retransmissions),
`cmds.rs` (handlers `/…`), `commands.rs` (parse + aide + usage),
`permissions.rs` (mode yolo/auto + persistance fichier + handler),
`persist.rs` (chats : autosave JSON ~5 s + sortie, restauration silencieuse,
fichier illisible préservé en `.corrupt`),
`connect.rs` (flow `/connect` : clé masquée → modèle → provider),
`history.rs` (fenêtre d'envoi + nommage des chats),
`sessions.rs` + `sessions/` (chats `chat.rs` + sous-agents `subagents.rs`),
`sidebar.rs` (colonne cercueils + bouton [+]), `chat.rs` (historique +
saisie + prompt custom masquable), `provider.rs` (client OpenAI-compatible, timeout 60 s),
`tokens.rs` (estimation tiktoken + ledger), `agents.rs` (ce contexte :
chargement, pin, compaction pure), `mascot.rs` (fantôme), `fx.rs`
(overlay horreur), `app/ui.rs` (mise en page), `theme.rs` (palette
unique), `bestiary.rs` (sprites), `gfx.rs` (images), `apparition.rs`
(panneau droit), `galerie.rs` (écran F6), `ghostty.rs` (OSC + mode 2026),
`hub.rs` + `hub/` (hub des esprits F7 : simulation en dalles, `iso.rs`
crypte isométrique, `render.rs` scène, `live.rs` image Kitty par frame,
`font.rs` police 3×5, `social.rs` orbes et bavardages), `app/esprits.rs`
(écran + touches du hub). Sprites du hub : `assets/hub/*.txt`.
`web/` (`/web` : backends → fetch → cache → synthèse, parsing pur dans
`web/parse.rs`).
`ghostty/` : thème, shader et lanceur `ghostty/crypte`.

Flux : `send` (user → provider, tâche async) → `drain_replies`
(`Incoming::Chat|Agent|Compact|Web`) → ledger + historique + affichage.
Bus : `/tell`/`/msg` déposent (affiché aussitôt, déclenché si libre,
enfilé sinon, pompé en groupe à la tombée) ; `/agent @<n>` livre ailleurs
(`Incoming::Agent(propriétaire, id, livraison)`, tag `[agent#id from Chat k]`,
statut sur le propriétaire, ledger + versement contexte sur la livraison
(rôle user, taggé, sans déclencher) ; `/ask` poursuit
l'échange agent (enfilé si en cours, 20 messages max) ; `/link` retransmet
chaque réponse en éventail (`[auto …]`, 8 relais max). Chaque chaîne reste
d'origine user (commande ou lien) : pas de boucles infinies.
`/compact` résume via le provider puis remplace l'historique par
`[agents.md pinné?]` + `[résumé]` + 4 derniers (voir
`agents::compacted_history`, pure et testée).
`/web` cherche (Tavily → Brave → DuckDuckGo → Wikipedia FR+EN) +
fetch top-3 parallèle, synthèse LLM avec sources `[n]`
(`Incoming::Web`, tag `[web:backend]`, brut si déconnecté,
historique inchangé).

## Conventions

- Fichier : 500 lignes max ET 30 fonctions max (tests inclus) ;
  fonction < 50 lignes. Au-delà : découper en modules, jamais
  laisser grossir. Vérifier : `wc -l` et
  `grep -cE 'fn [a-zA-Z_][a-zA-Z0-9_]* *\('` sur les fichiers touchés.
- Zéro duplication : extraire et partager les helpers, une seule
  implémentation par logique (`compacted_history`, `without_pinned`,
  `usage_line` vivent dans `agents.rs`).
- Couleurs : uniquement via `theme.rs` (jamais de `Color::Rgb` ailleurs,
  sauf les fonds RGBA de `gfx.rs` et `bestiary.rs`, documentés = `theme::VOID`).
- Pixel art : grilles `assets/pixel/*.txt` 32×32, lettres de palette
  (voir README) ; mise à l'échelle **entière** au plus proche voisin
  seulement (`bestiary::fit`), jamais de filtre lissant : la netteté est
  testée (`gfx::tests`). Après retouche : régénérer `docs/bestiaire.png`.
- Effets (`fx.rs`) : jamais sur le texte ni dans les zones d'images
  (`ImageBank::drawn`). Aucun son (pas de BEL, pas de notification).
- UI et messages en anglais ; clé API **jamais affichée ni logguée**
  (ni chat, ni log, ni test — les tests réseau lisent le store).
- `AGENTS.md` chargé une fois au démarrage (`--agents-file` prime,
  sinon `./AGENTS.md` puis `./AGENT.md`, restart pour recharger) ;
  injecté en tête de chaque chat + `/agent` ; exclu de `/compact`
  (résumé méta) et de `--once` (brut).
