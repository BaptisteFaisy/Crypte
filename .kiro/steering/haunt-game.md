---
inclusion: fileMatch
fileMatchPattern:
  - "src/haunt/**"
  - "src/app/haunt.rs"
---

# Crypte — Haunt, le jeu F9

Plateformer/tir original (Kiro envoie des tokens), niveau 1 « The Boneyard »
très difficile, vue 960×576, dalles 6 px, style bestiaire.

- **Simulation à pas fixe 240 Hz** : vitesses et durées écrites en pas de
  référence de 1/30 s, **toujours multipliées par `DT` / `SUB`**.
  L'écran est redessiné jusqu'à 240 fois/s ; l'encodage PNG Kitty tourne sur
  3 fils (`hub/live.rs`, frames numérotées, jamais d'attente ni de désordre).
- Entrées : `a`/`d` (ou ←/→), `w` saut 4 dalles, `s` s'enfoncer
  (`s`+`w` traverse une dalle), Espace = tir (2 max en vol), Entrée =
  start/relever, `p` pause, Échap/F9 = retour chat (run en pause).
- Protocole clavier Kitty empilé **seulement en jeu** (`app/haunt.rs`) ;
  ailleurs : halfblocks + répétition de touches.
- Niveau = grille texte `assets/haunt/niveau-1.txt` (une lettre par dalle,
  dernière ligne = zones du directeur) ; les tests vérifient que l'arène
  est atteignable. Sprites = grilles à lettres de palette.
- Look « 32-bit » : upscale entier au plus proche (×1, ×2 HiDPI), ciel
  tramé aux jointures, parallaxe, brouillard, vignette.
- Coût d'une frame : `cargo test --release -- --ignored frame_budget --nocapture`.
- Après retouche : `./target/debug/kiro-horror-tui --haunt-shot /tmp/haunt.png`.
