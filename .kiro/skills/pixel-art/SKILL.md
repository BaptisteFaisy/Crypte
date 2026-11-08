---
name: pixel-art
description: Ajouter ou retoucher un sprite pixel art 32x32 du bestiaire (grilles assets/pixel/*.txt, lettres de palette, mise à l'échelle entière). Utiliser pour tout nouveau monstre, retouche de sprite, régénération de la planche bestiaire ou rendu Kitty.
---

# Pixel art du bestiaire

Chaque créature est UNE grille texte `assets/pixel/<slug>.txt` : 32 lignes
de 32 lettres, `.` = transparent, sinon une lettre de la palette
(`src/theme.rs`, voir README § Bestiaire) :

- `W` os, `S` sable, `U` ombre, `K` encre
- `O`/`o` citrouille / sombre, `R`/`r` sang / séché
- `G`/`g` ectoplasme / sombre, `P`/`p` spectre / sombre
- `Y`/`y` bougie / sombre, `N` toile de jute

## Règles impératives

1. 32 lignes exactement, 32 caractères exactement par ligne (vérifier avec
   `awk '{ print length }' assets/pixel/<slug>.txt | sort -u`).
2. Lettres de palette uniquement + `.` (aucune autre lettre).
3. Couleurs uniquement via `theme.rs` côté code (jamais de `Color::Rgb`
   ailleurs que `gfx.rs`/`bestiary.rs` pour les fonds documentés).
4. Mise à l'échelle **entière** au plus proche voisin seulement
   (`bestiary::fit`) : jamais de filtre lissant, la netteté est testée
   (`gfx::tests`).
5. Kiro en balade : sprites `assets/pixel/kiro-*.txt` (`kiro-ko` =
   `kiro-frappe` retourné, testé) ; ne jamais écrire dans le rectangle
   du sprite (placeholders Kitty) ni sous la saisie.
6. Effets (`fx.rs`) : jamais sur le texte ni dans les zones d'images
   (`ImageBank::drawn`). Aucun son (pas de BEL).
7. Ne jamais copier d'image d'origine : tout sprite est redessiné
   (le bestiaire actuel est redessiné d'après une planche Dribbble,
   aucune image d'origine dans le dépôt).

## Après toute retouche

```sh
cargo build   # 0 warning exigé
cargo test    # tout vert exigé
./target/debug/kiro-horror-tui --planche docs/bestiaire.png  # régénère la planche
./target/debug/kiro-horror-tui --smoke   # rendu headless 80x24, sans réseau ni clé
```

Autres grilles du même esprit (mêmes règles de netteté) :
sprites du hub `assets/hub/*.txt`, sprites du jeu `assets/haunt/*.txt`
(+ `--haunt-shot /tmp/haunt.png` après retouche, niveau
`assets/haunt/niveau-1.txt` testé comme atteignable).
