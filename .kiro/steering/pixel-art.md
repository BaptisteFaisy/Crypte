---
inclusion: fileMatch
fileMatchPattern:
  - "assets/**/*.txt"
  - "src/bestiary.rs"
  - "src/gfx.rs"
  - "src/canvas.rs"
  - "src/apparition.rs"
  - "src/hub/**"
  - "src/haunt/**"
  - "src/phantom/**"
---

# Crypte — pixel art et rendu

- Grilles texte : `assets/pixel/<slug>.txt` = 32 lignes de 32 lettres,
  `.` = transparent, sinon lettre de palette `src/theme.rs`
  (`W` os, `S` sable, `U` ombre, `K` encre, `O`/`o` citrouille,
  `R`/`r` sang, `G`/`g` ectoplasme, `P`/`p` spectre, `Y`/`y` bougie,
  `N` toile). Sprites du hub : `assets/hub/*.txt`. Sprites du jeu :
  `assets/haunt/*.txt` (mini-sprites 6–18 px, dalles 6 px).
- Rendu net garanti : upscale **entier** au plus proche voisin sur un canvas
  à la taille pixel exacte (`bestiary::fit`) — 1 pixel image = 1 pixel écran
  en Kitty, = 1 demi-cellule en halfblocks. Jamais de filtre lissant
  (la netteté est testée dans `gfx::tests`).
- Ghostty : image Kitty réémise chaque frame sous le même id (PNG, ×2–×6
  au plus proche) ; ailleurs : halfblocks truecolor.
- Après toute retouche de sprite : régénérer `docs/bestiaire.png`
  (`--sheet docs/bestiaire.png`) et vérifier `--preview` / `--haunt-shot`.
- Kiro KO = sprite `kiro-frappe` retourné (testé) ; ne jamais écrire dans le
  rectangle du sprite en balade (placeholders Kitty) ni appliquer `fx.rs`
  sur le texte ou les zones d'images.
