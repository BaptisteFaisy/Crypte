---
name: crypte-verif
description: Vérifier kiro-horror-tui sans réseau ni clé (smoke test headless, tests, formatage, aperçus ANSI) et produire la release Ghostty (fenêtre habillée, rebuild depuis origin/main). Utiliser avant tout commit, après retouche visuelle, ou pour livrer un binaire.
---

# Vérification et release de la crypte

## Smoke test headless (sans réseau, sans clé)

```sh
cargo build                      # 0 warning exigé
cargo test                       # tout vert exigé
cargo fmt --check                # formatage propre exigé
./target/debug/kiro-horror-tui --smoke              # rendu headless 80x24
./target/debug/kiro-horror-tui --apercu /tmp/crypte.ans  # 4 écrans x 2 tailles, lire avec cat dans Ghostty
./target/debug/kiro-horror-tui --once "Reply with exactly: OK"  # nécessite clé ou --muse-auth
```

Après retouche visuelle, ajouter la commande de régénération couverte
par le skill `pixel-art` (`--planche`, `--haunt-shot`).

## Règles fichiers (rappel AGENTS.md)

- Fichier : 500 lignes max ET 30 fonctions max (tests inclus).
- Fonction : moins de 50 lignes. Au-delà : découper, jamais laisser grossir.
- Vérifier : `wc -l` + `grep -cE 'fn [a-zA-Z_][a-zA-Z0-9_]* *\('`
  sur chaque fichier touché. Zéro duplication (helpers partagés).
- UI et messages en anglais ; clé API jamais affichée ni logguée.

## Release Ghostty

Le lanceur `ghostty/crypte` utilise le terminal courant s'il peut
accueillir la crypte (TTY interactif, truecolor, ≥ 80x24), sinon il
ouvre une fenêtre Ghostty habillée (thème + shader, build release
si besoin) :

```sh
KH_API_KEY=... KH_MODEL=... ghostty/crypte
ghostty/crypte rebuild --muse-auth   # origin/main frais dans un worktree dédié, build release forcé, checkout courant intact
cargo build --release                # met à jour le symlink `kiro-horror-tui`
ghostty/crypte-test.sh               # dispatch lanceur (sur place vs fenêtre)
```

`rebuild` refuse uniquement si le worktree géré est sale (un arbre
courant sale ne bloque pas). Avec `--muse-auth`, la clé vient de
`~/.config/muse/auth.json` (jamais affichée ni copiée).
