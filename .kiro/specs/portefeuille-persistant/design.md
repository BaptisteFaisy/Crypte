# Portefeuille persistant — design

## Vue d'ensemble

Un fichier JSON `wallet.json` dans le dossier de données (même
dossier que l'almanach) garde deux cumuls monotones : pièces gagnées
et pièces dépensées sur toutes les sessions. Au démarrage, la boutique
charge ces cumuls comme base ; pendant la session, le gagné courant
vaut base + tokens de la session, le dépensé courant vaut base +
achats de la session. Sauvegarde à chaque achat réussi et à la sortie.

## Composants

- `src/wallet.rs` (nouveau, pur, testé) : le type
  `WalletFile { earned: u64, spent: u64 }`, `load(path)` qui rend le
  contenu plus une éventuelle erreur non fatale, `save(path)` en
  écriture atomique (fichier temporaire + renommage, comme le coffre),
  et le chemin du fichier depuis `XDG_DATA_HOME` (repli
  `~/.local/share/kiro-horror-tui/`, comme l'almanach).
- `src/shop.rs` (câblage fin) : `Shop` garde un `earned_base` et un
  `spent_base` chargés au démarrage ; `buy_selected` sauvegarde après
  chaque achat réussi ; aucun changement de rendu (même en-tête).
- `src/cmds.rs` (`/usage`, câblage fin) : la ligne boutique affiche les
  cumuls vie entière, plus l'erreur éventuelle de chargement.
- `src/app.rs` (câblage fin) : sauvegarde à la sortie normale.

## Flux de données

Démarrage :

```text
wallet.json --load--> (earned_base, spent_base) --> Shop
fichier absent  --> (0, 0), silence
fichier fautif  --> (0, 0) en mémoire, erreur gardée pour /usage,
                    fichier fautif jamais écrasé
```

Achat réussi et sortie normale :

```text
Shop --save--> relit wallet.json --max(cumuls)--> écrit wallet.json
```

Le solde affiché vaut toujours
`(earned_base + tokens_session) - (spent_base + achats_session)`,
borné à zéro (jamais négatif).

## Décisions

- D1 — JSON dans le dossier de données, comme l'almanach : pas de
  nouveau format ni de nouvelle dépendance, chemins déjà établis.
- D2 — Deux cumuls monotones plutôt qu'un solde : le gagné se
  recalcule depuis les tokens (l'almanach font foi), le dépensé ne
  peut que croître, la fusion multi-instances devient un simple max.
- D3 — Pas de chiffrement : les pièces n'ont aucune valeur et aucune
  confidentialité, le coffre chiffré reste réservé aux clés API.
- D4 — Sauvegarde à l'achat + à la sortie, pas d'autosave périodique :
  les achats sont rares et l'earned se recalcule ; en cas de crash, seules
  les pièces gagnées depuis le dernier achat sont perdues (documenté).
- D5 — Fusion par max à chaque sauvegarde (relire avant d'écrire, comme
  l'almanach) : deux cryptes ouvertes ne s'écrasent jamais.

## Cas limites

- Fichier absent : zéros silencieux (R4).
- JSON invalide, types inattendus, permissions : zéros en mémoire,
  fichier fautif préservé, erreur visible dans `/usage` (R5).
- Échec d'écriture : session mémoire inchangée, erreur dans `/usage` (R6).
- Dépassement `u64` : arithmétique saturante partout, comme `Wallet` actuel.
- Ancienne session sans base : `earned_base = 0`, comportement identique
  à aujourd'hui pendant la première session migrée.

## Stratégie de test

- Unitaires `wallet.rs` : round-trip charge/sauve, fichier absent,
  JSON invalide (jamais écrasé), fusion par max — chemins injectés
  (dossier temporaire) UNIQUEMENT, jamais le `~/.local/share` réel ni
  `set_var`, selon la convention `.kiro/` du dépôt.
- Unitaires `shop.rs` : solde avec base non nulle, achat qui débite le
  cumul, trop cher qui ne sauvegarde pas.
- Vérifications dépôt : `cargo test` vert, `cargo fmt --check` propre,
  `--smoke` headless OK, `wc -l` + comptage `fn` sur les fichiers
  touchés (500 lignes / 30 fonctions max, fonctions < 50 lignes).
- Aucune nouvelle couleur : rendu inchangé, `theme.rs` seul autorisé
  de toute façon ; messages UI en anglais.

## Contraintes dépôt rappelées

- `src/wallet.rs` : 500 lignes max ET 30 fonctions max, fonctions
  < 50 lignes ; `src/shop.rs` (424 lignes aujourd'hui) reste sous les
  seuils — sinon découper, jamais laisser grossir.
- Zéro duplication : le chemin du dossier de données et l'écriture
  atomique sont partagés avec l'existant, pas recopiés.
- `cargo build` 0 warning exigé.
