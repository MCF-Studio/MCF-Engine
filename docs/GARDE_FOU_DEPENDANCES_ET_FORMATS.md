# Garde-fou — dépendances et formats

Ce contrôle est obligatoire avant toute modification de `Cargo.toml`, de
`Cargo.lock` ou d'un format persistant (`.mcfmap`, manifeste, cache compilé).
Il s'applique aussi aux dépendances transitoires introduites par une crate.

## Dossier de preuve minimal

- Version exacte, source et date de consultation.
- Compatibilité compilée avec Rust et Bevy 0.20.1 du dépôt.
- Licence, mainteneurs, activité et avis de sécurité connus.
- Prototype isolé, reproductible, avec commande, machine et résultat brut.
- Effet sur client, serveur headless, taille binaire, mémoire et licence.
- ADR accepté si la dépendance change une frontière d'architecture.

Sans ces éléments, le statut reste `à prototyper` dans
[`DEPENDANCES_ETUDIEES.md`](DEPENDANCES_ETUDIEES.md) : aucune intégration de
production n'est autorisée.

## Formats persistants

Avant de modifier un format :

1. incrémenter ou justifier explicitement la version ;
2. écrire la migration aller et le comportement de refus pour une version
   inconnue ;
3. conserver des échantillons anciens et nouveaux dans les tests ;
4. vérifier bornes, références, tailles et permissions à l'import ;
5. prouver l'écriture atomique et l'absence de fichier temporaire après succès
   ou échec.

Une migration qui ne préserve pas les données doit être refusée par défaut et
exiger une exportation explicite de sauvegarde.

## Revue avant fusion

La PR doit lier l'inventaire, le prototype et, si requis, l'ADR. Le relecteur
vérifie que les tests couvrent les échecs attendus et qu'aucun choix de format
ou de bibliothèque n'est présenté comme définitif sans mesure.
