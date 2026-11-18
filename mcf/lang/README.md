# 🧪 `mcf/lang/` — lecture du langage MCF (garé)

> **Statut : prototype sur la branche `lang/pont-bevy`** (crate `mcf_lang_bridge`, exemple `lang_demo`), à la demande d'Aroun le 2026-11-18. Pas encore dans `main`. Licence (D67) tranchée le 2026-11-18 : bibliothèques du compilateur en `MIT OR Apache-2.0`. Avant : « garé ». Rien n'est branché au jeu. Ce dossier garde l'idée au chaud et fixe les règles pour le
> jour où on s'y met.

## L'idée

Le langage `.mcf` d'Aroun (`MCF-Studio/MCF-MyLangage`, compilateur en Rust) sait déjà décrire des interfaces
(`.uimcf` : composants, styles, liaisons, vérifiés par `mcf check`, phase 10 de son propre plan). MCF Engine étant
aussi en Rust, on peut **lire ces fichiers directement dans le moteur** :

```
fichier .uimcf ──► crates du compilateur MCF (syntax + semantic) ──► arbre UI vérifié
                                                                     │
                                       mcf_lang_bridge (à créer) ◄───┘
                                                                     │
                                       entités bevy_ui (nœuds, textes, boutons, styles)
```

Usage visé, par ordre de priorité :

1. **Interfaces du jeu** : HUD, téléphone, inventaire, menus, en `.uimcf`, rechargées à chaud pendant le dev.
2. Liaisons de données : l'UI lit des valeurs exposées par le jeu (argent, faim, heure…), en lecture seule.
3. Plus tard, peut-être : petits scripts `.mcf` (événements d'un objet de la carte), via l'interpréteur.

## Conditions pour sortir du garage

- Le compilateur a une **API de bibliothèque stable** pour `.uimcf` (pas seulement la CLI).
- Les phases 1–2 de MCF Engine sont terminées (le moteur ne doit pas attendre le langage).
- La question de licence ci-dessous est tranchée.

## Points de vigilance

- **Licence** : MCF-MyLangage est en **GPL-3.0**, Bevy en MIT / Apache-2.0. Lier le compilateur GPL dans le
  binaire du jeu rend le binaire distribué soumis à la GPL. Aroun étant l'auteur, il peut publier les crates
  nécessaires (`syntax`, `semantic`, `runtime`) sous une double licence, ou garder `mcf_lang_bridge` en
  **feature optionnelle désactivée par défaut**. À décider avant la première ligne de code.
- **Intégration** : dépendance Cargo `git` **épinglée sur un commit** (`rev = "..."`), derrière la feature
  `mcf_lang`. Jamais de copie du code du compilateur ici.
- **Sécurité** : un fichier `.mcf` / `.uimcf` est traité comme non fiable. Bac à sable : pas d'accès fichier,
  réseau ni processus, sauf capacités accordées explicitement par le moteur ; limites de temps et de mémoire.
  Cohérent avec la promesse du langage (chiffrement, aucune télémétrie).
- **Performance** : la compilation de l'UI se fait au chargement (ou à l'import d'assets), jamais à chaque frame.
