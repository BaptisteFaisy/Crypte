# 100 idées pour aller plus loin (project space 3D)

État au 2026-11-12 : moteur 3D logiciel livré (ÉTAPE 3 + 4), vérifié sous Ghostty.
Les 10 retenues pour implémentation immédiate sont marquées ⭐.

## A. Fidélité au prototype (1-12)
1. ⭐ Fantômes = sprites réels du dépôt (`assets/hub/agent-*.txt` via `bestiary::decode_grid` + `fit` + `blit` teinté blanc) au lieu de la silhouette procédurale.
2. ⭐ Bloom approché : downsample ×4, seuil 0.62, blur additif, recompose (DESIGN §6.4).
3. ⭐ Panneau de chat fidèle : gradient 3 %→0,75 %, bordure 1 px `line`, radius 18, corner ticks orange 9 px, blur 6 px (DESIGN §4).
4. Fantôme « extrudé » : silhouette du path SVG (§6.3) avec profondeur simulée (deux plans décalés + flancs).
5. Yeux noirs aplatis + halo de contact sous le fantôme (§6.3).
6. Lumières : hémisphérique + directionnelle simulées (ombrage de Lambert sur les faces des dalles).
7. Point light orange au-dessus du fichier en cours d'écriture.
8. Piliers lointains très faints (3-7 %, fondu vers le haut) + 1 000 poussières à 40 % blanc.
9. Brouillard progressif lié à la « découverte » (1 500 → 4 000 → 26 000).
10. Vignette radiale à 80 % sur les bords (déjà 55 %).
11. Dalle active qui se soulève de 34 unités et passe de 0,6→1 en 0,5 s à la création.
12. Révélation gauche→droite du texte de réponse (1,6 s).

## B. Narration / UI flottante (13-24)
13. ⭐ Grand titre d'étape « 04 / 06 · ACT » cuit dans l'image, 64 px équivalent (police pixel ×8).
14. ⭐ Caption `[WHO] what happens` en bas (chips you / main ghost / sub-agents / kiro / alert).
15. ⭐ Bloc métriques flottant à droite (6 lignes, règle 1 px, barre de progression orange 2 px).
16. Trace des 6 étapes (Ask, Context, Think, Act, Critic, Answer) avec icônes isométriques.
17. En-tête : logo fantôme, CRYPT, tagline, « live · project · model · F7 hub ».
18. Barre de statut : connected · tok/s · p50 · context %.
19. Ligne « thinking » : 3 points orange pulsants + étape en minuscules.
20. Bulle de réponse avec avatar fantôme 26 px et méta-ligne `file +4 −1 · checkpoint N saved`.
21. Remarque de Kiro en italique, indentée, fantôme outline à cornes.
22. Lignes système avec `›` orange (ex. `/rewind`, `F7 hub`), alertes 100 % orange pulsantes.
23. Bouton RUN/RUNNING orange avec glow 30 px, caret clignotant pendant la saisie.
24. Parallaxe des couches UI selon la vitesse caméra (titre 1,5 ; caption 1,2 ; métriques 0,55).

## C. Données et événements (25-36)
25. ⭐ Diff-scan périodique (std::fs, mtime + lignes, sans dépendance) pour capter les écritures hors shell (sous-agents, éditeur).
26. ±lignes exacts : brancher `checkpoint::write_tracked` sur les écritures réelles, ou comparer le nombre de lignes avant/après la commande shell.
27. Détecter les créations/suppressions de fichiers → `file.create` / `file.delete` (dalle qui s'effondre).
28. `project.open` au changement de chat (chaque chat peut avoir son cwd/workspace).
29. Workspaces (`src/workspace.rs`) : un quartier par workspace d'agent, pas seulement le cwd.
30. Sous-agents multi-chats : un fantôme par agent, étiquette du chat propriétaire.
31. Cible de vol d'un sous-agent = le fichier qu'il touche vraiment (déduit de son premier `FileRead`).
32. Recherche web = plateforme flottante de 130 hors du projet (DESIGN §6.1).
33. `checkpoint.saved` = repère au sol (borne/pierre) numéroté.
34. `/rewind` groupé : tous les fichiers depuis le rang N (au lieu d'un snapshot à la fois).
35. Hooks Kiro (`hook_run.rs`) comme événements de scène (tirs = flashs discrets).
36. `/spec run` : les tâches cochées apparaissent comme des dalles « validées » (coche blanche).

## D. Vue hub / multi-projets (37-46)
37. ⭐ Districts réels : plusieurs projets scannés (`list_projects` sur `~/code` configurable), espacés de 3 500-9 000.
38. ⭐ Cadre orange pointillé + « you are here » sur le projet actif (DESIGN §6.1).
39. Nom de chaque projet écrit au sol en grand.
40. Projets distants révélés par le brouillard à mesure qu'on s'éloigne.
41. Stats par projet : nb fichiers, lignes totales, dernière activité (mtime max), dépôt sale ou non.
42. Projets GitHub non clonés en grisé (source optionnelle, hors v1).
43. Navigation hub : flèches/Entrée pour « voler » vers un projet et en faire le projet actif visuel.
44. Mini-carte en coin (tous les districts en points).
45. Tri des districts par activité récente.
46. Transition caméra hub→projet en spline (pas un lerp linéaire).

## E. Lisibilité / labels (47-54)
47. ⭐ LOD des étiquettes : n'afficher que les N fichiers les plus proches/les plus actifs, jamais en hub.
48. ⭐ Anti-chevauchement des étiquettes (boîtes écran, priorité à l'actif/modifié).
49. Chemin complet tronqué au centre (`src/…/render.rs`) quand la place manque.
50. Étiquettes des dossiers uniquement jusqu'à une distance donnée, fichiers au-delà du zoom.
51. Surbrillance de l'étiquette du fichier courant (blanc plein + halo).
52. Étiquettes orientées au sol (perspective aplatie) plutôt que billboards.
53. Police 3×5 ×2 avec lissage de sous-pixel (ou police vectorielle légère maison).
54. Noms de dossiers en petites capitales espacées (+0,16 em comme le design).

## F. Performance (55-64)
55. ⭐ `Basis` de caméra calculé une fois par frame au lieu d'à chaque `project()` (×N appels) — gain majeur.
56. Culling par boîte englobante (dossiers/fichiers hors champ non projetés).
57. Regroupement automatique : > 500 fichiers → une pile par dossier (déjà amorcé, à automatiser par mesure).
58. Découpage du rendu en tranches (rayon tracing de lignes) pour éviter le fillrate inutile des grands aplats.
59. Cache des projections des sommets par frame (chaque dalle re-projette 4 coins).
60. Encode PNG : mesurer et choisir `CompressionType`/`FilterType` selon la scène (déjà Fast+Sub).
61. Saut de frame si les encodeurs sont saturés (pattern `hub/live.rs`) + compteur de fps affiché.
62. Test `frame_budget` ignoré : coût d'une frame 960×540 avec 500 fichiers (comme `haunt/draw.rs:366`).
63. Rendu à 20 fps sur batterie/charge (détection load average) et 30 sinon.
64. Allocation zéro par frame (tampons réutilisés, pattern `Painter` de `haunt/draw.rs`).

## G. Interaction (65-74)
65. ⭐ `--space-shot <png>` : planche PNG hors terminal (pattern `haunt/demo.rs`) pour voir le rendu sans TUI.
66. ⭐ Commande `/space on|off|hub|shot` en plus de F8/F7.
67. F8 persisté dans `settings` (l'état survit au redémarrage).
68. Souris : molette = zoom caméra, drag = yaw/pitch (hors panneau).
69. Clic sur un fichier = le fantôme y va (et `/open` le fichier dans l'éditeur).
70. Raccourci « recentrer sur le fichier actif ».
71. Mode « cinéma » : la caméra suit l'action sans intervention, F8 deux fois.
72. Survol d'une dalle = info-bulle (lignes, dernière modif, auteur git).
73. Toggle des étiquettes (T) et du sol (G) pour déboguer.
74. Vitesse d'animation configurable (`/space speed 0.5|1|2`).

## H. Sémantique git / projet (75-82)
75. Couleur d'aura selon le statut git : modifié (orange), non suivi (blanc pulsé), supprimé (effondrement).
76. Branche courante affichée au sol près du nom du projet.
77. Diff en dalles : +4 −1 = 4 micro-dalles orange, 1 grise qui tombe.
78. « Fichiers chauds » : halo proportionnel au nombre de commits récents.
79. Arbre limité aux fichiers suivis par git (`git ls-files`) plutôt qu'au scan disque.
80. `.gitignore` respecté explicitement (au-delà de target/.git/node_modules).
81. Worktrees git : un district par worktree du même dépôt.
82. PR/CI en cours : un petit fanion au-dessus du projet.

## I. Horreur / ambiance Crypt (83-92)
83. Kiro en balade sur la scène 3D (il traverse les dalles, réagit aux écritures).
84. `fx.rs` adapté : brouillard rouge très rare sur alerte, sans casser la sobriété.
85. Les dalles « meurent » (s'effritent) quand un fichier est supprimé.
86. Fantômes des sous-agents échoués = gris/cendres au lieu de blancs.
87. Jump scare discret : un district lointain s'allume une fois quand un autre chat répond.
88. Poussière et débris au sol après un gros impact (persistent 10 s).
89. Lanternes/points de lumière le long des allées de dossiers.
90. Mode « crypte » : le sol devient des dalles de pierre quand le hub est fermé.
91. Écho sonore visuel (anneaux concentriques) quand le modèle répond.
92. Almanach : les tokens du jour forment une traînée de particules au-dessus du projet.

## J. Robustesse, tests, outillage (93-100)
93. ⭐ Tests de rendu déterministes par golden image (hash de la frame, pas de PNG binaire en base).
94. Test d'invariant : aucune cellule du panneau avec un fond non `Reset` quand la 3D est active.
95. Repli non-Kitty : mode demi-blocs (`hub/render.rs::to_cells`) testé sur terminal sans Kitty.
96. Détection terminale : si pas de Kitty, F8 affiche un message au lieu d'une image cassée.
97. Journal `--space-log` (JSONL des SceneEvent) pour rejouer une session en différé.
98. Rejeu : `--space-replay <jsonl>` pour revoir une session sans LLM.
99. Doc `docs/design/project-space/PLAN.md` + captures dans le dépôt (ce qui est fait ici).
100. Garde CI : budget de lignes/fonctions par fichier du module `space/` vérifié automatiquement.
