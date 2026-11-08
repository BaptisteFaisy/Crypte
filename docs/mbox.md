# mbox — boîte aux lettres des agents

Miroir Rust du **broadcast v2** shell (`bcast`) : écrire au user
(`BOITE-USER.md`), parler aux autres agents (bus `phone|gpu|file|divers`),
recevoir des ordres (TTL + ACK), tout en **fichiers partagés**
interopérables à l'identique entre le binaire Rust et le `bcast` shell.

Deux parties : [guide utilisateur](#1-guide-utilisateur) puis
[guide développeur](#2-guide-développeur-architecturejetonsinteroplimites).

---

## 1. Guide utilisateur

### 1.1 Invocation

Tout passe par le binaire, sous-commande `--mbox` (elle avale tout le
reste d'argv, `--` inclus — voir `src/args.rs:91`) :

```sh
./target/debug/kiro-horror-tui --mbox <cmd...>
```

Deux variables d'environnement :

- `BCAST_DIR` : dossier broadcast (défaut
  `/home/baptiste/Bureau/.scratch/broadcast`, `mod.rs:51`). Les exemples
  ci-dessous tournent sur un dossier jetable
  (`BCAST_DIR=/tmp/mbox-doc-test`).
- `BCAST_SENDER` : identité d'expéditeur par défaut (sinon `--from ID`,
  exigé par `send`/`bus`).

### 1.2 Aide et diagnostic

```sh
$ ./target/debug/kiro-horror-tui --mbox --help
bcast 2.0 — broadcast user + bus inter-agents
  bcast send [-p P0|P1|P2] [--from ID] [--dest @x]... [--state k=v] [--force] -- <msg>
  bcast bus -t phone|gpu|file|divers [--from ID] -- <msg>
  bcast tail [-n N] [--bus t] [--all] | bcast digest | bcast rotate [--keep-days 7]
  bcast hb <nom> <état...> | bcast hb list | bcast ordre new|list|revoque|ack|pull | bcast doctor
  bcast metriques [--agent ID] | bcast fantome rejouer|diff|etat|purge | bcast --fantome <cmd...>
  bcast jeton new [--ttl H] <nom> | bcast jeton revoque <nom> | bcast jeton list | bcast jeton purge
  bcast piste [--ordre ID] [--agent ID] | bcast vague plan|run|lister|etat
Inbox : P0 popup+son, P1 popup, P2 log seul. Throttle : dedup 30 min, 6 msg/h.
Sorties : 0 ok (throttled:/same-state inclus), 1 exécution, 2 usage, 3 déjà-en-cours.
Écritures sensibles (--force, ordre new/revoque/ack) : --token "nom:secret" (voir jeton new).
```

`doctor` vérifie le dossier (sortie réelle, dossier initialisé par un
`send`) :

```sh
$ ./target/debug/kiro-horror-tui --mbox doctor
OK inbox 1 lignes 98 octets
OK format des 200 dernières lignes
OK bus 4 topics
OK ordres 0 ligne(s)
OK metriques send=1 dup=0 rate=0 same=0 bus=0 bus_dup=0 ordres=0 revoques=0 acks=0 hb=0
OK piste 0 spans 0 ordres traces
OK écriture verrouillée
```

Sur un dossier absent, `doctor` rend `ERR dossier absent : …` (code 2).

### 1.3 Écrire au user : `send`

```sh
BCAST_SENDER=doc-test ./target/debug/kiro-horror-tui --mbox send -p P2 -- message doc test
# → sent
```

- `-p P0` = popup+son, `P1` = popup (défaut), `P2` = log seul.
- `--dest @x` (répétable), `--state k=v` (sonde : `same-state` si
  inchangé, sans écrire), `--force` (contourne le throttle, exige
  `--token`, tracé `[bypass-throttle]`).
- Throttle : déduplication 30 min + quota 6 msg/h (voir § 1.8).
  `send` rend `sent`, `throttled:dup`, `throttled:rate` ou `same-state`,
  toujours en code 0.

### 1.4 Bus inter-agents : `bus`, `tail`, `digest`

```sh
./target/debug/kiro-horror-tui --mbox bus -t divers -- ping bus   # → sent
./target/debug/kiro-horror-tui --mbox tail -n 3
# → - [2026-11-08 16:32] doc-test [P2] : message doc test {#pid=… …}
./target/debug/kiro-horror-tui --mbox digest                      # → digests=N
```

`tail` lit l'inbox (`-n N`, défaut 20) ou un sujet (`--bus t`, `--all`
= inbox + les 4 sujets). `digest` vide la file P2. `rotate`
(`--keep-days 7`) archive les vieux journaux.

### 1.5 Présence : `hb`

```sh
./target/debug/kiro-horror-tui --mbox hb sonde-doc ok   # → ok
./target/debug/kiro-horror-tui --mbox hb list           # sondes fraîches
```

### 1.6 Ordres : `ordre`

Cycle de vie : `new` → `list`/`pull` → `ack lu|applique|refuse`,
avec TTL (heures, défaut 24, `0` = permanent) et `revoque`.

```sh
./target/debug/kiro-horror-tui --mbox ordre list                    # → aucun ordre
./target/debug/kiro-horror-tui --mbox jeton new doc-agent           # → secret=<hex, 1x>
./target/debug/kiro-horror-tui --mbox ordre new --to @moi --ttl 24 --token "doc-agent:<secret>" -- Relire docs/mbox.md
./target/debug/kiro-horror-tui --mbox ordre pull @moi               # mes ordres actifs
./target/debug/kiro-horror-tui --mbox ordre ack O-20261108-001 applique --from moi --token "doc-agent:<secret>"
./target/debug/kiro-horror-tui --mbox ordre revoque O-20261108-001 "doublon" --token "doc-agent:<secret>"
./target/debug/kiro-horror-tui --mbox ordre deleguer O-20261108-001 --to @moi --resume "reprends la relecture" --token "doc-agent:<secret>"
# → delegue=O-20261108-002 (nouvel ordre lié par supersede=O-20261108-001, voir § 1.10)
```

`new`, `revoque`, `ack` et `deleguer` sont des écritures sensibles :
elles exigent `--token "nom:secret"` (voir § 1.7). `pull` lit sans
solder. `new` scelle un `sig=` HMAC (vérifié à l'`ack`, voir § 2.4).

### 1.7 Jetons : `jeton`

L'autorité des jetons vit **hors broadcast**, dans le dossier des
réglages (`Store::default_dir`, jamais dans `BCAST_DIR`). Le secret
(32 octets hex) n'est montré **qu'une fois**, à l'émission :

```sh
./target/debug/kiro-horror-tui --mbox jeton new --ttl 1 doc-agent
# → jeton=doc-agent secret=<32 octets hex, affiché une seule fois>
./target/debug/kiro-horror-tui --mbox jeton list
./target/debug/kiro-horror-tui --mbox jeton revoque doc-agent   # → revoque=doc-agent
./target/debug/kiro-horror-tui --mbox jeton purge               # jette les expirés
```

`--ttl H` : heures (défaut 24, `0` = permanent). Nom déjà actif =
code 3 (`DéjàEnCours`).

### 1.8 Métriques : `metriques`

Compteurs durables (`.state/metriques`, lignes `cle=valeur`), globaux
et par agent (sortie réelle) :

```sh
$ ./target/debug/kiro-horror-tui --mbox metriques
send_ok=1
agent doc-test-9cefc5d6 send_ok=1
integrite ordres: 0 ok 0 ko 0 sans hash, signatures: 0 ok 0 ko 0 sans
```

`metriques --agent ID` restreint la section `agent ...` à un
agent ; les compteurs globaux et la ligne `integrite` s'affichent
toujours. La ligne `integrite` vérifie les `hash=` scellés des
ordres et compte les `sig=` (`ok` = présente et bien formée,
`ko` = mal formée, `sans` = ordre shell nu — voir § 2.4).

### 1.9 Bac à sable : `--fantome` et `fantome`

`--fantome <cmd...>` rejoue une commande sur un **miroir**
(`.fantome/`, zéro écriture prod) puis affiche le diff (sortie réelle) :

```sh
$ ./target/debug/kiro-horror-tui --mbox --fantome send -p P2 -- essai fantome
sent
fantome diff .state/metriques: prod=5 miroir=2
fantome diff bus/divers.log: prod=1 miroir=0
fantome: send code=0 +0 lignes (miroir seul, prod intacte)
$ ./target/debug/kiro-horror-tui --mbox fantome etat
fantome: miroir /tmp/mbox-doc-test/.fantome (1 ops tracées)
fantome: dernière 1794151950 op=send -p P2 -- essai fantome code=0
fantome diff .state/metriques: prod=5 miroir=2
fantome diff bus/divers.log: prod=1 miroir=0
```

`fantome rejouer|diff|etat|purge` gère le miroir. Refus typés avant
exécution : `jeton`, `rotate`, commandes imbriquées, inconnues.

### 1.10 Délégation : `ordre deleguer`

Transférer un ordre à un autre agent, à portée minimale : seul un
résumé explicite voyage, plus les **métadonnées** de pièces
explicites (`--piece`, répétable, ≤ 8, relatifs sans `..`, ≤ 64 Kio
— chemin, taille et SHA-1, jamais le contenu). Sortie réelle :

```sh
$ ./target/debug/kiro-horror-tui --mbox ordre deleguer O-20261108-001 --to @moi --resume "reprends la relecture" --token "doc-agent:<secret>"
delegue=O-20261108-002
$ ./target/debug/kiro-horror-tui --mbox ordre pull @moi
O-20261108-002 [2026-11-08 16:58] -> @moi : Délégation de O-20261108-001 par doc-deleg : reprends la relecture supersede=O-20261108-001 hash=ac229c30a19367518627399416bc1bc3af576f2c
```

Le transfert est un ordre normal (`src=delegation:<agent>`, TTL 24 h)
lié à sa source par `supersede=<id>` ; le destinataire accuse via
`ordre ack <nouvel-id> lu|applique|refuse`. La source doit être active
(ni révoquée ni expirée). Le transfert n'est pas scellé (`sig=`
absent) : le scellement lierait l'ACK au secret du délégant, que le
destinataire n'a pas (voir § 2.4).

### 1.11 Piste d'audit : `piste`

Spans 100 % locaux (`piste.log`, jamais exportés) :
`run_id|epoque|agent|action|ordre_id|verdict`, corrélés par `ordre_id`
(sortie réelle) :

```sh
$ ./target/debug/kiro-horror-tui --mbox piste --ordre O-20261108-001
240df9b78348|1794153513|doc-deleg-f698b653|ordre_cree|O-20261108-001|ok
$ ./target/debug/kiro-horror-tui --mbox piste --agent doc-deleg
240df9b78348|1794153513|doc-deleg-f698b653|ordre_cree|O-20261108-001|ok
```

Actions tracées : `ordre_cree`, `ordre_ack`, `ordre_revoque`
(`envoi`, `delegation`, `jeton_emis` réservés). Jamais de secret
dans les spans : agent assaini, pas de texte d'ordre, jeton réduit
à son nom. `doctor` affiche le résumé (`OK piste N spans M ordres
traces`, voir § 1.2).

### 1.12 Vagues : `vague`

Orchestrer une file de commandes mbox (`ID|APRES|COMMANDE`, `#` et
lignes vides ignorés) avec dépendances et parallélisme (sortie réelle) :

```sh
$ cat /tmp/mbox-doc-test/vague-doc.txt
V-1||bus -t divers --from vague-t -- coucou doc
V-2|V-1|hb sonde-doc ok
$ ./target/debug/kiro-horror-tui --mbox vague plan --fichier /tmp/mbox-doc-test/vague-doc.txt
vague: 2 taches
1. V-1 [apres: -] :: bus -t divers --from vague-t -- coucou doc
2. V-2 [apres: V-1] :: hb sonde-doc ok
$ ./target/debug/kiro-horror-tui --mbox vague run --fichier /tmp/mbox-doc-test/vague-doc.txt
sent
ok
vague: ok=2 ko=0
$ ./target/debug/kiro-horror-tui --mbox vague lister
V-1 [ok] :: bus -t divers --from vague-t -- coucou doc
V-2 [ok] :: hb sonde-doc ok
$ ./target/debug/kiro-horror-tui --mbox vague etat
vague: attente=0 cours=0 ok=2 echec=0 bloque=0 expiree=0 total=2
```

`plan` ordonne sans exécuter, `run` exécute (`--paralleles N`,
1..32, défaut 4 ; `--delai S`, 1..3600, défaut 60 ; code 1 si une
tâche échoue), `lister`/`etat` relisent la file persistée. Liste
blanche : `send bus ordre(new|ack) hb metriques` — `jeton`,
`rotate` et `vague` imbriquée refusés. Dépendance KO → `bloque`,
timeout → `expiree`. File persistée `vague/file.log` (0600, elle
peut porter des `--token`), synthèse APPEND `vague/vague-AAAA-MM-JJ.md`
toujours caviardée (`--token ***`).

### 1.13 Codes de sortie

`0` ok (`throttled:`/`same-state` inclus), `1` erreur d'exécution,
`2` erreur d'usage (`bcast: …` sur stderr), `3` déjà-en-cours
(jeton existant). `version` affiche `bcast 2.0.0`.

---

## 2. Guide développeur (architecture/jetons/interop/limites)

### 2.1 Architecture : 23 modules sous `src/mbox/`

`mod.rs` = socle partagé : chemins (`BcastDir` : `defaut/depuis/init/
boite/bus/ordres/acks/vue_ordres/hb/etat/tailles`), horodatage
(`epoque`, `maintenant`), identités sûres (`sanitize`), codes
(`Sortie`, `code`). Chaque module réutilise ce socle (`super::` ou
`crate::mbox::`) — aucune logique dupliquée.

| module | rôle |
| --- | --- |
| `mod.rs` | socle : chemins, temps, sanitisation, codes |
| `format.rs` | lignes v2 (construire/parser), priorités, TTL |
| `verrou.rs` | verrou frère + `flock` (1 point d'attente) |
| `fichier.rs` | append verrouillé, réécriture atomique, lectures |
| `autorite.rs` | autorité des jetons (émettre/vérifier/révoquer) |
| `jeton.rs` | CLI `jeton` (extension sans équivalent shell) |
| `ordres.rs` | ordres : parse 8 champs, TTL, révocation, `hash=` |
| `ack.rs` | ACK `lu\|applique\|refuse` + motif de refus |
| `journal.rs` | journaux : `lister` (+ pont `tail --all`) |
| `throttle.rs` | dedup inbox 1800 s, quota 6/h, dedup bus 300 s |
| `digest.rs` | `digest_flush` (file P2) |
| `sonde.rs` | sondes `hb` (état, fraîcheur, `hb list`) |
| `cli.rs` | CLI : `send/bus/tail/digest/hb/version/help` |
| `cli_gestion.rs` | CLI : `rotate/ordre` |
| `docteur.rs` | CLI : `doctor` (inbox + bus + ordres + hb) |
| `metriques.rs` | compteurs durables + `integrite ordres` |
| `fantome.rs` | miroir `.fantome/` + diff, garde typée |
| `outils.rs` | découpage d'options, `--from`, porte `--token` |
| `delegation.rs` | `ordre deleguer` : transfert à portée minimale |
| `piste.rs` | spans d'audit locaux + CLI `piste` |
| `signature.rs` | HMAC-SHA1 : scellement `sig=` + vérification |
| `vague.rs` | CLI `vague` : file, liste blanche, caviardage |
| `vague_exec.rs` | exécution vagues : topo, threads, synthèse |

`cli.rs:55-91` (`exec`) répartit vers les commandes ; `main.rs:232`
branche `--mbox` via `outils::lance_mbox`. `digest.rs`, `docteur.rs`,
`outils.rs` sont nés de découpages (fichiers qui dépassaient 500 L).

### 2.2 Jetons

- Stockage : fichier `mbox-jetons` **0600**, dossier **0700**
  (`autorite.rs:212-219`), écriture atomique tmp+rename.
- Secret 32 octets, montré une fois (`jeton.rs:51`), jamais affiché
  ni journalisé ailleurs ; comparaison à temps constant, fail-closed.
- Porte : `outils::porte_jeton` (`--token "nom:secret"`, découpe au
  dernier `:`) + garde anti-secret sur le texte, exigée pour `send
  --force`, `ordre new/revoque/ack`.
- Verdicts : expiré, révoqué, inconnu → refus code 1 ; nom déjà actif
  à l'émission → code 3.

### 2.3 Interopérabilité shell ↔ Rust

- Verrou frère `<cible>.lock` (`verrou.rs:25-30`, miroir de
  `lib-common.sh:54`), jamais le log lui-même : Rust et shell se
  sérialisent mutuellement. `flock` via `libc`, boucle `LOCK_NB` +
  attente 10 s comme `flock -w 10` (`ATTENTE_VERROU_SECS`).
- Écritures : append verrouillé + réécriture atomique ; lectures
  fail-open (`fichier.rs`). Compteurs throttle fail-open, autorité et
  écritures sensibles fail-closed — un seul point d'attente
  (`verrouiller`, `verrou.rs`).
- Formats lus à l'identique du shell : ordre
  `id|epoch|date|to|ttl|statut|src|texte` (8 champs, `ordres.rs`),
  `O-AAAAMMJJ-NNN`, extensions suffixes `prio=/supersede=/hash=/sig=`,
  ACK 5 champs, lignes inbox v2 + continuations.
- Preuve : `tests/mbox_interop.rs` pilote le binaire (`--mbox`) **et**
  le `bcast` shell sur un `BCAST_DIR` jetable (roundtrip deux sens,
  bus + dedup, cycle jetons/ordres, codes). Autorité confinée par
  `XDG_DATA_HOME` jetable : aucun test n'écrit hors de son bac.

### 2.4 Intégrité et métriques

- `hash=` scellé à la création de l'ordre (`journal.rs` via
  `empreinte`) ; `metriques` affiche `integrite ordres: N ok M ko K
  sans hash, signatures: …`.
- `sig=` vérifiée (HMAC-SHA1 maison sur `id|dest|texte normalisé`,
  `signature.rs`, zéro dépendance) : scellée par `ordre new` avec le
  secret du jeton présenté, contrôlée par `ordre ack` avec le secret
  du jeton présenté (comparaison constante, calcul toujours exécuté).
  `sig=` absente (ordres shell) → ACK accepté ; présente et invalide
  → refus (code 1). Limite connue : la clef étant le secret présenté
  à l'ACK, un ordre scellé par X n'est ackable que par un détenteur du
  même secret (l'autorité ne ré-expose jamais les secrets) — les
  transferts `deleguer` restent donc non scellés (voir § 1.10).
- Compteurs `.state/metriques` (lignes `cle=valeur`, verrou frère
  fail-open) : `send_ok/dup/rate/same-state/bus/…` + `par_agent.<id
  sanitisé>.<clé>`.

### 2.5 Limites (règle Crypte, rappel `AGENTS.md`)

- Fichier source : **500 lignes max ET 30 fonctions max** (tests
  inclus, `grep -cE 'fn [a-zA-Z_][a-zA-Z0-9_]* *\('`) ; fonction
  **< 50 lignes**. Au-delà : découper en modules, jamais laisser
  grossir. État au 2026-11-08 : 23 modules + `tests/mbox_interop.rs`,
  tous sous les plafonds (ex. `cli.rs` 492 L/18 fn, `sonde.rs`
  489 L/25 fn, `autorite.rs` 486 L/27 fn, `delegation.rs` 476 L/20 fn,
  `tests/mbox_interop.rs` 482 L/22 fn).
- **Zéro duplication** : une seule implémentation par logique (époque
  unique dans `format`, verrou unique dans `verrou`, découpage unique
  dans `outils`).
- Code **et commentaires en français** ; **aucune donnée secrète**
  affichée ni journalisée (ni chat, ni log, ni test, ni doc — les
  exemples ci-dessus masquent les secrets).
- CI : `mbox.yml` rejoue `fmt --check`, build, tests unitaires +
  interop, et la garde des limites ; tout nouveau job respecte le
  budget 5 min (`timeout-minutes: 5`, `concurrency` + cancel,
  `fetch-depth: 1`, cache cargo).
