# kiro-horror-tui — contexte agents

Chat LLM dans une crypte hantée (TUI ratatui) pensé pour Ghostty :
multi-chats, hub des esprits (panneau droit, affichage seul), Kiro la
mascotte en grand fantôme blanc qui hante l'écran chat et critique les
prompts (`/critic`, clic = le frapper), bestiaire pixel art (panneau
latéral + `/bestiary`, images Kitty), effets horreur, compteur tokens,
almanach tokens/jour (`/usage`), sous-agents `/agent`,
compaction `/compact`. UI et messages **en anglais**.

## Commandes

```sh
cargo build                      # debug, 0 warning exigé
cargo test                       # tous les tests doivent être verts
cargo fmt --check                # formatage propre exigé
./target/debug/kiro-horror-tui --smoke   # headless 80x24, sans réseau ni clé
./target/debug/kiro-horror-tui --planche docs/bestiaire.png  # après retouche d'un sprite
./target/debug/kiro-horror-tui --apercu /tmp/crypte.ans      # 4 écrans × 2 tailles en ANSI
./target/debug/kiro-horror-tui --muse-auth --once "Reply with exactly: OK"
cargo build --release            # met à jour `kiro-horror-tui` (symlink)
ghostty/crypte-test.sh           # dispatch lanceur (sur place vs fenêtre)
```

## Archi (`src/`)

`main.rs` (CLI + bootstrap), `app.rs` (état + boucle 10 ms, tick 100 ms),
`app/ctors.rs` (constructeurs), `app/model.rs` (modèle effectif + thinking),
`bus.rs` (adressage, files, liens auto, plafond 8 relais),
`mouse.rs` (molette = défilement de l'historique ; clics : Kiro en
balade = le frapper (bulle = la taire), [+] = nouveau chat, [×] =
fermer, lignes sidebar = choisir, onglets = écrans, articles boutique
= acheter ; drag bord sidebar = largeur colonne),
`scroll.rs` (défilement : décalage depuis le bas + fenêtre visible),
`drain.rs` (routage réponses async + pompes + retransmissions),
`events.rs` (flux JSONL temps réel `--events-log`/`KH_EVENTS_LOG` :
chats + sous-agents, silencieux si désactivé),
`cmds.rs` (handlers `/…`), `cmds_web.rs` (handler `/web`),
`cmds_link.rs` (handlers `/link`+`/unlink`),
`cmds_vps.rs` (handlers `/vps` + écran VPS), `vps.rs` (profils
`~/.config/kiro-horror-tui/vps.toml`, ssh, statuts distants),
`commands.rs` (parse), `texts.rs` (aide),
`settings.rs` + `settings/` (`/settings` : table des fournisseurs
`known.rs`, coffre chiffre `store.rs` (ChaCha20-Poly1305, `0600`, cle
maitre `master.key` ou `KH_SETTINGS_KEY`), ecran `ui.rs`, touches
`keys.rs`),
`permissions.rs` (mode yolo/auto + règles fines par outil + persistance
+ `T5State`, handler + porte dans `permissions_cmds.rs`),
`persist.rs` (chats : autosave JSON ~5 s + sortie, restauration silencieuse,
fichier illisible préservé en `.corrupt`),
`connect.rs` (handlers + flow `/connect` : statut, store Muse, clé masquée → modèle),
`history.rs` (fenêtre d'envoi + nommage des chats),
`question.rs` (bloc ```question : parse, rendu, réponse par chiffre),
`sessions.rs` + `sessions/` (chats `chat.rs` + sous-agents `subagents.rs` +
fermeture `close.rs`),
`workspace.rs` (workspaces `<session>/agent`, zéro token),
`sidebar.rs` (colonne cercueils + boutons [+] et [×]), `chat.rs` (historique +
saisie + prompt custom masquable), `provider.rs` (client OpenAI-compatible, timeout 60 s),
`tokens.rs` (estimation tiktoken + ledger), `agents.rs` (ce contexte :
chargement, pin, compaction pure),
`phantom.rs` + `phantom/` (Kiro en balade : humeurs + clics, `motion.rs`
errance/recul/K.O., `draw.rs` sprite 32×32 via `ImageBank` + bulle +
étoiles + BONK, jamais sur les cellules du sprite, `lines.rs` consigne
de critique + répliques), `app/critic.rs` (critique = même modèle que
le chat, `Incoming::Critique`, `/critic`, clics), `fx.rs`
(overlay horreur), `app/ui.rs` (mise en page), `theme.rs` (palette
unique), `bestiary.rs` (sprites), `gfx.rs` (images), `apparition.rs`
(panneau droit, `/bestiary`), `ghostty.rs` (OSC + mode 2026),
`hub.rs` + `hub/` (hub des esprits : simulation en dalles, `iso.rs`
crypte isométrique, `render.rs` scène, `live.rs` image Kitty par frame,
`font.rs` police 3×5, `social.rs` orbes et bavardages), `app/esprits.rs`
(panneau hub). Sprites du hub : `assets/hub/*.txt`.
`shop.rs` (boutique F6 + onglet : portefeuille 1 token = 1 pièce,
5 bonus d'ambiance, clic ou ↑↓ + Entrée).
`almanac.rs` (tokens par jour, bloc `/usage`) + `almanac/disk.rs`
(JSON `~/.local/share/kiro-horror-tui/`, relu avant chaque écriture,
jamais réécrit s'il est illisible).
`web/` (`/web` : backends → fetch → cache → synthèse, parsing pur dans
`web/parse.rs`, pertinence dans `web/rank.rs`).
`tools/` (outils agent : `wire.rs` types OpenAI, `browser.rs`/`desktop.rs`
argv purs, `exec.rs` exécution + timeout + journal, `flow.rs` boucle +
confirmations y/n, `log.rs` JSONL tailles seules, `live.rs` tests live
ignorés).
`kiro/` (compat Kiro : `steering.rs`, `spec.rs` (+ T2 : noms,
`check_task`), `hooks.rs`, `mcp.rs` (+ T4 : `cwd`/`headers`),
`ignore.rs`, `status.rs`, `glob.rs`, `cmds.rs` (`/kiro`),
`spec_cmds.rs` (`/spec` : handlers `new`/statut/`run` + drain, tests
dans `spec_cmds/tests.rs`), `spec_prompts.rs` (prompts purs),
`spec_run.rs` (`SpecGen`/`SpecRun`, purs), `mcp_client.rs`
(protocole), `mcp_conn.rs` (stdio/HTTP), `mcp_pool.rs` (parc),
`mcp_cmds.rs` (`/mcp`), `hook_match.rs` (regex `matcher`),
`hook_run.rs` (moteur : sélection + tirs + file `agent`),
`hook_exec.rs` (`sh -c` async + `HookOutcome`), `hook_cmds.rs`
(`/hooks`), `frontmatter.rs` (partagé), `agents_md.rs` (customs)
+ `agents_parse.rs` (parse `.json`/`.md` schéma Kiro),
`skills.rs` + `skills_cmds.rs` (`/skills`, `/skill`),
`checkpoint.rs` + `checkpoint_cmds.rs` (`/checkpoints`, `/rewind`),
`memory.rs` + `memory_cmds.rs` (`/remember`) ; steering `always` +
mémoire fusionnés au pin agents.md, voir `agents::load_merged_in`).
`ghostty/` : thème, shader et lanceur `ghostty/crypte`.

Flux : `send` (user → provider, tâche async, outillé si `/tools on` ;
en parallèle `critique` → `Incoming::Critique` → bulle de Kiro, almanach
seul, jamais l'historique) →
`drain_replies` (`Incoming::Chat|Agent|Compact|Web|ToolDone`) → almanach
(`Incoming::usage`, `None` pour `ToolDone`) + ledger + historique + affichage.
Boucle outils : réponse avec `tool_calls` (budget 8 tours, voir
`tools::MAX_ROUNDS`) → file séquentielle → porte `tool_gate`
(`permissions_cmds.rs` : règle fine `allow`/`ask`/`deny` par outil,
exact + préfixe `*`, défaut = mode `yolo`/`auto` via
`tools::needs_confirm` ; `deny` refuse sans y/n et prévient le modèle)
→ `exec::run_tool` (binaires existants, timeout, sortie tronquée) →
`Incoming::ToolDone` → résultats rejoués (`tool`, `tool_call_id`) →
nouvel appel, jusqu'au texte final (seul persisté à l'historique ;
guide transitoire `tools::GUIDE`).
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
Kiro : `.kiro/` projet + `~/.kiro/` lus comme Kiro (steering `always`
→ pin, `/kiro` = statut). `/spec new` génère requirements → design →
tasks (3 appels chaînés via `Incoming::SpecNew`, fichiers écrits au
fil de l'eau) ; `/spec <nom>` = statut ; `/spec <nom> run` exécute
les tâches via sous-agents `/agent` (requirements + design en
contexte), tâche cochée sur disque à chaque tombée, `y/n` entre
tâches en `auto` (voir `spec_waiting`), direct en `yolo`. T4 :
client MCP tools (`enabled` connectés au premier besoin,
`mcp_<serveur>_<outil>`, `autoApprove` sans confirmation, `/mcp` +
`/mcp refresh`). T3 : moteur hooks (trigger + `matcher` regex +
`.kiroignore` fichiers ; `command` = `sh -c` async 30 s via
`Incoming::HookDone`, `agent` = prompt préfixé au prochain envoi ;
tirs : envoi, outils, fichiers spec, tâches, agents, boot ;
garde anti-récursion profondeur 1, `/hooks` = statut + récents).
T5 : règles fines `/permissions <outil> <allow|ask|deny>` (`list`,
`reset`, persistées à côté du mode), agents customs
`.kiro/agents/` (format Kiro officiel : `.json`/`.md`, `prompt`,
`model`, `tools`, `mcpServers`, `team/<nom>` ; listés par `/agents`,
`/agent <nom> <texte>` avec prompt custom + modèle déclaré ;
`tools`/`mcpServers` lus et affichés, non exécutés), skills
`.kiro/skills/*/SKILL.md` (`/skills`,
`/skill` préfixé à la saisie, transitoire), checkpoints
(`write_tracked`, anneau 20, `/checkpoints`, `/rewind`),
mémoire `.kiro/MEMORY.md` + globale (pin après steering,
`/remember`). Kiro : complet (T1-T5).
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
- `.kiro/` : mêmes fichiers que Kiro (workspace + global) ; tests via
  chemins injectés UNIQUEMENT (jamais `set_var` : un seul site
  historique, `spec_bout_en_bout`, fixture sans steering ni MCP) ;
  `KH_KIRO_*` = overrides manuels du binaire ; en test, global=None
  par défaut (jamais `~/.kiro` réel).
- Couleurs : uniquement via `theme.rs` (jamais de `Color::Rgb` ailleurs,
  sauf les fonds RGBA de `gfx.rs` et `bestiary.rs`, documentés = `theme::VOID`).
- Kiro en balade : sprites `assets/pixel/kiro-*.txt` (fantôme blanc du
  hub en 32×32 ; `kiro-ko` = `kiro-frappe` retourné, testé) ; arène =
  sidebar + discussion (jamais le panneau droit ni sous la saisie) ;
  aucune écriture dans le rectangle du sprite (placeholders Kitty).
- Pixel art : grilles `assets/pixel/*.txt` 32×32, lettres de palette
  (voir README) ; mise à l'échelle **entière** au plus proche voisin
  seulement (`bestiary::fit`), jamais de filtre lissant : la netteté est
  testée (`gfx::tests`). Après retouche : régénérer `docs/bestiaire.png`.
- Effets (`fx.rs`) : jamais sur le texte ni dans les zones d'images
  (`ImageBank::drawn`). Aucun son (pas de BEL, pas de notification).
- UI et messages en anglais ; clé API **jamais affichée ni logguée**
  (ni chat, ni log, ni test — les tests réseau lisent le store). Au
  repos : coffre `/settings` chiffré (ChaCha20-Poly1305, `0600`), saisie
  masquée `•`, clé maître locale (`master.key`) ou `KH_SETTINGS_KEY`.
- `AGENTS.md` lu une seule fois, au premier envoi (`--agents-file` prime,
  sinon `./AGENTS.md` puis `./AGENT.md`, jamais relu ensuite) ;
  injecté en tête de chaque chat + `/agent` ; exclu de `/compact`
  (résumé méta) et de `--once` (brut).
