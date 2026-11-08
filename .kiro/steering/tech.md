---
inclusion: always
---

# Crypte — stack technique

- **Rust** (édition 2021, MSRV 1.74), binaire unique `kiro-horror-tui`.
- **TUI** : `ratatui` 0.29 + `crossterm` 0.28 ; boucle 10 ms, tick 100 ms.
- **Images** : protocole graphique Kitty (placeholders Unicode) via
  `ratatui-image` + `image` (PNG seul) ; repli halfblocks truecolor hors Ghostty.
- **Réseau** : `reqwest` rustls, client OpenAI-compatible maison (`provider.rs`,
  timeout 60 s) ; estimation locale des tokens via `tiktoken-rs`.
- **Chiffrement** : `chacha20poly1305` (coffre des clés API).
- **Dépendances minimales** : chaque feature cargo est justifiée dans
  #[[file:Cargo.toml]] — ne pas ajouter de crate ou de feature sans le prouver
  par `grep` + `cargo check`, et documenter le choix en commentaire.

## Commandes (tout doit être vert avant de rendre)

```sh
cargo build                      # 0 warning exigé
cargo test                       # tous verts
cargo fmt --check                # formatage propre exigé
./target/debug/kiro-horror-tui --smoke   # rendu headless 80x24, sans réseau ni clé
```

Autres modes print-only (toujours sur place, jamais de fenêtre) : `--help`,
`--once "<texte>"`, `--sheet`, `--preview`, `--demo`, `--haunt-shot`.
`ghostty/crypte` = lanceur (sur place si le terminal peut, sinon fenêtre
Ghostty habillée) ; `ghostty/crypte-test.sh` = tests du dispatch.

## Exigences non négociables

- **0 warning** (`cargo build`), **0 test rouge**, **`cargo fmt` propre**.
- Après retouche d'un sprite : régénérer `docs/bestiaire.png`
  (`--sheet`) et vérifier `--haunt-shot` pour le jeu.
- Ne jamais compiler ni lancer le TUI interactif dans un terminal non
  compatible : `--smoke` suffit pour valider un rendu.
