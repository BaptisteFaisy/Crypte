# Brief : vue 3D des projets (fond de l'écran)

> Tout est dans ce dossier `docs/design/project-space/` :
> - `DESIGN.md` : la spec.
> - `crypt-lab.html` : le prototype.
> - `apercu-vue-app.png` : l'écran cible.

---

Tu travailles sur **Crypt** (https://github.com/BaptisteFaisy/Crypte). C'est un chat LLM en Rust (ratatui) mis en scène dans une crypte hantée. Lis d'abord le README et AGENTS.md pour comprendre l'architecture.

## Ce que fait Crypt (à garder en tête)

Une requête passe par six étapes :

1. Le prompt.
2. Le contexte : AGENTS.md, les fichiers de steering et les skills.
3. L'appel au modèle.
4. La boucle d'outils, avec des sous-agents lancés par `/agent`.
5. Kiro, le critique, qui tourne en parallèle.
6. La réponse, avec des checkpoints (`/rewind`) et les tokens convertis en coins.

Le hub (F7) liste les projets.

**Existant à réutiliser :** `src/hub.rs` et `src/hub/` (`iso.rs` pour la projection isométrique, `render.rs` pour le dessin, `live.rs`) simulent déjà une crypte en dalles où vivent les agents. Pars de là : réutilise ce qui peut l'être, et signale ce que le nouveau design change. Par exemple, les sous-agents sont ici des fantômes blancs qui tombent, alors que le hub actuel les dessine en petits fantômes noirs qui s'endorment.

## Objectif

Ajouter un **fond en 3D qui montre les projets de l'utilisateur**, et surtout **le projet sur lequel il travaille en ce moment**. On doit voir où l'agent travaille dans le code, en direct.

Exemple de référence : quand on travaille sur le projet `crypt` lui-même, on voit son arborescence en 3D. Le fantôme principal va sur les fichiers qu'il lit ou modifie.

## Correspondances visuelles (à respecter)

| Élément réel | Représentation 3D |
|---|---|
| Un projet (dépôt) | Un quartier posé au sol, avec son nom. Le projet actif est entouré d'un cadre orange pointillé, avec la mention « you are here ». |
| Un dossier | Une plaque rectangulaire au sol (contour fin), avec son chemin écrit au sol. |
| Un fichier | Une pile de dalles translucides, une dalle pour environ 150 lignes. Le nom du fichier apparaît au-dessus. |
| L'agent principal | Un fantôme blanc en 3D qui flotte au-dessus du fichier qu'il traite. |
| Lecture d'un fichier | Un faisceau blanc du fantôme vers le fichier, dont le dessus s'éclaire en blanc. |
| Écriture d'un fichier | Le fichier s'allume en orange. Une dalle s'ajoute s'il grossit. |
| Fichier modifié | Il garde une aura orange légère tant que la modification existe. |
| Sous-agent (`/agent`) | Un petit fantôme sort du principal et va sur son fichier, ou sur une plateforme qu'il construit hors du projet (recherche web). À la fin, il est poussé dans le vide et tombe. Sa chute fait un impact au sol : anneaux et poussière. |
| Kiro | Un petit fantôme à cornes orange posé sur un perchoir près du fichier qu'il juge. Il est poussé à la fin, lui aussi. |
| `/rewind` | Les dalles créées depuis le checkpoint se détachent et s'effondrent. La caméra chute avec elles. |
| Hub (F7) | La caméra s'éloigne et montre tous les projets. |

## Données à brancher

- **Liste des projets :** depuis les dossiers locaux, pas GitHub.
  - Le projet actif est le dossier où Crypt est lancé.
  - Les autres projets sont les sous-dossiers contenant un `.git` dans une liste configurable de dossiers de travail, par exemple `~/code`.
  - GitHub pourra s'ajouter plus tard comme source optionnelle.
  - Détails en §9 de DESIGN.md.
- **Arborescence et taille des fichiers :** lire l'arbre du projet actif en ignorant `target/`, `.git/` et `node_modules`. Compter les lignes par fichier.
- **Événements en direct** (fichier lu, fichier écrit, sous-agent lancé ou terminé, Kiro, checkpoint, rewind) : repérer où ils se produisent dans le code. Les exposer sous forme d'événements typés, sans coupler le rendu à la logique de l'agent.

## Rendu

**Disposition voulue :**

- La vue 3D occupe **tout le fond** de l'écran.
- Le chat (conversation, trace des étapes, saisie) est un **rectangle semi-transparent posé par-dessus**, sur une partie de l'écran, par exemple la colonne de gauche.
- On voit la 3D à travers ce rectangle, légèrement floutée, et tout autour de lui.
- Le reste de l'interface (métriques, titre de l'étape en cours) flotte aussi au-dessus de la 3D.

**Point technique à trancher dans ton plan :** Crypt est aujourd'hui une app terminal (ratatui), et un terminal ne sait pas afficher une vraie 3D derrière un panneau semi-transparent. Propose la meilleure façon d'obtenir cette disposition, en comparant par exemple :

- **(a)** Une fenêtre graphique pour Crypt (wgpu ou bevy en Rust, ou une webview comme Tauri avec three.js), où le chat est redessiné en panneau translucide au-dessus de la scène.
- **(b)** Rester dans le terminal et simuler l'effet : rendu 3D en braille ou en demi-blocs sur tout l'écran, avec le panneau du chat qui assombrit les cellules derrière lui. Le résultat est plus pauvre visuellement.

Donne les avantages et inconvénients de chaque option et recommandes-en une, avant de coder.

## Style (obligatoire)

- **Fond :** noir `#050505`.
- **Fantômes et lignes :** blanc `#F5F5F3`.
- **Orange `#FF5B1F` :** uniquement pour ce qui est actif ou modifié. C'est le seul accent de couleur.
- **Dalles et sol :**
  - Les dalles sont translucides avec des arêtes fines.
  - Le sol est une grille de points.
  - Un brouillard noir fait apparaître les projets lointains au fur et à mesure.
- **Ambiance :** très sobre et moderne. Aucun décor gratuit.
- **Référence complète :** dans `docs/design/project-space/`.
  - **`DESIGN.md`** : la spec. Couleurs, typo, disposition au pixel, panneau du chat, objets 3D et leurs tailles, et surtout la table « événement → rendu » avec les durées.
  - **`crypt-lab.html`** : le prototype qui marche. Ouvre-le dans un navigateur et clique sur « ⤢ Vue app » pour voir l'écran cible. Son code three.js sert de référence pour les matériaux, la caméra, les chutes et les impacts.
- **Suis DESIGN.md.** Si quelque chose est impossible techniquement, dis-le et propose l'équivalent le plus proche.

## Contraintes

- Ne casse pas le chat existant. La vue 3D doit pouvoir s'activer et se couper (propose un raccourci, par exemple F8).
- Performances : au moins 30 images par seconde sur un projet de 500 fichiers. Au-delà, regrouper les fichiers d'un dossier en une seule pile.
- Ajoute des tests pour le passage des événements de l'agent vers l'état de la scène.

## Livrable attendu

1. **Avant de coder :** un court plan d'architecture qui indique où brancher les événements, quel rendu, et les fichiers touchés.
2. **Une première version :** le projet actif en 3D. Le fantôme se déplace sur les fichiers lus et écrits pendant une vraie requête.
3. **Ensuite :** les sous-agents et leurs chutes, Kiro, `/rewind`, puis la vue de tous les projets.

Pose-moi tes questions si un point bloque, plutôt que de supposer.
