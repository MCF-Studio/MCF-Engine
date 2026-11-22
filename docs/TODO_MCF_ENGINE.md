# To-do MCF Engine — synthèse opérationnelle

> Synthèse de `MCF_Engine_Specification_Editeur_Scenes_Cartes.md` et
> `MCF Engine — update-opti-amélioration.md`, alignée sur `ROADMAP.md`.
>
> Règle de travail : **mesurer → comparer → prototyper → décider → intégrer**.
> Une case ne se coche qu'avec le test, le rapport ou l'ADR associé.

## Cap et ordre de priorité

La priorité immédiate est la phase 2 du projet : déterminer, avec des mesures
comparables à Godot, si MCF Engine convient au monde ouvert et au multijoueur.
L'éditeur, le terrain, l'eau et le modding ne doivent pas faire dériver ce
chantier avant cette décision.

1. Finaliser les deux bancs d'essai décisifs : grosse carte puis réseau.
2. Prendre une décision explicite : continuer, réorienter ou arrêter.
3. Mettre en place l'éditeur de base et les fondations de production.
4. Industrialiser monde ouvert, réseau et contenu selon les résultats.

## 0. Cadrage et décisions à consigner avant les chantiers concernés

- [x] Créer un répertoire `docs/adr/` et le modèle d'ADR (problème,
  alternatives, critères, décision, conséquences, risques, migration) :
  [modèle](adr/ADR-0000-template.md).
- [x] Mettre à jour le document de spécification : Bevy 0.20.1 est déjà le
  socle du dépôt ; ne pas laisser entendre qu'il faut encore choisir la version.
- [ ] Réconcilier la stratégie de scripting : la feuille de route gare le
  langage `.mcf` / `.uimcf`, tandis que la spécification propose Lua. Décider
  s'il s'agit d'une alternative, d'un complément limité ou d'un besoin écarté.
- [x] Confirmer la cible réseau de référence : le `ROADMAP.md` demande
  32 → 256 bots ; le dossier de recherche parle de 60–80 joueurs. Définir les
  paliers et les seuils officiels dans `mcf/docs/bancs-essai.md`.
- [ ] Désigner la machine de référence et, si possible, une configuration
  « joueur moyen » ; archiver matériel, pilotes, OS et paramètres de chaque run.
- [x] Définir les critères de comparaison Godot / MCF : scène, parcours,
  résolution, profil de build, durée de chauffe et métriques.
- [x] Interdire toute dépendance ou changement de format sans : compatibilité
  Bevy/Rust vérifiée, licence, maintenance, prototype et ADR si structurant.

## 1. Phase 2 — banc d'essai A : grande carte (bloquant)

- [x] Générer de façon déterministe la carte de référence (4 × 4 km, extensible
  à 8 × 8 km).
- [ ] Charger les calques générés dans Bevy avec des modèles mid-poly réels.
- [ ] Ajouter un terrain de test compatible avec le scénario et la collision
  nécessaire au parcours.
- [ ] Définir et enregistrer le parcours caméra rejouable.
- [ ] Construire les paliers de charge définis dans le protocole : bâtiments,
  objets, végétation, lampes et personnages animés.
- [ ] Prototyper les LOD : règles par catégories, hystérésis et visualisation.
- [ ] Prototyper le HLOD par cellule ; mesurer son impact sur CPU, GPU, mémoire
  et appels de dessin lorsque la métrique devient disponible.
- [ ] Prototyper le streaming par cellules : chargement asynchrone,
  préchargement directionnel, annulation des tâches obsolètes et budgets RAM/VRAM.
- [ ] Évaluer l'occlusion culling en rendu forward, séparément pour ville et
  intérieurs ; ne l'activer que si le bénéfice est mesuré.
- [ ] Mettre en place l'index spatial requis par sélection, streaming, culling
  et opérations locales ; éviter tout balayage global par frame.
- [ ] Instrumenter et exporter les mesures brutes : frame time moyen/p99/1 %
  low, CPU par système, GPU, RAM/VRAM, triangles, entités/secteurs actifs,
  chargements et draw calls si accessibles.
- [ ] Reproduire la même scène et le même parcours sous Godot 4.7.
- [ ] Documenter résultats, limites et conclusion dans
  `mcf/docs/bancs-essai.md` ; ne pas masquer les chiffres défavorables.

**Critère de sortie :** rapport brut et comparable, puis décision d'Aroun.

## 2. Phase 2 — banc d'essai B : réseau (bloquant)

- [ ] Auditer Lightyear, bevy_replicon + renet et le réseau Godot actuel :
  compatibilité, licence, maintenance, intégration, prédiction, interpolation
  et interest management.
- [ ] Rédiger l'ADR de la bibliothèque réseau après prototypes, pas avant.
- [ ] Créer un serveur dédié headless qui ne dépend d'aucun renderer.
- [ ] Construire un générateur de bots et un émulateur de réseau dégradé
  (latence, gigue, perte, reconnexions).
- [ ] Prototyper l'interest management spatial par cellules, avec marge de
  transition ; prévoir les cas intérieur, véhicule et zone logique.
- [ ] Tester les paliers 32, 60, 80, puis 128 et 256 bots, en cas dispersé et
  rassemblement dense, avec déplacements rapides et reconnexions.
- [ ] Mesurer ticks/s et durée p99, CPU, RAM, bande passante entrée/sortie,
  taille des paquets, messages/s, entités répliquées/client et retard de
  réplication.
- [ ] Comparer les résultats au réseau Godot dans des conditions équivalentes.
- [ ] Consigner rapport brut, limites et recommandation dans
  `mcf/docs/bancs-essai.md`.

**Critère de sortie :** rapport brut et comparable, puis décision d'Aroun.

## 3. Socle transversal à livrer avant l'industrialisation

- [x] Produire l'audit du dépôt : crates, plugins, versions Rust/Bevy,
  dépendances, systèmes opérationnels/incomplets et risques de performance :
  [audit initial](AUDIT_MCF_ENGINE.md).
- [x] Maintenir un inventaire des dépendances étudiées avec statut : candidate,
  à prototyper, référence technique ou écartée :
  [inventaire](DEPENDANCES_ETUDIEES.md).
- [x] Standardiser le rapport de prototype : sources et versions, licences,
  environnement, procédure reproductible, résultats, limites et recommandation :
  [modèle](RAPPORT_PROTOTYPE_TEMPLATE.md).
- [ ] Compléter le diagnostic avec profiling CPU/GPU, traces, mémoire, assets
  résidents, chargement de secteurs, physique, réseau et taille des binaires.
- [ ] Garantir des mesures avant/après sur même matériel, scène et paramètres.
- [ ] Conserver une séparation stricte : données de carte indépendantes du
  client ; serveur autoritaire headless ; gameplay indépendant de l'UI ;
  editor réutilisant le runtime sans être requis par lui.
- [ ] Versionner chaque format et écrire les migrations explicites ; ne jamais
  sérialiser aveuglément le `World` ECS comme persistance de production.

## 4. Éditeur MCF — noyau (après décision phase 2)

- [ ] Valider le socle UI retenu (`bevy_egui` + `egui_dock`) et étudier Jackdaw,
  `bevy_editor` et `bevy_experimental_editor` comme références, sans importer
  de code ou dépendance sans audit.
- [x] Définir les données source (objets, courbes, terrain, environnement) et
  les données dérivées reconstructibles (meshes, collisions, LOD, caches) :
  [architecture des données](ARCHITECTURE_DONNEES_CARTE.md).
- [x] Définir les commandes d'édition structurées pour placement,
  transformation, terrain, courbes, eau et environnement : elles pilotent
  Undo/Redo, sauvegarde, validation et future collaboration :
  [contrat des commandes](COMMANDES_EDITION.md).
- [ ] Livrer le viewport 3D, caméra éditeur, sélection multiple, gizmos,
  snapping, transformation précise, duplication et suppression.
- [ ] Livrer hiérarchie, dossiers, calques, visibilité/verrouillage et
  inspecteur contextuel ; la hiérarchie visuelle ne doit pas imposer un parent
  ECS par dossier.
- [ ] Livrer Asset Browser : recherche, tags, aperçu, métadonnées, glisser-
  déposer, informations collision/LOD/dépendances.
- [ ] Livrer sauvegarde atomique, autosave, récupération après crash,
  Undo/Redo par deltas et validation de cohérence.
- [ ] Livrer le mode « Essayer » : exécuter la scène puis revenir exactement à
  l'état d'édition.
- [ ] Valider le critère utilisateur de la roadmap : un contributeur novice
  pose, déplace et enregistre 20 objets en moins de 10 minutes avec l'aide
  intégrée seulement.

## 5. Éditeur MCF — carte et monde

- [ ] Ajouter vue 2D orthographique : grille, coordonnées, zoom au curseur,
  recherche, sélection rectangulaire et synchronisation immédiate avec la 3D.
  La convention de source 2D et de conversion des routes/zones est définie dans
  [CONVENTION_CARTE_2D_VERS_MCFMAP.md](CONVENTION_CARTE_2D_VERS_MCFMAP.md) ; le
  convertisseur et le type de zones natif restent à implémenter.
- [ ] Ajouter scènes, sous-scènes/prefabs, instances, surcharges et références
  stables ; un dev doit pouvoir créer une porte animée depuis l'éditeur.
- [ ] Concevoir terrain par tuiles : heightmap, matériaux, masques,
  collision, végétation, LOD, version et continuité aux frontières.
- [ ] Livrer outils terrain initiaux : monter, creuser, lisser, aplanir,
  pente, plateformes et aperçu pour les grandes opérations.
- [ ] Livrer peinture de sol multicouche avec limites explicites de rendu et
  mémoire, plus reconstruction locale asynchrone.
- [ ] Concevoir l'outil de courbes commun : points/tangentes, édition,
  séparation des données source et du mesh dérivé.
- [ ] Étendre les courbes aux routes, chemins, rivières, intersections,
  collisions et raccords terrain ; valider raccords hydrologiques.
- [ ] Prototyper eau en niveaux de qualité (simple → vagues → réflexions →
  côte/profondeur), mesurer le coût GPU et garder la physique des bateaux
  indépendante du rendu des vagues.
- [ ] Ajouter heure, météo, éclairage et prévisualisation ; tester baie, port,
  pleine mer et rivière sous plusieurs conditions.
- [ ] Ajouter marqueurs, zones et chemins IA comme données de gameplay
  exportables, sans dépendance à l'éditeur.

## 6. Production, contenu et gameplay (après fondations validées)

- [ ] Choisir un unique backend physique après prototype Avian/Rapier :
  personnage, pentes, véhicules, objets dynamiques, serveur headless et réseau.
- [ ] Définir pipeline Blender → glTF → métadonnées : axes/mètres, validation,
  collision, LOD, thumbnails, textures KTX2/Basis et séparation source/compilé.
- [ ] Mettre en place persistance stable : objets, joueurs, inventaires,
  véhicules et PNJ ; migrations, transactions, sauvegardes et reprise après crash.
- [ ] Étudier navigation/PNJ : navmesh, obstacles, LOD de simulation, mise en
  veille à distance et réplication des seuls états utiles.
- [ ] Ajouter animation et audio adaptés au runtime, dont support headless et
  besoins RP (spatialisation, bus, voix de proximité à terme).
- [ ] N'aborder le scripting/modding qu'après décision de stratégie : API
  limitée et versionnée, capacités explicites, quotas temps/mémoire, aucun accès
  fichier/réseau/processus arbitraire, validation serveur des effets partagés.

## 7. Import, export et profils de permissions

- [ ] Définir manifeste de carte : identifiants, versions, dépendances,
  empreintes, licences, permissions, scripts et taille.
- [ ] Implémenter validation stricte avant import/export : version, références,
  IDs, assets, dépendances, scripts, bornes et autorisations.
- [ ] Importer dans un espace isolé et ne jamais écraser un contenu existant
  silencieusement ; produire un rapport erreurs/avertissements/informations.
- [ ] Prévoir export total ou sélectif (secteur, scène, prefab, sélection) avec
  fermeture contrôlée des dépendances.
- [ ] Appliquer les profils studio/moddeur par capacités réellement vérifiées,
  jamais en masquant seulement des boutons.

## 8. Décisions de sortie et traçabilité

- [ ] Après A et B, produire une note de décision : poursuivre, réorienter ou
  arrêter, avec les mesures qui la justifient.
- [ ] Pour chaque système retenu, publier l'ADR et le plan d'intégration par PR.
- [ ] Mettre à jour `ROADMAP.md` uniquement avec preuves : lien vers test,
  rapport ou démonstration enregistrée.
- [ ] Garder le périmètre initial maîtrisé : pas de collaboration temps réel,
  éditeur de mesh/audio/animation complet, générateur de ville ou IDE de scripts
  avant les fondations et les tests de production.
