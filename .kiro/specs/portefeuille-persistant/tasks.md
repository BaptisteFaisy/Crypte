# Portefeuille persistant — tasks

- [ ] 1. Crée src/wallet.rs : type WalletFile, load, save atomique et chemin XDG, avec tests unitaires à chemins injectés.
  Détail : couvre fichier absent, JSON invalide jamais écrasé, fusion par max ; aucune écriture hors du chemin injecté.
- [ ] 2. Ajoute earned_base et spent_base à Shop et fais-en la base du solde affiché.
  Détail : solde = (base + session) borné à zéro ; les tests boutique existants restent verts.
- [ ] 3. Charge le portefeuille au démarrage avant le premier rendu de la boutique.
  Détail : absent = zéros silencieux ; fautif = zéros en mémoire, fichier préservé, erreur gardée pour /usage.
- [ ] 4. Sauvegarde à chaque achat réussi et à la sortie normale.
  Détail : relire puis fusionner par max avant d'écrire ; échec = session mémoire inchangée.
- [ ] 5. Affiche les cumuls vie entière et l'erreur éventuelle dans /usage.
  Détail : textes UI en anglais, aucune nouvelle couleur (theme.rs uniquement).
- [ ] 6. Documente dans le README (§ Shop) et l'AGENTS.md (§ Archi).
  Détail : fini le « current session », chemin wallet.json, règle du max multi-instances.
- [ ] 7. Vérifie : cargo test vert, cargo fmt --check, --smoke, seuils 500 lignes / 30 fonctions sur les fichiers touchés.
  Détail : build 0 warning ; en cas de dépassement, découper, jamais laisser grossir.
