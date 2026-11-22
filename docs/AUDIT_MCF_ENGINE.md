# Audit technique initial — MCF Engine

**Date :** 2026-11-21  
**Base inspectée :** workspace `mcf/`, Bevy local 0.20.1, Rust minimal 1.97.1.

## Résumé

Le dépôt dispose déjà de frontières de crates saines pour démarrer les bancs
d'essai : le format `.mcfmap` n'a aucune dépendance au rendu, les composants
communs sont réutilisables côté serveur, et le serveur démarre sans fenêtre ni
GPU. Le risque principal n'est pas une dette de couplage immédiate : ce sont
les fonctionnalités de production encore non démontrées (chargement massif,
streaming, LOD/HLOD, réseau et physique).

## État constaté

| Crate | Rôle vérifié | État / limite constatée |
|---|---|---|
| `mcf_map` | Format `.mcfmap`, validation stricte, lecture et écriture atomique | Données indépendantes de Bevy ; pas de migration de format au-delà de v1. |
| `mcf_core` | UID, référence catalogue, nom de calque, réflexion des composants | Pas de rendu : utilisable côté serveur. |
| `mcf_bench` | Mesures, statistiques, parcours rejouable et export CSV | Sert de socle aux mesures ; les rapports A/B restent à produire. |
| `mcf_big_map` | Génération déterministe de calques de la carte de test A | Aucun rendu ni chargement runtime dans cette crate. |
| `mcf_test_a` | Chargement Bevy des calques générés et modèles mid-poly en code | Prototypage Test A : terrain, streaming et mesures comparables restent à compléter. |
| `mcf_server` | Boucle headless, cadence configurée par `MCF_TICK_RATE` | Aucun transport ni réplication. |
| `mcf_client` | Fenêtre et caméra de démonstration | Squelette, sans gameplay ni chargement de monde. |
| `mcf_editor` | Carte 2D/3D, sélection, transformations, catalogue local, annuler/refaire, sauvegarde atomique | Première base de carte ; pas encore de panneaux dockables, multi-sélection, terrain, courbes ni assets réels. |

## Dépendances MCF déclarées

- **Bevy 0.20.1 local** : fork utilisé par chemin, avec patch explicite des
  sous-crates pour empêcher le mélange entre Bevy local et publié.
- **Serde / serde_json / thiserror** : format et erreurs de carte.
- **bevy_egui 0.43.1** et **egui_dock 0.21.0** : UI d'édition ; les versions
  ont été choisies pour egui 0.36 et Bevy 0.20. L'intégration dockable reste à
  construire.

La licence déclarée par le workspace MCF est `LicenseRef-MCF-Studio-Proprietary`.
Toute dépendance nouvelle doit suivre la règle du projet : compatibilité,
licence, maintenance, prototype et ADR avant intégration.

## Garanties déjà présentes

- Les calques sont traités comme des données non fiables : taille,
  profondeur, champs, UID, bornes et références sont validés avant conversion.
- L'écriture de carte est atomique : fichier temporaire synchronisé, relu puis
  renommé.
- Le serveur utilise `MinimalPlugins`, `ScheduleRunnerPlugin` et `LogPlugin` ;
  aucune dépendance de rendu n'est déclarée dans sa crate.
- Les lints workspace interdisent `unsafe`; les `unwrap` et `todo` sont au
  minimum signalés.

## Risques et manques à traiter avant une décision de migration

1. **Test A incomplet :** pas encore de résultat comparable Godot/MCF pour le
   chargement massif, le terrain, le streaming, LOD/HLOD et l'occlusion.
2. **Test B absent :** aucune bibliothèque réseau choisie ni prototype de
   réplication, bots, dégradation réseau ou interest management.
3. **Physique absente :** aucun backend n'est encore évalué pour personnage,
   véhicule et serveur headless.
4. **Pipeline d'assets absent :** les modèles de test sont générés en code ;
   glTF, métadonnées, collision, LOD et textures compressées sont à définir.
5. **Éditeur de carte précoce :** la première interface est fonctionnelle,
   mais il ne faut pas la présenter comme le critère de sortie de la phase 3
   avant test utilisateur et preuves associées.

## Prochain ordre de travail recommandé

1. Compléter le chargeur/runtime et le parcours rejouable du Test A, puis
   publier les mesures brutes.
2. Prototyper les candidats réseau dans des crates isolées et rédiger une ADR
   seulement après résultats comparables.
3. Maintenir l'éditeur en petites étapes : commandes structurées, gestion de
   calques/visibilité, vraie bibliothèque d'assets, sauvegarde de récupération
   et test utilisateur.
