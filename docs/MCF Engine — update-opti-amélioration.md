MCF Engine — Dossier de recherche et d'optimisation pour les agents
Document de référence
Rust + Bevy
Version 1.0

Date : 19 novembre 2026
Projet : MCF Engine — MCF Studio
Public cible : agents IA de développement, recherche et optimisation

1. Mission générale
L'objectif est de faire évoluer MCF Engine pour en faire un moteur capable de supporter un jeu 3D de rôle et d'exploration avec :
- Un monde vaste, découpé en secteurs et chargé progressivement.
- Des environnements terrestres et maritimes.
- De l'eau, une météo dynamique et des ambiances environnementales.
- Des personnages, PNJ, véhicules et objets interactifs.
- Un système de navigation et de comportements pour les PNJ.
- Un éditeur interne pour créer des scènes, des cartes et des contenus.
- Un système de scripts pour les comportements configurables et le modding encadré.
- Un multijoueur avec serveur autoritaire.
- Une cible de conception de 60 à 80 joueurs simultanés, à valider par des tests réels.
Le but n'est pas de transformer MCF Engine en assemblage de plugins. Il faut construire une architecture cohérente, performante, modulaire et durable.

La mission des agents : rechercher les meilleures solutions, comprendre leur fonctionnement, récupérer les projets open source intéressants, les comparer avec les systèmes existants et proposer des améliorations mesurables.
2. Consignes obligatoires aux agents
2.1 Comprendre avant de modifier
Avant toute recherche ou modification, l'agent doit :
1. Explorer le dépôt MCF Engine.
2. Comprendre son architecture actuelle.
3. Identifier les systèmes déjà implémentés.
4. Relever les versions de Rust et Bevy.
5. Cartographier les dépendances.
6. Identifier les éventuels problèmes de performances.
7. Éviter de refaire ce qui existe déjà.
Ne jamais partir du principe que le moteur est vide ou qu'une solution trouvée sur GitHub est forcément meilleure.
2.2 Vérifier chaque projet
Pour chaque dépôt ou bibliothèque étudié, vérifier :
- La dernière release.
- La date des derniers commits.
- La version de Bevy compatible.
- La version minimale de Rust.
- La licence open source.
- Les dépendances nécessaires.
- Les exemples disponibles.
- Les issues ouvertes et les problèmes connus.
- La maturité réelle du projet.
- La facilité d'intégration dans MCF Engine.
Attention : Bevy évolue rapidement et introduit régulièrement des changements d'API. Au 19 novembre 2026, la branche stable publiée est Bevy 0.20. La documentation officielle indique notamment la sortie de Bevy 0.20 le 8 octobre 2026. 
bevy.org+1

2.3 Classer les solutions
Chaque projet doit recevoir un statut :
Statut	Signification
A — Candidate sérieuse	Projet maintenu, compatible et intéressant à intégrer
B — À prototyper	Projet prometteur nécessitant des tests
C — Référence technique	Code intéressant à étudier, mais pas forcément à intégrer
D — À écarter	Projet abandonné, incompatible ou trop risqué

2.4 Ne pas intégrer sans validation
Les agents peuvent créer des prototypes isolés et faire des propositions, mais ne doivent pas :
- Remplacer une technologie fondamentale sans comparaison.
- Ajouter une dépendance lourde sans justification.
- Changer les formats de fichiers sans prévoir de migration.
- Introduire plusieurs moteurs physiques.
- Coupler le serveur au renderer.
- Faire une migration globale de Bevy pour un seul plugin.
- Optimiser une fonctionnalité sans avoir identifié un problème mesurable.
Chaque proposition doit apporter une amélioration démontrable.

3. Fondations Bevy et Rust
Priorité critique
Bevy — sources officielles
Ressource	Lien	Travail demandé
Dépôt officiel Bevy	GitHub	Comprendre l'architecture et les crates
Site officiel	bevy.org	Explorer les fonctionnalités
Documentation	Quick Start	Étudier les pratiques recommandées
API Rust	docs.rs	Vérifier les API de la version utilisée
Exemples officiels	GitHub Examples	Rechercher des implémentations existantes
Plugins	Bevy Assets	Explorer les extensions disponibles
Migration guides	Documentation	Anticiper les changements de version
UI	Bevy UI	Étudier l'interface native
Assets	Bevy Assets	Comprendre le pipeline de ressources

Points à étudier
Les agents doivent approfondir :
- ECS et gestion des entités.
- Components, Resources et Bundles.
- Scheduling et exécution des systèmes.
- Events et Observers.
- States.
- Reflection.
- Gestion des assets.
- Scenes et formats de scènes.
- Render pipeline.
- UI.
- Picking et gizmos.
- Asset processing.
- Applications headless.
- Modularité des plugins.
Rust — références
- The Rust Book
- Rust Reference
- Cargo Book
- Rustonomicon
- Rust API Guidelines
- Tokio
- Serde
- docs.rs
Les agents doivent privilégier le Rust sûr. Toute utilisation de unsafe doit être justifiée, isolée et accompagnée de tests.

4. Multijoueur, réseau et serveur dédié
Priorité critique
Le multijoueur est l'un des systèmes les plus importants de MCF Engine.

L'architecture doit permettre de faire tourner un serveur indépendant du client graphique, capable de gérer la simulation du monde, les joueurs, les véhicules, les PNJ et les interactions.
4.1 Bibliothèques à étudier


Lightyear
À étudier en premier
Réseau multijoueur pour Bevy : réplication ECS, prédiction, rollback, interpolation, gestion de bande passante et interest management.
Documentation


bevy_replicon

Réplication d'entités Bevy et gestion de leur visibilité par client. À étudier également pour comprendre les possibilités de réplication sélective.


Renet

Alternative réseau à comparer avec Lightyear, notamment sur le transport et la gestion des connexions.


4.2 Point important : Lightyear et Bevy 0.20
La documentation actuelle de Lightyear indique :
Lightyear	Bevy
0.31	0.20
0.28 à 0.30	0.19
0.26 à 0.27	0.18
0.25	0.17
0.20 à 0.24	0.16

Lightyear 0.31 utilise notamment bevy_replicon pour ses fonctionnalités de réplication et d'interest management. Il propose aussi la prédiction côté client, l'interpolation des snapshots et la gestion des priorités réseau. 
docs.rs+2


Liens à consulter :
- Lightyear GitHub
- Lightyear Docs
- Lightyear Releases
4.3 Architecture réseau à étudier
Les agents doivent proposer une architecture autour de ces principes :
- Serveur autoritaire.
- Validation serveur des actions importantes.
- Réplication sélective des entités.
- Interest management spatial.
- Fréquence de réplication adaptée au type d'entité.
- Interpolation pour les entités distantes.
- Prédiction pour les actions qui en ont réellement besoin.
- Priorités réseau.
- Limitation de la bande passante.
- Gestion des pertes de connexion.
- Reconnexion.
- Synchronisation des états persistants.
4.4 Interest management
C'est un élément essentiel pour MCF Engine.

Un joueur ne doit pas recevoir toutes les informations concernant les autres joueurs et toutes les entités de la carte.

Étudier les systèmes suivants :
1. Grille de cellules.
2. Secteurs définis dans les données de carte.
3. Quadtree ou grille hiérarchique.
4. Zones logiques : intérieur, bâtiment, véhicule, instance.
5. Zones de visibilité spécifiques à certaines interactions.
Prévoir une marge de transition entre deux secteurs pour éviter les apparitions/disparitions brutales des entités.
4.5 Tests réseau obligatoires
Créer un banc de test avec des clients simulés :
Scénario	Objectif
10 joueurs	Vérifier les bases
30 joueurs	Charge intermédiaire
60 joueurs	Première cible importante
80 joueurs	Cible haute
80 joueurs dans une zone	Tester le pire cas
80 joueurs dispersés	Tester le streaming et les secteurs
Rassemblement massif	Tester un événement collectif
Déplacements rapides	Tester les changements de secteurs
Reconnexions simultanées	Tester la résilience

Mesurer :
- CPU serveur.
- RAM serveur.
- Temps de tick.
- Bande passante entrante et sortante.
- Taille moyenne des paquets.
- Entités répliquées par joueur.
- Latence.
- Retard de réplication.
- Nombre de messages par seconde.
Attention : aucune bibliothèque ne garantit à elle seule 80 joueurs. C'est l'architecture complète de MCF Engine qui devra être testée.

5. Physique et collisions
Priorité haute
Deux solutions principales sont à comparer.
Projet	Lien	Intérêt
Avian	GitHub	Physique ECS conçue pour Bevy
Rapier	GitHub	Moteur physique généraliste
bevy_rapier	GitHub	Intégration Rapier avec Bevy

Tests à réaliser
- Déplacement d'un personnage.
- Collisions avec le terrain.
- Pentes et escaliers.
- Objets dynamiques.
- Objets empilés.
- Véhicules.
- Collisions entre véhicules et environnement.
- Physique côté serveur sans renderer.
- Synchronisation des états physiques.
Questions à résoudre
- Quelle solution est la plus adaptée à un jeu de rôle ?
- Quel est le coût de simulation de nombreux objets ?
- Peut-on désactiver les objets éloignés ou inactifs ?
- Comment gérer les véhicules ?
- Comment synchroniser les objets physiques en multijoueur ?
- Quel est le coût d'une simulation physique serveur ?
Ne retenir qu'un backend physique principal.

6. Monde ouvert, terrain et streaming
Priorité critique
MCF Engine doit pouvoir afficher une grande carte sans charger en permanence l'intégralité de son contenu.
6.1 Projets à examiner
Projet	Lien	Intérêt
Bevy Wilderness	GitHub	Terrain et clipmaps
bevy-clipmap	GitHub	Rendu de grands terrains
bevy_terrain	GitHub	Terrain à grande échelle
Saddle World Terrain	GitHub	Streaming de heightfields
big_space	GitHub	Coordonnées et partitionnement de très grands mondes
bevytiles	GitHub	Organisation en tuiles

Ces projets sont des pistes à auditer, pas des dépendances validées. Certains peuvent être expérimentaux ou ne pas suivre la version actuelle de Bevy.
6.2 Techniques à étudier
- Chunks.
- Chargement asynchrone.
- Déchargement progressif.
- Hystérésis pour éviter les chargements répétés aux frontières.
- LOD géométrique.
- LOD des matériaux.
- Occlusion culling.
- Instancing.
- Génération des collisions à la demande.
- Cache disque.
- Cache mémoire.
- Streaming des assets.
- Séparation entre données terrain, rendu et physique.
6.3 Organisation recommandée
Les agents doivent proposer un système dans lequel la carte est divisée en secteurs indépendants.

Chaque secteur peut contenir :
- Terrain.
- Bâtiments.
- Routes.
- Végétation.
- Objets statiques.
- Points d'intérêt.
- Données de navigation.
- Données de collision.
- Références aux assets.
Le chargement doit être piloté par la position des joueurs, la caméra et les besoins de simulation.
6.4 Coordonnées de très grands mondes
Étudier big_space, mais uniquement si les dimensions du monde le justifient.

Une grande carte de jeu classique ne nécessite pas forcément un système de coordonnées spatiales complexes.

7. Eau, météo et environnement maritime
Priorité haute
L'eau est un élément important de l'identité visuelle de MCF Engine.

Il faut étudier la possibilité de construire un système d'eau performant et évolutif, plutôt que de dépendre d'un plugin dont les possibilités seraient limitées.
Recherches à mener
- Surface d'eau avec shaders.
- Normales animées.
- Normal maps.
- Vagues procédurales.
- Vagues de type Gerstner.
- Réflexion.
- Réfraction.
- Transparence.
- Profondeur et absorption de la lumière.
- Écume côtière.
- Transitions entre plage et eau.
- Effets de météo.
- Synchronisation météo serveur/client.
- Vent partagé entre eau, végétation et effets.
Ressources techniques
- Exemples shaders Bevy
- Exemples 3D Bevy
- Spécification WGSL
- WGSL — GPU Web
- Bevy Assets
Travail demandé
Créer plusieurs prototypes d'eau :
1. Eau simple et peu coûteuse.
2. Eau avec vagues et normales animées.
3. Eau avec réflexion et effets avancés.
4. Eau côtière avec écume et profondeur.
Mesurer leur coût GPU et leur impact sur les performances.

La physique des bateaux doit être indépendante du rendu des vagues.

8. Éditeur de scènes et de cartes
Priorité haute
L'éditeur est un élément important de MCF Engine. Il doit permettre de construire des cartes et des scènes sans devoir modifier directement du code à chaque placement d'objet.
8.1 Projets à examiner
Projet	Lien	Travail demandé
Jackdaw	GitHub	Étudier viewport, hiérarchie, inspector et outils
bevy_editor	GitHub	Étudier l'édition de scènes
bevy_experimental_editor	GitHub	Étudier l'architecture d'éditeur

Les dépôts doivent être vérifiés individuellement : activité, compatibilité, licence et état réel du développement.
8.2 Fonctionnalités attendues
L'éditeur doit être capable de proposer :
- Viewport 3D.
- Vue 2D de carte.
- Caméra libre.
- Hiérarchie des entités.
- Inspecteur de propriétés.
- Déplacement, rotation et échelle.
- Sélection multiple.
- Duplication.
- Grille et snapping.
- Undo/redo.
- Sauvegarde et rechargement.
- Import/export.
- Gestion des assets.
- Placement d'objets.
- Peinture de terrain.
- Routes, chemins et rivières.
- Marqueurs et points d'intérêt.
- Propriétés personnalisées.
- Prévisualisation de la météo et de l'heure.
8.3 Architecture à étudier
Il faut séparer :
1. Le format de données de carte.
2. L'éditeur.
3. Le runtime client.
4. Le serveur dédié.
L'éditeur doit pouvoir réutiliser les systèmes du moteur lorsque c'est pertinent, mais le serveur ne doit jamais avoir besoin du renderer pour charger les données de simulation.

Les fichiers de carte doivent être versionnés et prévoir des migrations.

9. UI et contrôles
Projets et ressources
Projet	Lien	Utilisation
Bevy UI	Documentation	Interface du jeu
bevy_egui	GitHub	Interface des outils
egui	GitHub	UI immédiate
bevy-inspector-egui	GitHub	Inspecteur
Leafwing Input Manager	GitHub	Gestion des entrées
Bevy Enhanced Input	GitHub	Entrées contextuelles

Points à comparer
- UI native Bevy contre egui.
- Performances.
- Facilité de personnalisation.
- Gestion des menus complexes.
- Gestion des manettes.
- Remappage des touches.
- Actions contextuelles.
- Support des interfaces de l'éditeur.
Ne pas imposer une seule technologie à toutes les interfaces. L'UI du jeu et l'UI de l'éditeur peuvent avoir des besoins différents.

10. Scripting et modding
Priorité moyenne à haute
Le cœur du moteur doit rester en Rust.

Le scripting doit permettre de rendre certaines mécaniques configurables, de gérer des dialogues et de créer des comportements sans exposer tout le moteur.
Bibliothèques à comparer
Projet	Lien	Intérêt
mlua	GitHub	Intégration de Lua dans Rust
Rhai	GitHub	Langage embarqué dans Rust
Rune	GitHub	Langage de script embarqué

Architecture à proposer
- API publique documentée.
- Événements auxquels les scripts peuvent s'abonner.
- Commandes de gameplay validées par le moteur.
- Handles contrôlés pour les entités.
- Permissions par module.
- Limites de temps d'exécution.
- Limites mémoire.
- Logs par script.
- Gestion des erreurs.
- Chargement et déchargement.
- Versionnement de l'API.
- Séparation des scripts serveur et client.
Les scripts ne doivent pas avoir accès arbitrairement aux ressources internes du moteur.
Prototypes demandés
1. Un PNJ avec dialogues déterministes.
2. Un objet interactif.
3. Un événement déclenché par l'entrée dans une zone.
4. Un comportement configurable par script.
Comparer les performances, la facilité d'écriture et la sécurité de chaque solution.

11. PNJ, navigation et comportements
Ressources
- vleue_navigator
- Bevy Assets
Points à étudier
- Navmesh.
- Pathfinding.
- Obstacles dynamiques.
- Zones interdites.
- Coûts de déplacement.
- Mise à jour locale du chemin.
- Machines à états.
- Behavior trees.
- GOAP, si nécessaire.
- Perception événementielle.
- Tâches planifiées.
- Dialogues déterministes.
Optimisation
Les PNJ éloignés des joueurs ne doivent pas nécessairement être simulés à la même fréquence que ceux qui sont à proximité.

Étudier :
- Réduction de fréquence.
- Simulation simplifiée.
- Activation à proximité.
- Mise en veille.
- Sauvegarde des états importants.
- Réplication des seuls états utiles.
Les décisions qui ont un impact sur le monde doivent être traitées côté serveur. Le client peut gérer leur présentation visuelle et leurs animations.

12. Audio et animation
Audio
- bevy_kira_audio
- Kira
Étudier :
- Audio spatial 3D.
- Bus audio.
- Transitions.
- Effets.
- Musiques.
- Streaming audio.
- Fonctionnement headless.
Animation
- Exemples d'animation Bevy
- glTF
Étudier :
- Animations glTF.
- Transitions entre animations.
- Blend trees.
- Retargeting.
- Synchronisation réseau.
- Coût des squelettes.
- Animation des PNJ.
- Animation des personnages joueurs.

13. Assets et pipeline de contenu
Références
- glTF
- Spécification glTF 2.0
- Blender
- Bevy Assets
- Serde
- RON
Points à étudier
- Import GLB/glTF.
- Conventions d'échelle.
- Orientation des axes.
- Compression des textures.
- LOD des meshes.
- Instancing.
- Validation automatique.
- Génération de thumbnails.
- Métadonnées.
- Références d'assets stables.
- Hot reload.
- Versionnement.
- Séparation entre fichiers source et fichiers compilés.
Livrable attendu : un pipeline reproductible depuis Blender jusqu'à l'intégration dans MCF Engine.

14. Sauvegarde et persistance
Ressources
- Exemple Bevy de sérialisation du monde
- Serde
- SQLx
- Tokio
- RON
Éléments à prendre en charge
- Données de carte.
- État des objets.
- État des joueurs.
- Inventaires.
- Véhicules.
- PNJ persistants.
- Sauvegarde périodique.
- Sauvegarde événementielle.
- Transactions.
- Migrations de données.
- Reprise après crash.
Ne pas sérialiser aveuglément tout le World ECS comme système de sauvegarde de production. Définir des modèles persistants stables, séparés des composants temporaires du runtime.

15. Profiling et optimisation
Priorité critique
Aucune optimisation ne doit être acceptée sans mesure reproductible.
Outils
Outil	Lien	Utilisation
Guide de profiling Bevy	GitHub	Tracing CPU/GPU
Tracy	GitHub	Profiling détaillé
Perfetto	Site officiel	Analyse de traces
RenderDoc	Site officiel	Inspection des frames GPU
cargo-flamegraph	GitHub	Flamegraphs CPU
Criterion	GitHub	Microbenchmarks
cargo-bloat	GitHub	Taille des exécutables
cargo-llvm-lines	GitHub	Analyse de génération de code

Mesures à instrumenter
- Temps de frame client.
- Temps CPU par système ECS.
- Temps GPU.
- Propagation des transforms.
- Nombre d'entités actives.
- Draw calls.
- RAM.
- VRAM.
- Temps de chargement des chunks.
- Nombre d'assets résidents.
- Temps de simulation physique.
- Coût des PNJ.
- Tick serveur.
- Bande passante par client.
- Temps de sauvegarde.
- Temps de compilation.
- Taille des exécutables.
Chaque optimisation doit fournir un avant/après sur la même scène, avec le même matériel et les mêmes paramètres.

16. Architecture cible à faire évaluer
Cette architecture est une proposition de départ. Les agents doivent l'évaluer par rapport au dépôt réel et proposer des ajustements.
MCF Engine Workspace
│
├── mcf_core
│   ├── Types communs
│   ├── Événements
│   ├── Configuration
│   └── Temps et états
│
├── mcf_world
│   ├── Secteurs et chunks
│   ├── Streaming
│   ├── Terrain
│   ├── Eau et météo
│   └── Données de carte
│
├── mcf_physics
│   ├── Backend physique
│   └── Abstractions gameplay
│
├── mcf_network
│   ├── Protocole
│   ├── Réplication
│   ├── Interest management
│   └── Validation serveur
│
├── mcf_gameplay
│   ├── Personnages
│   ├── Véhicules
│   ├── Objets interactifs
│   ├── PNJ
│   └── Dialogues
│
├── mcf_scripting
│   ├── Runtime
│   ├── API publique
│   └── Permissions et quotas
│
├── mcf_assets
│   ├── Chargement
│   ├── Validation
│   └── Pipeline
│
├── mcf_persistence
│   ├── Modèles persistants
│   └── Sauvegardes et migrations
│
├── mcf_client
│   ├── Rendu
│   ├── Caméra
│   ├── UI
│   └── Audio
│
├── mcf_server
│   └── Simulation autoritaire headless
│
└── mcf_editor
    ├── Viewport
    ├── Hiérarchie
    ├── Inspector
    ├── Outils de carte
    └── Import / Export


Principes à préserver
- Le serveur ne dépend pas du renderer.
- Les données de carte sont indépendantes du client.
- Le gameplay ne dépend pas de l'UI.
- Le réseau ne dicte pas toute la structure ECS.
- Les scripts n'accèdent qu'aux API exposées.
- L'éditeur réutilise les crates runtime quand c'est pertinent.
- Les formats de fichiers sont versionnés.
- Chaque crate possède une responsabilité claire.

17. Organisation du travail des agents
Je recommande de faire travailler les agents par spécialité plutôt que de demander à un seul agent de tout optimiser en même temps.
Phase 1 — Audit général
L'agent principal doit examiner le dépôt réel de MCF Engine.

Travail demandé :
1. Identifier toutes les crates.
2. Identifier les plugins utilisés.
3. Relever les versions de Rust et Bevy.
4. Cartographier les dépendances.
5. Repérer les systèmes déjà opérationnels.
6. Identifier les systèmes incomplets.
7. Rechercher les éventuels problèmes de performances.
8. Identifier les dépendances obsolètes ou inutiles.
Livrable : AUDIT_MCF_ENGINE.md
Phase 2 — Recherche spécialisée
Répartir les domaines entre plusieurs agents :
Agent	Mission
Réseau	Étudier Lightyear, Replicon, Renet et le serveur autoritaire
Monde	Étudier les chunks, le streaming, les LOD et les coordonnées
Rendu	Étudier le renderer Bevy, les shaders, l'occlusion et l'instancing
Physique	Comparer Avian et Rapier
Éditeur	Étudier les éditeurs Bevy existants et leurs composants récupérables
Scripting	Comparer Lua, Rhai et Rune
Gameplay	Étudier les PNJ, la navigation, les interactions et les animations
Données	Étudier les assets, formats, sauvegardes et migrations
Performance	Auditer le profiling et identifier les goulots d'étranglement

Chaque agent doit travailler indépendamment, puis transmettre ses conclusions à l'orchestrateur.
Phase 3 — Prototypes
Ne pas intégrer immédiatement les solutions trouvées.

Créer des prototypes indépendants pour tester :
- 80 clients simulés et l'interest management.
- Le chargement d'une grande carte découpée en chunks.
- Une scène dense avec bâtiments et végétation.
- La physique d'un personnage et d'un véhicule.
- Le terrain avec différents niveaux de détail.
- Plusieurs variantes de rendu de l'eau.
- Des dialogues de PNJ pilotés par script.
- La sauvegarde et le rechargement d'une carte.
Phase 4 — Décisions techniques
Chaque décision importante doit être documentée dans un ADR (Architecture Decision Record).

Exemple :

docs/adr/ADR-0001-networking-library.md

Chaque ADR doit contenir :
- Le problème rencontré.
- Les solutions étudiées.
- Les critères de comparaison.
- La solution retenue.
- Les raisons du choix.
- Les conséquences sur l'architecture.
- Les risques.
- Le plan de migration.
Phase 5 — Intégration
- Une fonctionnalité par PR.
- Tests automatisés.
- Documentation.
- Benchmarks avant/après.
- Aucun changement de format non versionné.
- Aucune dépendance ajoutée sans justification.

18. Priorités de développement
Domaine	Priorité	Justification
Serveur et réseau	Critique	Conditionne le multijoueur
Streaming et secteurs	Critique	Conditionne la taille du monde
Profiling	Critique	Permet les décisions basées sur des mesures
Physique	Haute	Personnages, objets et véhicules
Pipeline d'assets	Haute	Production et qualité du contenu
Éditeur de cartes	Haute	Création du monde
PNJ et navigation	Haute	Monde vivant
Eau et météo	Haute	Identité visuelle
Persistance	Haute	Continuité du monde
UI, audio et animation	Haute	Expérience de jeu
Scripting et modding	Moyenne à haute	Extensibilité


19. Format obligatoire des rapports d'agents
Tous les agents doivent rendre leurs recherches avec une structure commune.

# Rapport — [Domaine]

## 1. Résumé exécutif
Conclusion rapide et recommandation.

## 2. État actuel de MCF Engine
Ce qui existe déjà.
Ce qui manque.
Les éventuels problèmes constatés.

## 3. Sources consultées
Pour chaque source :
- URL
- Date de consultation
- Version / release / commit
- Licence

## 4. Solutions comparées
Tableau comparatif :
- Compatibilité Bevy
- Maturité
- Performances
- Licence
- Avantages
- Inconvénients
- Coût d'intégration

## 5. Prototype et tests
- Environnement de test
- Configuration
- Procédure
- Résultats
- Limites

## 6. Recommandation
- Adopter
- Prototyper
- Surveiller
- Rejeter

## 7. Impacts sur MCF Engine
Crates concernées.
Dépendances.
Modifications nécessaires.
Risques.

## 8. Questions restantes
Les points nécessitant une décision.



Les agents doivent impérativement différencier les faits vérifiés, les hypothèses et les opinions.

20. Liste des dépôts à examiner ou récupérer
Voici la liste de départ destinée aux agents qui doivent cloner les projets, lire le code source et récupérer les parties intéressantes.

Important : récupérer le code source ne signifie pas intégrer tout le dépôt. Il peut être plus intéressant de reprendre une approche technique ou un composant isolé.
A. Fondations et réseau
Bevy — Moteur principal

https://github.com/bevyengine/bevy
Ouvrir 


Lightyear — Réseau et réplication

https://github.com/cBournhonesque/lightyear
Ouvrir 


bevy_replicon — Réplication ECS

https://github.com/viridia/bevy_replicon
Ouvrir 


Renet — Alternative réseau

https://github.com/lucaspoffo/renet
Ouvrir 



B. Physique
- Avian
- Rapier
- bevy_rapier
C. Monde et terrain
- Bevy Wilderness
- bevy-clipmap
- bevy_terrain
- Saddle World Terrain
- big_space
- bevytiles
D. Éditeur et outils
- Jackdaw
- bevy_editor
- bevy_experimental_editor
- bevy_egui
- egui
- bevy-inspector-egui
E. Scripting et navigation
- mlua
- Rhai
- Rune
- vleue_navigator
F. Données et persistance
- Serde
- RON
- SQLx
- Tokio
G. Audio, contrôles et pipeline
- bevy_kira_audio
- Kira
- Leafwing Input Manager
- Bevy Enhanced Input
- glTF
- Spécification glTF 2.0
H. Profiling et optimisation
- Tracy
- cargo-flamegraph
- Criterion
- cargo-bloat
- Perfetto
- RenderDoc

21. Sources officielles à surveiller régulièrement
Les agents doivent aussi surveiller les sources suivantes, car elles permettent de découvrir les nouvelles fonctionnalités, les nouveaux plugins et les changements de compatibilité.
Source	Utilité
Bevy News	Nouvelles versions et fonctionnalités
Bevy GitHub	Code, issues et releases
Bevy Assets	Nouveaux plugins communautaires
docs.rs	Versions et documentation des crates
crates.io	Versions publiées et dépendances
Rust Blog	Évolutions du langage
Rust GitHub	Évolutions du compilateur

La veille doit être faite régulièrement. Un projet intéressant aujourd'hui peut devenir obsolète après une évolution majeure de Bevy.

22. Consigne globale à transmettre à l'orchestrateur
Voici le texte que tu peux transmettre directement à ton agent principal.
Mission : audit et optimisation de MCF Engine
Tu es responsable de la recherche, de l'analyse et de l'amélioration technique de MCF Engine.

Tu disposes du dossier de recherche MCF Engine comme index de départ. Tu dois l'utiliser pour organiser le travail de plusieurs agents spécialisés.
Objectif
Faire évoluer MCF Engine, moteur développé en Rust avec Bevy, pour supporter un jeu 3D de rôle et d'exploration avec un monde vaste, des secteurs, du terrain, de l'eau, des personnages, des PNJ, des véhicules, des objets interactifs, un éditeur de cartes et un multijoueur visant 60 à 80 joueurs simultanés.

Cette capacité multijoueur est un objectif à démontrer par des tests, pas une performance présumée.
Méthode de travail
1. Commence par auditer intégralement le dépôt réel de MCF Engine.
2. Comprends les systèmes existants avant de proposer des changements.
3. Identifie les problèmes et les opportunités d'amélioration.
4. Délègue les recherches par domaine à des agents spécialisés.
5. Exige des sources vérifiables et des rapports comparatifs.
6. Vérifie systématiquement les versions de Bevy et Rust, les licences et la maintenance des projets.
7. Fais réaliser des prototypes isolés avant toute intégration.
8. Mesure les performances avant et après chaque optimisation.
9. Propose des décisions d'architecture documentées.
10. Prépare une feuille de route classée par impact, coût, risque et dépendances.
Priorités
Concentre les premiers efforts sur :
- Architecture du serveur dédié.
- Réseau et réplication.
- Interest management.
- Streaming du monde et gestion des secteurs.
- Profiling et performances.
- Physique.
- Éditeur de scènes et de cartes.
Les autres systèmes doivent être étudiés en parallèle, mais ne doivent pas détourner les ressources des fondations du moteur.
Règles
Ne remplace aucune technologie existante sans comparaison.
N'ajoute aucune dépendance sans justification.
Ne modifie pas les formats de fichiers sans plan de migration.
Ne couple pas le serveur au renderer.
Ne fais pas d'optimisation prématurée.
Ne confonds pas popularité GitHub et qualité technique.
Ne présente jamais une hypothèse comme un résultat mesuré.
Résultat attendu
À la fin de l'audit, fournis :
- Un état des lieux complet de MCF Engine.
- Les solutions intéressantes découvertes.
- Les bibliothèques à récupérer ou à tester.
- Les améliorations possibles.
- Les prototypes nécessaires.
- Les risques techniques.
- Une feuille de route priorisée.
- Les décisions nécessitant une validation.
Attends la validation des changements structurels avant de les appliquer.

Ton objectif n'est pas de multiplier les fonctionnalités : c'est de construire un moteur cohérent, performant, maintenable et capable de soutenir les ambitions de MCF Studio.


23. Ma recommandation pour lancer le chantier
Je commencerais par trois groupes de travail en parallèle :
Groupe 1 — Fondations
Priorité 1

Bevy, Lightyear, serveur dédié, physique et architecture générale.

Groupe 2 — Monde
Priorité 2

Streaming, terrain, eau, météo, LOD, végétation et gestion des secteurs.

Groupe 3 — Production
Priorité 3

Éditeur, assets, scripting, PNJ, animation et outils de création.

Un point important : au moment de cette documentation, Bevy 0.20 est sorti et Lightyear 0.31 annonce sa compatibilité avec cette version. C'est donc une combinaison particulièrement intéressante à examiner pour le socle réseau de MCF Engine. 
bevy.org+1


La première étape concrète devrait être l'audit du dépôt actuel, puis un prototype serveur/réseau avec des clients simulés. C'est ce qui permettra de savoir très tôt si l'architecture choisie tient la route avant de construire tout le reste autour.

Principe directeur : mesurer → comparer → prototyper → décider → intégrer


pour l'éditeur un base est disponible ici pour faire le notre : https://github.com/jbuehler23/jackdaw?utm_source=chatgpt.com