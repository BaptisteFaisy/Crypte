# Portefeuille persistant — requirements

Aujourd'hui, les pièces de la boutique (F6) n'existent que pour la
session en cours : quitter la TUI efface les pièces gagnées et les
pièces dépensées. Cette feature conserve le portefeuille entre les
sessions, comme l'almanach conserve déjà les tokens par jour. Le
change reste inchangé : 1 token consommé = 1 pièce gagnée.

## 1. Conservation du solde entre les sessions

**User story.** En tant que joueur de la crypte, je veux que mes
pièces survivent au redémarrage, pour pouvoir économiser en vue des
gros articles (eau bénite, 900 pièces) sur plusieurs soirs.

- R1. QUAND la TUI démarre ET QU'un fichier portefeuille valide existe,
  ALORS LE SYSTÈME DOIT recharger les pièces gagnées et dépensées
  cumulées avant le premier affichage de la boutique.
- R2. QUAND un achat réussit, ALORS LE SYSTÈME DOIT sauvegarder aussitôt
  les pièces gagnées et dépensées cumulées sur disque.
- R3. QUAND la TUI se ferme normalement, ALORS LE SYSTÈME DOIT
  sauvegarder les pièces gagnées et dépensées cumulées sur disque.
- R4. QUAND aucun fichier portefeuille n'existe (premier lancement),
  ALORS LE SYSTÈME DOIT partir de zéro pièce gagnée et zéro dépensée,
  sans erreur ni message.

## 2. Fichier illisible ou corrompu

**User story.** En tant que joueur, je veux que la boutique continue
de marcher même si son fichier est abîmé, pour ne jamais perdre ma
soirée à cause d'un JSON cassé.

- R5. QUAND le fichier portefeuille est illisible ou invalide, ALORS LE
  SYSTÈME DOIT repartir de zéro en mémoire, ne jamais écraser le
  fichier fautif, et signaler l'erreur dans `/usage`.
- R6. QUAND une sauvegarde échoue (disque plein, permissions), ALORS LE
  SYSTÈME DOIT garder la session en mémoire telle quelle et signaler
  l'erreur dans `/usage`, sans fermer ni bloquer la boutique.

## 3. Plusieurs cryptes ouvertes à la fois

**User story.** En tant que joueur, je veux ouvrir deux cryptes sans
qu'elles s'effacent mutuellement leurs pièces.

- R7. QUAND deux instances sauvegardent le portefeuille, ALORS LE
  SYSTÈME DOIT fusionner en gardant les plus grands cumuls (gagnées
  et dépensées), jamais écraser un cumul plus élevé par un plus bas.

## 4. Affichage

**User story.** En tant que joueur, je veux voir mes totaux de vie
entière, pour savoir combien j'ai gagné et dépensé depuis toujours.

- R8. QUAND j'ouvre `/usage`, ALORS LE SYSTÈME DOIT afficher les pièces
  gagnées et dépensées cumulées sur toutes les sessions, en plus du
  solde courant.
- R9. QUAND j'ouvre la boutique (F6), ALORS LE SYSTÈME DOIT afficher le
  même en-tête qu'aujourd'hui (solde + gagnées + dépensées), les
  chiffres devenant les cumuls vie entière.

## 5. Non-objectifs

- Aucun chiffrement : les pièces n'ont aucune valeur, contrairement aux
  clés API du coffre `/settings`.
- Aucune remise à zéro automatique ou manuelle des cumuls.
- Aucun transfert ni partage du portefeuille entre machines.
