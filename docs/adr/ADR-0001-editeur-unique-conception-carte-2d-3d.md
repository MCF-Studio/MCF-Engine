# ADR-0001 — Conception de carte 2D intégrée à l'éditeur, noyau de géométrie unique

- **Statut :** accepté
- **Date :** 2026-11-22
- **Décideurs :** Aroun (carte blanche technique donnée le 2026-11-22)
- **Remplace / est remplacé par :** aucune. Rend obsolète `CONVENTION_CARTE_2D_VERS_MCFMAP.md`.

## Contexte et problème

Aroun veut concevoir la carte du jeu **vue de dessus** (relief, réseau routier hiérarchisé, quartiers, bâtiments),
puis la retrouver et la retoucher en 3D, avec une exigence ferme : un bâtiment en bord de route en 2D l'est au
millimètre en 3D, dans les deux sens d'édition.

L'idée initiale était un outil séparé (« MCF-MapCreator », Tauri 2 + Vue 3) qui exporterait vers l'éditeur Bevy.

Ce qu'on constate dans `mcf_editor` (le 2026-11-22) :
- le plan 2D est **dessiné une seconde fois** avec egui, alors que la 3D a sa propre géométrie ;
- les routes sont des pavés par segment : trou côté extérieur des virages, chevauchement côté intérieur ;
- les largeurs de route sont codées en dur, et `main.rs` fait 2 150 lignes.

## Alternatives étudiées

| Alternative | Compatibilité Bevy/Rust | Licence / maintenance | Prototype | Avantages | Risques / coût |
|---|---|---|---|---|---|
| A. Outil séparé Tauri/Vue + export vers l'éditeur | Tauri peut dépendre de `mcf_map` ; la géométrie 2D serait en TS | une appli de plus à maintenir | aucun | pile web familière | **deux noyaux de géométrie** → divergence ; aller-retour et fusion des modifications faites des deux côtés |
| B. Vue Plan dans `mcf_editor` + crate `mcf_geo` partagée | natif, Rust pur sans Bevy (utilisable par le serveur) | interne | rubans et plan actuels = base existante | **une source de géométrie**, un historique, édition croisée en direct | refonte de la vue 2D actuelle |

## Critères de décision

- Exactitude 2D = 3D démontrable par un test automatique.
- Pas de procédure d'export / import pendant le travail courant.
- Données utilisables telles quelles par le client et le serveur headless.
- Aucune nouvelle dépendance externe pour le noyau.

## Décision

Alternative **B**.
- `mcf_geo` : crate Rust pure (`f64`, déterministe, sans Bevy). C'est la seule à calculer les tracés, emprises,
  carrefours, îlots, parcelles, blocs et le terrain final.
- La vue Plan est une caméra orthographique sur la même scène, avec un style de rendu « plan ». egui ne dessine plus
  de géométrie.
- Les routes sont **dessinées** (tracé + classe) et leur géométrie est générée. Les chemins de terre sont peints dans
  le matériau du sol.
- Les projets de carte sont stockés en mètres, par famille de données, dans le dépôt du jeu (`cartes/<nom>/`),
  avec les PNG en Git LFS.
- On repart sur une **nouvelle convention propre** : les routes et carrefours v1 de `.mcfmap` et l'import
  `.mcf2d.json` sont retirés (import dès J1 ; routes v1 de `.mcfmap` au jalon J3). Aucune carte réelle n'en dépend.

Détail : `docs/SPEC_CONCEPTION_CARTE_2D_3D.md`.

## Conséquences

- Nouvelle crate `mcf/crates/mcf_geo`. `mcf_editor` est découpé en modules (J1).
- `.mcfmap` ne garde que les objets posés et les `base_edits`. Le réseau, les quartiers, le bâti et le relief ont
  leurs propres fichiers versionnés et validés strictement.
- Un test d'exactitude 2D/3D (écart ≤ 1 mm) tourne en CI.
- Abandon si le style « plan » rendu par Bevy ne tient pas 60 i/s sur une carte de 5 km après optimisation (mesure
  en `bench-release`). Repli : rendre le plan en tuiles pré-calculées, toujours depuis `mcf_geo`.
