# Commandes d’édition MCF

Toutes les modifications persistantes passent par une commande structurée. Une
commande connaît les UIDs de ses cibles, l’état avant/après, son calque et les
préconditions de version. L'interface (raccourci, gizmo, panneau ou futur
réseau) ne modifie pas directement les données sources.

## Commandes du noyau carte

| Commande | Donnée minimale | Annulation | Validation avant application |
|---|---|---|---|
| `CreateItem` | UID, catalogue, transform, propriétés | retire l'UID créé | UID libre, catalogue et propriétés valides |
| `DeleteItem` | élément complet et position dans le calque | réinsère l'élément | UID existant, calque non verrouillé |
| `TransformItems` | UIDs, transforms avant/après, magnétisme | restaure `avant` | bornes coordonnées/échelle, UID existants |
| `EditRoad` | spline avant/après | restaure la spline | points, tangentes, intersections et gabarit valides |
| `EditTerrainTiles` | différences compressées par tuile | applique l'inverse | version des tuiles, bornes de hauteur |
| `EditEnvironment` | paramètres avant/après | restaure `avant` | valeurs autorisées et version du profil |

## Cycle

1. Construire la commande à partir de la sélection et de l'outil actif.
2. Valider les préconditions et la donnée résultante sans écrire de fichier.
3. Appliquer la commande aux données source et reconstruire localement les
   dérivés affectés.
4. Ajouter la commande à l'historique ; une nouvelle commande vide la pile
   Redo.
5. À la sauvegarde, valider le calque complet puis écrire atomiquement.

## État transitoire actuel

L’éditeur applique déjà l’historique de transformations et garde les éléments
créés, dupliqués ou supprimés dans le document en mémoire. La prochaine
itération doit représenter ces opérations par les commandes ci-dessus afin que
la duplication, la suppression, la sauvegarde de récupération et une future
collaboration partagent le même mécanisme.
