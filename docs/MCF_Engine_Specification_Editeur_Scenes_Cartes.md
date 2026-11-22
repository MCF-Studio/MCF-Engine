# MCF Engine --- Spécification fonctionnelle de l'éditeur de scènes et de cartes

**Version :** 0.1 --- Document de cadrage fonctionnel\
**Statut :** Base de discussion avant conception technique\
**Moteur cible :** MCF Engine, basé sur Bevy 0.20.1 / Rust\
**Scripting de modding envisagé :** Lua\
**Référence d'expérience :** Eden Editor (Arma 3), étendu à un véritable
éditeur de monde

------------------------------------------------------------------------

## 1. Vision et objectifs

MCF Engine doit intégrer un éditeur de scènes et de cartes permettant de
construire des mondes jouables complets, avec une expérience inspirée
d'Eden Editor d'Arma 3 : placer des objets dans le monde, les
sélectionner, les déplacer, les configurer, tester immédiatement le
résultat et sauvegarder la carte.

L'objectif n'est pas de reproduire Eden à l'identique. Il faut en
reprendre la simplicité de prise en main et la logique de travail, tout
en allant plus loin sur :

-   l'édition du terrain et des reliefs ;
-   la peinture des surfaces et la distribution de matériaux ;
-   le dessin de routes, chemins, rivières, ruisseaux et autres tracés ;
-   l'édition dans une vue 2D cartographique et dans une vue 3D ;
-   la configuration des propriétés physiques et gameplay des objets ;
-   les scripts associés aux objets et aux éléments du monde ;
-   la prévisualisation de la météo, de l'heure et de l'éclairage ;
-   la gestion de l'eau et des environnements maritimes ;
-   l'import/export de cartes et de contenus de modding ;
-   une version joueur/moddeur volontairement limitée par rapport à
    l'éditeur interne du studio.

L'éditeur doit être un outil de production quotidien pour l'équipe MCF
Studio, mais aussi une fonctionnalité de création communautaire. Son
architecture doit donc être pensée dès le départ pour distinguer les
fonctions internes, les fonctions accessibles aux moddeurs et les
données réellement distribuables.

### 1.1 Principes directeurs

1.  **Le monde est une donnée éditable, pas une scène monolithique.**
    Terrain, objets, routes, eau, zones, scripts et paramètres
    environnementaux doivent être stockés dans des ressources
    structurées.
2.  **2D et 3D éditent le même monde.** Une modification dans une vue
    doit être immédiatement cohérente dans l'autre.
3.  **Le résultat doit être visible immédiatement.** Les changements de
    terrain, matériaux, météo, heure et objets doivent pouvoir être
    prévisualisés sans lancer une session de jeu complète.
4.  **L'éditeur ne doit pas être le runtime.** Le runtime doit pouvoir
    charger une carte exportée sans embarquer les outils de création.
5.  **Rust gère le cœur et les systèmes coûteux.** Lua sert à étendre le
    gameplay et le comportement des contenus, dans un environnement
    contrôlé.
6.  **La création doit rester accessible.** Les actions courantes
    doivent être réalisables à la souris, avec des outils visuels et des
    paramètres compréhensibles.
7.  **Les grandes cartes sont un cas d'usage de première classe.** Le
    format, le streaming et les outils doivent éviter de charger et
    recalculer toute la carte à chaque opération.

------------------------------------------------------------------------

## 2. Périmètre fonctionnel

### 2.1 Fonctions principales

-   Gestion de projets et de cartes.
-   Vue 2D de dessus avec coordonnées, grille, zoom et navigation.
-   Vue 3D libre avec caméra éditeur.
-   Mode de vue partagé 2D/3D.
-   Placement, sélection et transformation d'objets.
-   Hiérarchie et organisation des objets.
-   Inspecteur de propriétés contextuel.
-   Édition du terrain : hauteur, pente, lissage, nivellement et relief.
-   Peinture des sols et répartition de matériaux.
-   Dessin de routes, chemins, pistes, sentiers et cours d'eau.
-   Placement et édition de points de repère / marqueurs.
-   Configuration de l'eau, des côtes, des rivières et des surfaces
    maritimes.
-   Gestion de l'heure, de la météo, du ciel et de l'éclairage.
-   Propriétés physiques des objets : gravité, collision, masse,
    mobilité, destructibilité.
-   Association de scripts et d'événements.
-   Prévisualisation et test local.
-   Sauvegarde, versions, import et export.
-   Contrôle des fonctionnalités disponibles selon le profil
    utilisateur.

### 2.2 Fonctions à prévoir dans l'architecture, même si elles arrivent plus tard

-   Édition multi-utilisateur ou collaboration.
-   Outils de génération procédurale.
-   Système de préfabs / compositions réutilisables.
-   Édition des zones de gameplay et volumes de déclenchement.
-   Outils de navigation IA et zones de déplacement.
-   Édition des points de spawn et chemins de patrouille.
-   Outils de végétation et de répartition d'objets.
-   Outils de découpage et d'optimisation de carte.
-   Outils de validation automatique avant publication.
-   Historique avancé et comparaison de versions.
-   Extensions d'éditeur sous forme de plugins.

### 2.3 Hors périmètre initial

Ces fonctions ne doivent pas bloquer la première version :

-   Modélisation 3D complète de meshes.
-   Sculpture avancée de modèles comme dans Blender.
-   Création complète de matériaux procéduraux nodaux.
-   Éditeur audio complet.
-   Éditeur d'animation complet.
-   Éditeur de code Lua intégré de niveau IDE.
-   Collaboration temps réel.
-   Génération procédurale complète de villes.
-   Édition simultanée d'une même carte par plusieurs utilisateurs.

L'éditeur pourra ouvrir des outils externes spécialisés et importer
leurs résultats.

------------------------------------------------------------------------

## 3. Organisation générale de l'interface

### 3.1 Disposition cible

L'interface doit être dockable et configurable. Les panneaux peuvent
être déplacés, redimensionnés, masqués et réorganisés.

Disposition de référence :

-   **Barre supérieure :** projet, carte, sauvegarde, annuler/rétablir,
    modes d'édition, test, import/export.
-   **Barre d'outils gauche :** sélection, déplacement, rotation,
    échelle, terrain, peinture, tracés, marqueurs, placement.
-   **Zone centrale :** viewport 2D ou 3D, ou deux viewports côte à
    côte.
-   **Panneau droit :** hiérarchie, inspecteur et propriétés.
-   **Panneau inférieur :** console, erreurs, scripts, progression des
    imports, tâches de génération et diagnostics.
-   **Barre d'état :** coordonnées, secteur, altitude, outil actif, état
    de sauvegarde, mode de rendu.

Les panneaux doivent pouvoir être détachés dans une fenêtre séparée si
la plateforme le permet.

### 3.2 Modes de viewport

L'utilisateur doit pouvoir basculer entre :

1.  **Vue 2D carte** : caméra verticale orthographique, destinée à la
    composition et aux tracés.
2.  **Vue 3D** : caméra libre, avec navigation de type éditeur 3D.
3.  **Vue partagée** : 2D et 3D visibles simultanément.
4.  **Vue caméra de jeu** : prévisualisation depuis une caméra ou un
    point de vue sélectionné.
5.  **Vue de diagnostic** : affichage des secteurs, collisions, LOD,
    matériaux, navigation, limites et données techniques.

Le passage de 2D à 3D ne doit pas changer l'objet sélectionné ni perdre
l'outil actif.

### 3.3 Navigation 2D

-   Déplacement par glisser ou raccourci clavier.
-   Zoom centré sur le curseur.
-   Affichage d'une grille facultative.
-   Coordonnées du pointeur et de la sélection.
-   Orientation nord configurable.
-   Échelle et règle cartographique.
-   Affichage optionnel des noms de secteurs.
-   Affichage optionnel des marqueurs, routes, cours d'eau, objets,
    zones et limites.
-   Recherche d'un objet ou d'une coordonnée.
-   Centrage sur la sélection.
-   Sélection rectangulaire et sélection multiple.

La vue 2D doit rester lisible sur une carte très étendue. Des niveaux de
détail cartographiques peuvent simplifier l'affichage lorsque le zoom
est éloigné.

### 3.4 Navigation 3D

-   Orbite autour du point ciblé.
-   Déplacement latéral et vertical.
-   Zoom avant/arrière.
-   Mode caméra libre.
-   Mode caméra centrée sur la sélection.
-   Vitesse de déplacement réglable.
-   Accélération temporaire.
-   Alignement sur les axes.
-   Vues prédéfinies : dessus, dessous, nord, sud, est, ouest.
-   Possibilité de verrouiller la caméra sur un objet.
-   Contrôle de la visibilité des gizmos et overlays.

### 3.5 Sélection et transformations

Les outils fondamentaux doivent être disponibles dans les deux vues :

-   Sélection simple et multiple.
-   Déplacement sur un ou plusieurs axes.
-   Rotation.
-   Mise à l'échelle.
-   Duplication.
-   Suppression.
-   Alignement sur la grille.
-   Aimantation au terrain.
-   Aimantation aux objets ou surfaces.
-   Copie/collage.
-   Duplication en série.
-   Distribution le long d'un tracé.
-   Alignement sur une pente ou une surface.
-   Verrouillage temporaire d'axes.
-   Transformation numérique précise dans l'inspecteur.

Les outils doivent afficher un retour visuel clair et permettre
d'annuler chaque opération.

------------------------------------------------------------------------

## 4. Gestion des scènes et hiérarchie du monde

### 4.1 Hiérarchie

Le monde doit être organisé dans une hiérarchie lisible. Exemple de
structure logique :

-   Carte
    -   Environnement
        -   Terrain
        -   Eau
        -   Ciel et météo
    -   Secteur Nord
        -   Bâtiments
        -   Décorations
        -   Routes
        -   Zones
    -   Secteur Port
        -   Bâtiments
        -   Objets interactifs
        -   Quai et équipements
    -   Gameplay
        -   Points de spawn
        -   Marqueurs
        -   Zones de déclenchement
        -   Chemins IA
    -   Scripts et événements

Cette hiérarchie est une représentation de travail. Elle ne doit pas
obliger le moteur à créer une entité ECS parent pour chaque dossier
visuel.

### 4.2 Dossiers, groupes et couches

L'utilisateur doit pouvoir :

-   Créer des dossiers.
-   Renommer et réorganiser les dossiers.
-   Déplacer des objets entre dossiers.
-   Masquer/afficher un dossier.
-   Verrouiller un dossier contre les modifications.
-   Filtrer par type d'objet.
-   Sélectionner tous les objets d'un dossier.
-   Définir des couleurs d'organisation.
-   Créer des couches d'édition indépendantes.

Exemples de couches :

-   Terrain.
-   Routes.
-   Eau.
-   Bâtiments.
-   Végétation.
-   Mobilier.
-   Gameplay.
-   Éclairage.
-   Décoration.
-   Mod spécifique.

Les couches servent à la visibilité, au verrouillage, à l'organisation
et éventuellement à l'export sélectif.

### 4.3 Scènes et sous-scènes

Une carte peut être composée de scènes réutilisables :

-   Une scène de bâtiment.
-   Une scène de station-service.
-   Une scène de port.
-   Une scène de quartier.
-   Une scène de décoration.
-   Une scène de gameplay.

Une instance de sous-scène doit pouvoir être placée, déplacée et
configurée dans la carte. L'éditeur doit permettre de choisir entre
modifier une instance ou ouvrir la ressource source.

Il faut distinguer :

-   **Scène source :** composition réutilisable.
-   **Instance :** occurrence placée dans une carte.
-   **Surcharge :** propriété modifiée uniquement pour cette instance.
-   **Lien de ressource :** référence vers le contenu source.

------------------------------------------------------------------------

## 5. Placement et gestion des objets

### 5.1 Bibliothèque d'assets

L'Asset Browser doit proposer :

-   Recherche par nom.
-   Catégories et tags.
-   Collections et favoris.
-   Aperçu 3D avec rotation.
-   Aperçu des dimensions et du point d'origine.
-   Informations sur les collisions.
-   Informations sur les matériaux.
-   Indication des dépendances.
-   Indication de la taille mémoire estimée.
-   Filtrage par type : mesh, prefab, texture, matériau, son, script,
    ressource de terrain, etc.
-   Glisser-déposer dans le viewport.
-   Placement depuis la vue 2D ou 3D.

L'import d'un asset doit pouvoir générer ou configurer ses métadonnées :
échelle, orientation, origine, collisions, LOD et catégories.

### 5.2 Placement

Le placement doit être rapide :

-   Glisser-déposer depuis la bibliothèque.
-   Clic dans le monde pour placer.
-   Placement sur le terrain.
-   Placement sur une surface.
-   Placement à une altitude donnée.
-   Rotation avant validation.
-   Répétition du dernier objet placé.
-   Duplication rapide.
-   Placement en série.
-   Placement le long d'une route ou d'une courbe.
-   Placement aléatoire contrôlé pour la végétation et le décor.

Les objets doivent pouvoir être déplacés après placement dans la vue 2D
ou 3D.

### 5.3 Menu contextuel au double-clic

Le double-clic sur un objet ouvre une fenêtre ou un panneau de
propriétés détaillées. Un clic simple sélectionne l'objet ; le
double-clic ouvre son inspecteur ou son panneau dédié.

Le panneau doit être organisé par catégories.

#### Identité

-   Nom dans la carte.
-   Type d'objet.
-   Asset source.
-   Identifiant stable.
-   Tags.
-   Dossier et couche.
-   Description interne.
-   Visibilité dans l'éditeur.
-   Présence ou non dans l'export.

#### Transformations

-   Position X/Y/Z.
-   Rotation X/Y/Z.
-   Échelle X/Y/Z.
-   Point de pivot.
-   Verrouillage des transformations.
-   Alignement au terrain.
-   Orientation automatique selon la pente, si compatible.

#### Physique

-   Corps physique activé ou non.
-   Type : statique, dynamique ou cinématique.
-   Gravité activée/désactivée.
-   Masse.
-   Friction.
-   Restitution.
-   Détection de collision.
-   Forme de collision.
-   Collision activée en jeu.
-   Collision activée dans l'éditeur.
-   Contraintes éventuelles.

Les propriétés doivent être validées par le moteur. Par exemple, un
objet statique ne doit pas présenter des réglages dynamiques
incohérents.

#### Destructibilité

-   Objet destructible : oui/non.
-   Résistance ou points de vie.
-   Type de dégâts acceptés.
-   État visuel de remplacement.
-   Comportement à la destruction.
-   Éléments générés lors de la destruction.
-   Événement déclenché.
-   Délai ou animation de destruction.
-   Réapparition éventuelle.
-   Persistance de l'état dans la sauvegarde du monde.

La destructibilité doit être un composant de gameplay configurable, pas
une propriété implicite de tous les meshes.

#### Interaction

-   Objet interactif : oui/non.
-   Type d'interaction.
-   Distance d'interaction.
-   Texte ou icône d'interaction.
-   Conditions d'activation.
-   Événements associés.
-   Autorisations ou rôles requis, si le jeu en a besoin.

#### Scripts

-   Script Lua associé.
-   Événements auxquels le script répond.
-   Paramètres exposés au script.
-   Activation/désactivation du script.
-   Validation de la ressource.
-   Affichage des erreurs.
-   Bouton pour ouvrir le script dans l'éditeur de code.
-   Indication des permissions demandées par le script.

#### Rendu

-   Visibilité.
-   Ombres.
-   Niveau de détail.
-   Distance d'affichage.
-   Matériau ou variante de matériau.
-   Paramètres spécifiques autorisés par l'asset.
-   Options de rendu avancées réservées à l'éditeur interne si
    nécessaire.

### 5.4 Préfabriqués

Un prefab est une composition d'objets réutilisable. Il peut contenir
plusieurs meshes, collisions, scripts et points d'interaction.

L'éditeur doit permettre de :

-   Créer un prefab à partir d'une sélection.
-   Enregistrer un prefab dans une bibliothèque.
-   Placer plusieurs instances.
-   Modifier la source.
-   Mettre à jour les instances.
-   Définir des propriétés exposées et modifiables par instance.
-   Déconnecter une instance de sa source, avec avertissement.
-   Voir les dépendances du prefab.

------------------------------------------------------------------------

## 6. Éditeur de terrain

L'édition du terrain est une fonction centrale, pas un simple outil
secondaire. Elle doit être utilisable dans la vue 3D et, pour les
opérations adaptées, dans la vue 2D.

### 6.1 Représentation du terrain

Le terrain doit être découpé en secteurs ou tiles indépendants. La
taille des secteurs doit être configurable au niveau du projet ou de la
carte.

Chaque secteur doit pouvoir contenir :

-   Données d'altitude.
-   Données de matériaux/surfaces.
-   Masques de peinture.
-   Informations de collision.
-   Données de végétation ou de répartition.
-   Données de LOD.
-   Métadonnées et version.
-   Modifications locales persistantes.

Les frontières entre secteurs doivent être continues : pas de fissures
visibles, de différences brutales d'altitude ou de rupture de matériaux.

### 6.2 Outils de sculpture

Outils minimum :

-   Monter le terrain.
-   Abaisser le terrain.
-   Lisser.
-   Aplanir à une hauteur cible.
-   Créer une pente régulière.
-   Élever/abaisser une zone.
-   Pincer ou former une crête.
-   Creuser une dépression.
-   Échantillonner une hauteur.
-   Restaurer une zone depuis une sauvegarde ou un état antérieur.

Paramètres communs :

-   Taille du pinceau.
-   Intensité.
-   Douceur du bord.
-   Forme : cercle, carré, forme personnalisée.
-   Mode continu ou par clic.
-   Symétrie éventuelle.
-   Masque de protection.
-   Affichage du rayon et de la zone d'influence.

Les opérations doivent être annulables. Les modifications doivent être
enregistrées comme des changements de données de terrain, et non
uniquement comme des modifications temporaires du mesh affiché.

### 6.3 Outils de nivellement spécialisés

Pour construire une ville, des routes et des installations, il faut des
outils adaptés :

-   Aplanir une plateforme.
-   Créer une terrasse.
-   Créer une pente entre deux hauteurs.
-   Adapter le terrain à une route.
-   Créer une plateforme de bâtiment.
-   Lisser une jonction entre deux zones.
-   Générer un talus.
-   Creuser une tranchée.
-   Créer un fossé de drainage.

L'outil doit afficher un aperçu avant application lorsque l'opération
modifie une grande surface.

### 6.4 Peinture des sols et matériaux

L'utilisateur doit pouvoir peindre directement sur le terrain, avec un
résultat visible en temps réel.

Fonctions :

-   Peindre une surface.
-   Effacer une peinture.
-   Remplacer un matériau.
-   Mélanger plusieurs couches.
-   Peindre avec un pinceau.
-   Remplir une zone délimitée.
-   Peindre selon la pente.
-   Peindre selon l'altitude.
-   Peindre selon une texture ou un masque.
-   Échantillonner un matériau existant.
-   Copier/coller une configuration de pinceau.

Paramètres :

-   Taille et dureté du pinceau.
-   Opacité/intensité.
-   Débit.
-   Mode d'application.
-   Matériau cible.
-   Couche cible.
-   Mélange avec les couches existantes.
-   Rotation et échelle de texture.
-   Répétition et variation.

Le système doit prendre en charge plusieurs couches de matériaux par
secteur. Le nombre maximal de couches doit être défini selon les
contraintes de rendu et de mémoire.

### 6.5 Végétation et distribution

Même si l'outil complet arrive après la première version, le format doit
permettre :

-   Peindre des zones de végétation.
-   Distribuer des instances selon une densité.
-   Définir des espèces ou collections.
-   Exclure routes, bâtiments, eau et zones interdites.
-   Contrôler l'échelle et la rotation.
-   Ajouter une variation aléatoire déterministe.
-   Régénérer une zone sans modifier les autres secteurs.
-   Convertir une partie de la distribution en objets éditables
    individuellement.

------------------------------------------------------------------------

## 7. Routes, chemins, rivières et tracés

Le moteur doit proposer un outil de tracé basé sur des courbes
éditables. Il ne faut pas construire les routes ou rivières uniquement
en plaçant manuellement des segments de mesh.

### 7.1 Outil de courbe commun

Fonctions communes :

-   Créer un tracé par clics successifs.
-   Ajouter, déplacer et supprimer des points.
-   Modifier les tangentes.
-   Convertir un point en angle, courbe ou transition.
-   Fermer une boucle.
-   Prolonger un tracé.
-   Scinder ou fusionner deux tracés.
-   Ajuster la largeur.
-   Définir une altitude ou un décalage.
-   Aimantation au terrain.
-   Aperçu en direct.
-   Contrôle de la résolution de génération.
-   Lissage de la courbe.
-   Duplication et inversion du tracé.

Les données de courbe doivent être indépendantes du mesh généré. Le mesh
est un résultat dérivé et doit pouvoir être reconstruit.

### 7.2 Routes

L'outil route doit gérer :

-   Routes principales.
-   Routes secondaires.
-   Rues.
-   Chemins ruraux.
-   Pistes en terre.
-   Voies industrielles.
-   Routes privées.
-   Accotements.
-   Trottoirs.
-   Bordures.
-   Fossés et talus.
-   Marquages et éléments de signalisation.

Paramètres :

-   Largeur de chaussée.
-   Largeur des accotements.
-   Nombre de voies.
-   Matériau de surface.
-   Profil transversal.
-   Dévers.
-   Pente maximale conseillée.
-   Adaptation au terrain.
-   Découpe ou nivellement du terrain.
-   Raccords aux intersections.
-   Type de bordure.
-   Génération de collision.
-   Distance et niveau de détail.

L'outil doit proposer un mode d'intersection assistée : quand plusieurs
routes se croisent, l'éditeur doit aider à créer une jonction propre
plutôt que laisser des meshes se chevaucher.

### 7.3 Chemins et sentiers

Les chemins doivent utiliser une version simplifiée de l'outil route :

-   Largeur réglable.
-   Surface terre, gravier, herbe, pavés ou autre matériau.
-   Bordures facultatives.
-   Adaptation au relief.
-   Possibilité de générer des traces ou une légère déformation du
    terrain.
-   Connexion à des routes ou à des points d'intérêt.

### 7.4 Cours d'eau

Les rivières, ruisseaux et canaux doivent être édités comme des tracés
hydrologiques, pas comme de simples meshes posés sur le sol.

Fonctions attendues :

-   Tracé de la ligne centrale.
-   Largeur variable le long du parcours.
-   Profondeur variable.
-   Altitude et pente.
-   Berges et transitions.
-   Fond du lit.
-   Raccordement à un lac, à la mer ou à un autre cours d'eau.
-   Génération de terrain creusé si activée.
-   Matériau des berges.
-   Végétation de berge.
-   Débit ou vitesse visuelle de l'eau.
-   Direction du courant.
-   Zones calmes, rapides et cascades.
-   Outils de validation des écoulements et des raccords.

Il faut distinguer le tracé de rivière, sa géométrie, le lit du cours
d'eau et le système de rendu de l'eau. Cette séparation permettra de
changer le rendu sans perdre les données d'édition.

### 7.5 Autres tracés

Le même système de courbes pourra servir à :

-   Clôtures.
-   Murs.
-   Haies.
-   Câbles.
-   Rails.
-   Tuyaux.
-   Conduites.
-   Limites de zones.
-   Chemins de patrouille.
-   Réseaux de navigation.
-   Lignes électriques.
-   Quais et digues.

Chaque type de tracé définit son propre profil, ses paramètres et ses
règles de génération.

------------------------------------------------------------------------

## 8. Système d'eau --- exigence prioritaire

L'eau est un élément majeur du jeu en raison de la place de
l'environnement maritime. Elle doit être pensée dès la conception du
moteur, et non ajoutée tardivement comme un matériau transparent
standard.

Le rendu doit être beau dès la première version jouable. Il faut
néanmoins séparer le rendu visuel, les données physiques et les outils
d'édition afin de pouvoir faire évoluer les techniques.

### 8.1 Types d'eau

Le système doit prévoir au minimum :

-   Mer et océan.
-   Baies et ports.
-   Lacs.
-   Rivières.
-   Ruisseaux.
-   Canaux.
-   Eau peu profonde.
-   Eau profonde.
-   Zones de transition entre eau et terre.

Les types d'eau peuvent partager une base technique, mais doivent
permettre des paramètres visuels et physiques différents.

### 8.2 Qualité visuelle attendue

L'eau doit proposer :

-   Reflets du ciel et de l'environnement.
-   Réflexions adaptées à la qualité graphique.
-   Transparence contrôlée.
-   Profondeur visuelle et absorption de la lumière.
-   Couleur variant selon la profondeur et le type d'eau.
-   Vagues animées.
-   Normales animées à plusieurs échelles.
-   Micro-ondulations.
-   Écume sur les côtes et autour des obstacles.
-   Écume liée aux vagues et aux zones agitées.
-   Distorsion de la lumière sous la surface.
-   Effet de profondeur sous-marine.
-   Brume ou absorption sous l'eau.
-   Transition crédible entre eau et rivage.
-   Effets de pluie à la surface.
-   Réaction visuelle au vent.
-   Variations de couleur et d'agitation selon les conditions météo.

La priorité visuelle doit être donnée à la mer et aux zones portuaires :
ce sont des environnements où l'eau occupe une grande partie de l'image.

### 8.3 Paramètres d'édition

Dans l'inspecteur de l'eau :

-   Niveau de la surface.
-   Couleur de base.
-   Couleur en profondeur.
-   Transparence.
-   Absorption.
-   Intensité et taille des vagues.
-   Direction dominante des vagues.
-   Vitesse d'animation.
-   Force du vent.
-   Rugosité et intensité des reflets.
-   Écume.
-   Profondeur de transition.
-   Paramètres de sous-marin.
-   Profil de qualité.

L'utilisateur doit pouvoir créer des profils : mer calme, mer agitée,
port abrité, rivière lente, rivière rapide, eau peu profonde, etc.

### 8.4 Littoral et raccord terre/eau

L'éditeur doit aider à produire un rivage propre :

-   Détection ou définition de la ligne de côte.
-   Outil de peinture de plage ou de roche.
-   Transition humide/sèche.
-   Écume de rivage.
-   Ajustement de la profondeur près de la côte.
-   Gestion des rochers et obstacles.
-   Contrôle de la pente sous-marine.
-   Outil de correction des intersections entre terrain et eau.

Les transitions ne doivent pas dépendre uniquement d'un mesh de bordure
placé manuellement.

### 8.5 Eau et gameplay

Le système doit pouvoir exposer des données au gameplay :

-   Profondeur.
-   Hauteur de surface.
-   Courant.
-   Température éventuelle.
-   Type d'eau.
-   Zone navigable.
-   Zone peu profonde.
-   Zone interdite ou dangereuse.
-   Surface utilisable pour la navigation.
-   État de mer.

Le système de navigation maritime doit pouvoir exploiter ces données
sans analyser le shader.

### 8.6 Performance de l'eau

Prévoir :

-   Découpage spatial de la surface d'eau.
-   Niveaux de détail selon la distance.
-   Qualité variable des réflexions.
-   Limitation des effets coûteux hors champ.
-   Simulation simplifiée au loin.
-   Possibilité de désactiver certains effets par profil graphique.
-   Paramètres distincts pour l'eau proche et l'eau lointaine.
-   Outils de visualisation du coût GPU.

L'eau doit rester cohérente lorsque la caméra passe de la vue aérienne à
la surface, puis sous l'eau.

### 8.7 Critères de validation

La première version du système d'eau doit être testée dans au moins
quatre scènes de référence :

1.  Une baie avec côte visible et eau peu profonde.
2.  Un port avec quais, bateaux et obstacles.
3.  Une zone de pleine mer avec horizon et vagues.
4.  Une rivière ou un canal raccordé au terrain.

La validation ne doit pas se limiter à une capture d'écran : il faut
tester les mouvements de caméra, les variations météo, les changements
de lumière et les transitions sous-marines.

------------------------------------------------------------------------

## 9. Environnement, météo et heure

L'éditeur doit permettre de prévisualiser l'apparence de la carte selon
différents moments et conditions.

### 9.1 Heure

-   Heure locale du monde.
-   Jour, mois et saison.
-   Contrôle accéléré du temps.
-   Arrêt du temps.
-   Préréglages : matin, midi, après-midi, coucher du soleil, nuit.
-   Prévisualisation de la position du soleil.
-   Contrôle de la durée du jour.
-   Date de référence pour les variations saisonnières.

### 9.2 Éclairage et ciel

-   Direction et intensité du soleil.
-   Couleur et température de la lumière.
-   Intensité ambiante.
-   Ciel et nuages.
-   Brume atmosphérique.
-   Brouillard local.
-   Étoiles et lune.
-   Paramètres de nuit.
-   Exposition et adaptation.
-   Paramètres de qualité graphique.

### 9.3 Météo

Préréglages :

-   Ciel dégagé.
-   Peu nuageux.
-   Couvert.
-   Brume.
-   Brouillard.
-   Pluie légère.
-   Pluie forte.
-   Orage.
-   Vent fort.
-   Conditions maritimes agitées.

Paramètres continus :

-   Couverture nuageuse.
-   Densité des nuages.
-   Intensité de pluie.
-   Intensité du vent.
-   Direction du vent.
-   Brouillard.
-   Humidité visuelle.
-   Intensité des éclairs.
-   Agitation de la mer.
-   Visibilité.

L'éditeur doit permettre de figer un état météo ou de lancer une
transition temporelle pour tester les changements.

### 9.4 Prévisualisation

-   Changement immédiat de l'heure.
-   Changement immédiat de météo.
-   Prévisualisation d'une transition.
-   Sauvegarde de préréglages.
-   Comparaison de deux conditions.
-   Capture d'écran de référence.
-   Contrôle de la météo depuis une caméra de jeu.

Il faut pouvoir vérifier rapidement qu'un port reste lisible de nuit,
qu'une route est visible sous la pluie et que la mer reste crédible par
temps couvert.

------------------------------------------------------------------------

## 10. Marqueurs, zones et outils de gameplay

### 10.1 Marqueurs

L'utilisateur doit pouvoir poser des marqueurs en 2D et en 3D.

Types possibles :

-   Point d'intérêt.
-   Spawn joueur.
-   Spawn véhicule.
-   Point de livraison.
-   Point d'interaction.
-   Zone de mission.
-   Entrée/sortie.
-   Repère de navigation.
-   Point de patrouille.
-   Point de collecte.
-   Marqueur technique.

Propriétés :

-   Nom.
-   Icône.
-   Couleur.
-   Taille.
-   Orientation.
-   Description.
-   Catégorie.
-   Visibilité dans l'éditeur.
-   Visibilité en jeu.
-   Rayon.
-   Script ou événement associé.

### 10.2 Zones

Outils de création :

-   Cercle.
-   Rectangle.
-   Polygone.
-   Volume 3D.
-   Zone basée sur une courbe.
-   Zone basée sur une surface.

Usages :

-   Zone de spawn.
-   Zone de construction.
-   Zone de mission.
-   Zone interdite.
-   Zone de dégâts.
-   Zone de son.
-   Zone de météo locale.
-   Zone de streaming prioritaire.
-   Zone de navigation maritime.
-   Zone de protection.
-   Zone de déclenchement.

Les zones doivent être visibles et éditables dans les deux vues.

### 10.3 Points et chemins IA

Prévoir des outils dédiés pour :

-   Points de passage.
-   Chemins de patrouille.
-   Routes de véhicules.
-   Zones de déplacement.
-   Points d'attente.
-   Zones de navigation.
-   Liens entre points.

Ces données doivent être exportables et utilisables par les systèmes de
gameplay sans dépendre de l'éditeur.

------------------------------------------------------------------------

## 11. Lua et système de scripts

Lua est envisagé comme langage de modding et comme outil d'extension de
certains comportements. Le cœur du moteur reste en Rust.

### 11.1 Répartition Rust / Lua

**Rust doit gérer :**

-   ECS et cycle de vie des entités.
-   Rendu et ressources GPU.
-   Physique et collisions.
-   Streaming des secteurs.
-   Terrain et génération des meshes.
-   Système d'eau.
-   Audio.
-   Réseau.
-   Sérialisation et chargement.
-   API natives de gameplay coûteuses.
-   Sécurité et validation des données.
-   Scheduling des tâches.

**Lua peut gérer :**

-   Comportements d'objets.
-   Interactions.
-   Événements de mission.
-   Logique de zones.
-   Déclencheurs.
-   Petits systèmes de gameplay.
-   Configuration de contenus.
-   Quêtes et objectifs.
-   Scripts de mod.
-   Extensions de comportements exposés par le moteur.

Lua ne doit pas être utilisé pour recalculer chaque frame des opérations
lourdes de terrain, de rendu ou de physique.

### 11.2 API de scripting

Le moteur doit exposer une API stable et documentée. Exemples
conceptuels :

-   Obtenir une entité.
-   Lire ou modifier une propriété autorisée.
-   Écouter un événement.
-   Déclencher un événement.
-   Lire la position.
-   Déplacer un objet via les commandes autorisées.
-   Activer/désactiver un objet.
-   Lire les paramètres d'une zone.
-   Lancer une interaction.
-   Programmer un événement différé.
-   Écrire dans les logs.
-   Accéder aux données de gameplay autorisées.

L'API réelle sera définie en Rust et versionnée. Les exemples ci-dessus
sont des besoins fonctionnels, pas une proposition d'API définitive.

### 11.3 Événements de cycle de vie

Un objet peut exposer des événements comme :

-   Initialisation.
-   Chargement dans le monde.
-   Activation.
-   Interaction.
-   Entrée dans une zone.
-   Sortie d'une zone.
-   Réception d'un événement gameplay.
-   Destruction.
-   Déchargement du secteur.

Les événements doivent être documentés et prévisibles. Le moteur doit
éviter d'appeler un script Lua pour chaque objet à chaque frame si aucun
événement ne le nécessite.

### 11.4 Paramètres exposés

Un script peut déclarer des paramètres configurables dans l'inspecteur :

-   Nombre.
-   Texte.
-   Booléen.
-   Liste de choix.
-   Référence d'objet.
-   Référence de zone.
-   Couleur.
-   Vecteur.
-   Ressource.

L'éditeur génère les champs correspondants. Les valeurs sont
sauvegardées dans la carte, pas codées en dur dans le script.

### 11.5 Sécurité du modding

Les scripts de joueurs doivent être considérés comme non fiables.

Prévoir :

-   Permissions explicites.
-   API limitée.
-   Pas d'accès arbitraire au système de fichiers.
-   Pas d'exécution de commandes système.
-   Limites de temps d'exécution.
-   Limites mémoire.
-   Possibilité d'interrompre un script.
-   Gestion des erreurs sans faire tomber le jeu.
-   Journaux par script.
-   Version d'API déclarée.
-   Dépendances déclarées.
-   Signature ou provenance des packs selon le système de distribution.
-   Validation avant chargement en multijoueur.

Il faudra décider si les scripts sont exécutés côté serveur, côté client
ou des deux côtés. Les décisions ayant un impact sur l'état partagé
doivent être autorisées et validées par le serveur.

### 11.6 Éditeur de scripts

Pour la première version :

-   Association d'un fichier Lua.
-   Ouverture dans un éditeur externe.
-   Affichage des erreurs et logs.
-   Vérification de syntaxe.
-   Liste des événements disponibles.
-   Documentation de l'API.

À terme :

-   Éditeur intégré.
-   Coloration syntaxique.
-   Autocomplétion.
-   Documentation contextuelle.
-   Recherche de références.
-   Débogueur.
-   Points d'arrêt.
-   Inspection des variables autorisées.
-   Rechargement à chaud lorsque cela est sûr.

------------------------------------------------------------------------

## 12. Import, export et format de carte

L'import/export est une exigence fondamentale, car les joueurs pourront
créer et partager des cartes.

### 12.1 Format de projet

Un projet d'éditeur contient :

-   Configuration du projet.
-   Une ou plusieurs cartes.
-   Scènes et sous-scènes.
-   Références aux assets.
-   Scripts.
-   Paramètres d'environnement.
-   Données de terrain.
-   Tracés.
-   Zones et marqueurs.
-   Métadonnées.
-   Dépendances.
-   Version du format.

Les données doivent être lisibles et versionnées. Un format texte comme
RON ou JSON peut être utilisé pour les métadonnées et les structures
facilement diffables. Les données volumineuses de terrain, masques et
caches peuvent utiliser des fichiers binaires dédiés.

### 12.2 Format exporté

L'export d'une carte doit produire un paquet autonome contenant
uniquement :

-   Les données nécessaires à la carte.
-   Les assets référencés et autorisés.
-   Les scènes utilisées.
-   Les scripts autorisés.
-   Les configurations météo et environnementales.
-   Les données de terrain.
-   Les dépendances requises.
-   Un manifeste de paquet.

L'export ne doit pas inclure les outils internes de l'éditeur ni les
ressources non utilisées.

### 12.3 Manifeste

Le manifeste doit contenir au minimum :

-   Identifiant unique de carte.
-   Nom.
-   Auteur ou équipe.
-   Version de carte.
-   Version minimale du moteur.
-   Version du format de données.
-   Description.
-   Image de présentation facultative.
-   Liste des dépendances.
-   Liste des scripts.
-   Permissions demandées.
-   Taille du paquet.
-   Empreintes de fichiers.
-   Licence et conditions de distribution.
-   Paramètres de chargement.

### 12.4 Import

À l'import, le moteur doit :

1.  Lire le manifeste.
2.  Vérifier la version du format.
3.  Vérifier les dépendances.
4.  Vérifier les permissions des scripts.
5.  Vérifier les fichiers et leurs empreintes.
6.  Détecter les conflits d'identifiants.
7.  Signaler les ressources manquantes.
8.  Proposer une migration si le format est ancien.
9.  Présenter un rapport avant validation.
10. Importer dans un espace isolé ou un nouveau projet avant
    intégration.

Un import ne doit jamais écraser silencieusement des fichiers existants.

### 12.5 Identifiants stables

Les objets, scènes, zones, tracés et ressources doivent posséder des
identifiants stables. Ils ne doivent pas dépendre uniquement de leur nom
ou de leur ordre dans un fichier.

Ces identifiants servent à :

-   Retrouver les références.
-   Appliquer les mises à jour.
-   Gérer les dépendances.
-   Comparer les versions.
-   Fusionner des modifications.
-   Identifier les objets dans les scripts.

### 12.6 Export sélectif

L'utilisateur doit pouvoir exporter :

-   La carte complète.
-   Un secteur.
-   Une sélection.
-   Une scène ou composition.
-   Un prefab.
-   Un pack de ressources.
-   Un ensemble de scripts et de données associés.

L'export doit détecter les dépendances nécessaires et expliquer ce qui
sera ajouté au paquet.

------------------------------------------------------------------------

## 13. Éditeur interne et éditeur communautaire

Le même socle technique doit alimenter deux profils d'éditeur.

### 13.1 Éditeur interne MCF Studio

Accès complet :

-   Tous les outils de terrain.
-   Tous les outils de tracés.
-   Tous les paramètres de rendu.
-   Gestion complète de l'eau.
-   Outils de diagnostic.
-   Profiler.
-   Gestion des LOD.
-   Gestion du streaming.
-   Édition des collisions.
-   Outils de validation.
-   Gestion des ressources internes.
-   Outils de débogage Lua.
-   Accès aux propriétés avancées.
-   Outils de conversion et migration.
-   Gestion des cartes de production.

### 13.2 Éditeur joueur / moddeur

Accès contrôlé :

-   Placement d'objets issus des packs autorisés.
-   Création et organisation de scènes.
-   Édition du terrain dans les limites autorisées.
-   Peinture avec les matériaux autorisés.
-   Création de routes et chemins selon les outils exposés.
-   Création de rivières ou d'étendues d'eau selon les limites définies.
-   Placement de marqueurs et zones.
-   Réglage de l'heure et de la météo si autorisé.
-   Configuration des propriétés autorisées.
-   Scripts Lua dans une sandbox.
-   Import/export de cartes.
-   Utilisation des prefabs autorisés.

Restrictions possibles :

-   Pas d'accès aux assets internes.
-   Pas de modification des systèmes de rendu internes.
-   Pas d'accès aux paramètres réseau sensibles.
-   Pas d'accès aux outils de profiling internes.
-   Pas de modification des limites de sécurité.
-   Pas de script système.
-   Pas de modification des contenus protégés.
-   Limites de taille de carte, nombre d'objets, scripts ou ressources
    selon le contexte de distribution.

Les restrictions doivent être appliquées par les capacités du moteur et
les permissions des ressources, pas uniquement en masquant des boutons
dans l'interface.

### 13.3 Capacités et permissions

Chaque outil et chaque propriété doit déclarer une capacité requise. Le
profil utilisateur détermine les capacités disponibles.

Exemples :

-   `world.terrain.edit`
-   `world.material.paint`
-   `world.water.configure`
-   `world.object.place`
-   `world.object.physics`
-   `world.object.script`
-   `world.environment.configure`
-   `world.export`
-   `engine.debug`
-   `engine.profile`

Ces noms sont des identifiants de conception, pas nécessairement les
noms définitifs.

------------------------------------------------------------------------

## 14. Grandes cartes et performances

L'éditeur doit être capable de travailler sur des cartes plus grandes
que ce qui est nécessairement chargé en mémoire à un instant donné.

### 14.1 Découpage spatial

Le monde doit être divisé en secteurs. Un secteur contient ses propres
données et ressources dérivées.

Chaque secteur peut être :

-   Non chargé.
-   En cours de chargement.
-   Chargé en données.
-   En cours de génération.
-   Prêt à être affiché.
-   Actif.
-   En cours de déchargement.

L'éditeur peut afficher les limites des secteurs et leur état.

### 14.2 Streaming

Le système doit permettre :

-   Chargement autour de la caméra ou du joueur.
-   Préchargement dans la direction du déplacement.
-   Déchargement des secteurs éloignés.
-   Priorité aux secteurs proches.
-   Plusieurs points de référence de streaming.
-   Chargement asynchrone.
-   Annulation des tâches devenues inutiles.
-   Cache disque et mémoire.
-   Limite de mémoire configurable.

En édition, la caméra et les zones sélectionnées doivent pouvoir forcer
le chargement d'un secteur.

### 14.3 LOD

Les objets, terrains et surfaces d'eau doivent pouvoir disposer de
plusieurs niveaux de détail.

-   LOD selon la distance.
-   Hystérésis pour éviter les changements incessants.
-   Transition visuelle si possible.
-   Imposteurs ou représentations simplifiées pour certains objets.
-   LOD spécifique à l'éditeur.
-   Visualisation des niveaux actifs.

### 14.4 Spatial index

Le moteur doit utiliser une structure spatiale pour retrouver
efficacement les objets proches et les éléments concernés par une
opération.

Cas d'usage :

-   Sélection dans une zone.
-   Recherche de collisions proches.
-   Détection des objets à afficher.
-   Calcul des objets concernés par un pinceau.
-   Détection des intersections de tracés.
-   Streaming.
-   Culling.

L'éditeur ne doit pas parcourir toutes les entités du monde pour chaque
opération de sélection ou de rendu.

### 14.5 Tâches asynchrones

Les opérations coûteuses doivent être réalisées hors du thread principal
lorsque c'est possible :

-   Génération de terrain.
-   Construction de meshes.
-   Calcul des collisions.
-   Import d'assets.
-   Génération de routes.
-   Génération de rivières.
-   Calcul de LOD.
-   Préparation des textures.
-   Export de carte.

Les résultats doivent être appliqués de façon sûre au monde ECS. Une
tâche en cours doit pouvoir être annulée ou remplacée si ses paramètres
changent.

### 14.6 Outils de diagnostic

L'éditeur interne doit afficher :

-   FPS de l'éditeur.
-   Temps CPU et GPU.
-   Mémoire utilisée.
-   Nombre d'entités visibles.
-   Nombre de draw calls.
-   Nombre de meshes et matériaux.
-   Secteurs chargés.
-   Taille des caches.
-   Temps de génération.
-   Temps de chargement.
-   LOD actif.
-   Coût du système d'eau.
-   Coût des ombres et réflexions.

Un mode debug doit permettre de visualiser les secteurs, bounds,
collisions, LOD, volumes de streaming et index spatiaux.

------------------------------------------------------------------------

## 15. Sauvegarde, annulation et fiabilité

### 15.1 Sauvegarde

-   Sauvegarde manuelle.
-   Autosave configurable.
-   Sauvegarde incrémentale.
-   Sauvegarde par secteur.
-   Sauvegarde des paramètres de projet.
-   Détection des modifications non sauvegardées.
-   Récupération après fermeture anormale.
-   Vérification de cohérence à l'ouverture.

### 15.2 Undo / Redo

Toutes les actions destructrices ou structurantes doivent être
annulables :

-   Placement et suppression.
-   Transformation.
-   Changement de propriété.
-   Modification de terrain.
-   Peinture.
-   Modification de tracé.
-   Création/suppression de zone.
-   Modification de météo.
-   Organisation de la hiérarchie.

Les grosses opérations doivent utiliser des commandes compactes ou des
deltas, plutôt que copier toute la carte en mémoire.

### 15.3 Validation

Avant export, l'éditeur doit vérifier :

-   Assets manquants.
-   Scripts invalides.
-   Références cassées.
-   Identifiants dupliqués.
-   Tracés incomplets.
-   Routes déconnectées si cela pose problème.
-   Rivières avec raccords incohérents.
-   Secteurs corrompus ou absents.
-   Ressources non autorisées.
-   Dépendances manquantes.
-   Paramètres incompatibles avec la version cible.

Le rapport doit classer les problèmes en erreurs, avertissements et
informations.

------------------------------------------------------------------------

## 16. Architecture logicielle envisagée

Cette section donne une direction, pas une obligation de découpage
définitive.

### 16.1 Séparation Editor / Runtime

Le runtime ne doit pas dépendre des panneaux UI ni des outils de
l'éditeur.

Structure indicative :

``` text
mcf/
├── crates/
│   ├── mcf_core/
│   ├── mcf_ecs/
│   ├── mcf_render/
│   ├── mcf_assets/
│   ├── mcf_physics/
│   ├── mcf_world/
│   ├── mcf_terrain/
│   ├── mcf_water/
│   ├── mcf_scripting/
│   ├── mcf_serialization/
│   ├── mcf_runtime/
│   ├── mcf_editor_core/
│   ├── mcf_editor_ui/
│   ├── mcf_scene_editor/
│   ├── mcf_world_editor/
│   ├── mcf_terrain_editor/
│   ├── mcf_spline_editor/
│   ├── mcf_asset_browser/
│   ├── mcf_profiler/
│   └── mcf_modding/
├── apps/
│   ├── mcf_editor/
│   └── mcf_game/
└── projects/
```

Les noms et le nombre de crates seront ajustés après étude des
dépendances Bevy et des responsabilités réelles.

### 16.2 Données source et données dérivées

Il faut distinguer :

**Données source :** - Points de courbes. - Hauteurs du terrain. -
Peintures et masques. - Propriétés des objets. - Paramètres météo. -
Configuration de l'eau. - Scripts et paramètres exposés.

**Données dérivées :** - Meshes générés. - Collisions générées. - LOD. -
Textures compilées. - Caches de rendu. - Index spatiaux
reconstruisibles.

Les données dérivées peuvent être supprimées et reconstruites. Elles ne
doivent pas être la seule représentation d'une information importante.

### 16.3 Commandes d'édition

Les outils ne doivent pas modifier arbitrairement les données depuis
l'interface. Ils doivent produire des commandes d'édition structurées.

Exemples conceptuels :

-   `PlaceObject`
-   `TransformObject`
-   `DeleteObject`
-   `PaintTerrain`
-   `SculptTerrain`
-   `CreateSpline`
-   `UpdateSplinePoint`
-   `SetWaterProfile`
-   `SetEnvironment`
-   `AssignScript`

Ces commandes facilitent Undo/Redo, sauvegarde, journalisation,
validation et éventuelle collaboration.

### 16.4 Compatibilité des versions

Le format de carte, les scripts et les ressources doivent déclarer leur
version. Le moteur doit prévoir des migrations explicites plutôt que
supposer que les anciennes cartes restent compatibles automatiquement.

------------------------------------------------------------------------

## 17. Parcours utilisateur principaux

### 17.1 Créer une nouvelle carte

1.  Créer un projet ou ouvrir un projet existant.
2.  Choisir les dimensions ou l'étendue du monde.
3.  Choisir le système de secteurs.
4.  Définir une hauteur et un niveau d'eau initiaux.
5.  Choisir un environnement de départ.
6.  Créer la carte.
7.  Ouvrir la vue 2D et/ou 3D.

### 17.2 Construire un quartier

1.  Passer en vue 2D pour tracer les axes routiers.
2.  Créer les routes et chemins.
3.  Passer en 3D pour vérifier les pentes et intersections.
4.  Ajuster le terrain.
5.  Peindre les surfaces.
6.  Placer les bâtiments et objets.
7.  Configurer les collisions et propriétés.
8.  Ajouter les marqueurs et zones de gameplay.
9.  Tester sous différentes conditions météo et horaires.

### 17.3 Construire un port

1.  Définir la ligne de côte.
2.  Ajuster le terrain et le fond marin.
3.  Définir la surface d'eau et le profil maritime.
4.  Créer les quais, digues et accès.
5.  Tracer les routes et voies de circulation.
6.  Placer les bâtiments, équipements et objets.
7.  Ajouter les zones de navigation et profondeurs.
8.  Tester la mer calme, le vent et les vagues.
9.  Vérifier le rendu depuis le quai, un bateau et une vue aérienne.

### 17.4 Configurer un objet interactif

1.  Placer l'objet.
2.  Double-cliquer pour ouvrir ses propriétés.
3.  Configurer son identité.
4.  Définir ses collisions et sa physique.
5.  Activer ou non sa destructibilité.
6.  Définir son interaction.
7.  Associer un script Lua.
8.  Définir les paramètres exposés.
9.  Tester l'objet.
10. Corriger les erreurs affichées dans la console.

### 17.5 Partager une carte

1.  Lancer la validation.
2.  Corriger les erreurs.
3.  Vérifier les dépendances.
4.  Définir les informations du paquet.
5.  Choisir les éléments à exporter.
6.  Générer le paquet.
7.  Tester son import dans un projet vierge.
8.  Distribuer le paquet.

------------------------------------------------------------------------

## 18. Découpage de développement proposé

L'objectif est de construire un socle solide sans tenter de livrer tous
les outils simultanément.

### Phase 0 --- Fondations

-   Définir le format de projet.
-   Définir la séparation runtime/éditeur.
-   Définir les identifiants stables.
-   Définir les commandes d'édition.
-   Définir le système de sauvegarde.
-   Définir le système de ressources.
-   Définir le découpage spatial du monde.

**Livrable :** un projet minimal qui charge, affiche et sauvegarde un
monde.

### Phase 1 --- Éditeur de scènes de base

-   Viewport 3D.
-   Caméra éditeur.
-   Sélection.
-   Gizmos.
-   Placement d'objets.
-   Hiérarchie.
-   Inspecteur.
-   Propriétés de transformation.
-   Undo/Redo.
-   Sauvegarde.

**Livrable :** un éditeur utilisable pour composer une scène 3D simple.

### Phase 2 --- Vue cartographique 2D

-   Caméra orthographique.
-   Navigation 2D.
-   Grille et coordonnées.
-   Marqueurs.
-   Sélection et placement en 2D.
-   Passage 2D/3D.
-   Vue partagée.

**Livrable :** le même monde est éditable en vue carte et en 3D.

### Phase 3 --- Terrain et matériaux

-   Terrain découpé en secteurs.
-   Sculptures de base.
-   Nivellement.
-   Lissage.
-   Peinture de matériaux.
-   Sauvegarde des données source.
-   Reconstruction asynchrone.

**Livrable :** création et modification d'un terrain de test.

### Phase 4 --- Tracés

-   Éditeur de courbes.
-   Routes.
-   Chemins.
-   Intersections simples.
-   Rivières.
-   Outils de raccord terrain.
-   Génération des meshes et collisions.

**Livrable :** création d'un quartier et d'un réseau de routes/eau sans
modéliser manuellement chaque segment.

### Phase 5 --- Eau et environnement

-   Surface maritime.
-   Vagues.
-   Reflets.
-   Transparence et profondeur.
-   Littoral.
-   Rivières et lacs.
-   Météo.
-   Heure.
-   Prévisualisation temps réel.

**Livrable :** une scène maritime de référence avec qualité visuelle
validée.

### Phase 6 --- Propriétés avancées et Lua

-   Physique des objets.
-   Destructibilité.
-   Interactions.
-   Association de scripts.
-   API Lua.
-   Sandbox.
-   Logs et validation.
-   Paramètres exposés.

**Livrable :** un objet de carte configurable et scriptable.

### Phase 7 --- Import/export et modding

-   Manifeste.
-   Paquet de carte.
-   Gestion des dépendances.
-   Validation.
-   Import isolé.
-   Profils de permissions.
-   Export sélectif.
-   Versionnage.

**Livrable :** un moddeur peut créer, exporter, importer et tester une
carte.

### Phase 8 --- Outils avancés et optimisation

-   Streaming complet.
-   LOD.
-   Spatial index avancé.
-   Végétation.
-   Outils de diagnostic.
-   Profiler.
-   Outils de génération.
-   Préfabriqués avancés.
-   Extensions d'éditeur.

**Livrable :** éditeur capable de gérer des cartes de production de
grande taille.

------------------------------------------------------------------------

## 19. Critères d'acceptation

Une fonctionnalité n'est pas considérée comme terminée uniquement parce
qu'elle fonctionne dans une scène de démonstration. Elle doit être
intégrée au format de carte, aux sauvegardes et au workflow de
l'éditeur.

### Édition

-   Le même objet peut être sélectionné en 2D et en 3D.
-   Une transformation effectuée dans une vue est immédiatement visible
    dans l'autre.
-   Les objets peuvent être organisés, masqués et verrouillés.
-   Le double-clic ouvre un inspecteur adapté au type d'objet.
-   Les opérations essentielles sont annulables.

### Terrain

-   Le terrain peut être modifié sans reconstruire toute la carte.
-   Les frontières entre secteurs restent cohérentes.
-   Les modifications sont sauvegardées dans les données source.
-   La peinture reste cohérente après fermeture et réouverture du
    projet.

### Tracés

-   Une route peut être dessinée, modifiée et reconstruite.
-   Une courbe conserve ses points de contrôle après sauvegarde.
-   Les intersections et raccords sont détectables.
-   Une rivière peut être reliée à une autre surface d'eau.
-   Les meshes générés peuvent être reconstruits depuis les courbes.

### Eau

-   La mer présente des vagues et des reflets crédibles.
-   Le rivage ne présente pas de raccord visuellement cassé dans les
    scènes de référence.
-   La profondeur et la transparence varient de façon cohérente.
-   La météo influence l'apparence de l'eau.
-   Le rendu reste exploitable sur plusieurs profils graphiques.
-   Les données de profondeur et de navigation sont accessibles au
    gameplay.

### Objets et scripts

-   Les propriétés physiques sont sauvegardées.
-   La destructibilité est configurable.
-   Les scripts sont associés de manière persistante.
-   Une erreur Lua est visible sans faire tomber l'éditeur.
-   Les permissions des scripts sont appliquées.

### Modding

-   Une carte exportée peut être importée dans un projet vierge.
-   Les dépendances manquantes sont clairement signalées.
-   Les versions incompatibles sont détectées.
-   Les contenus non autorisés ne peuvent pas être intégrés
    silencieusement.
-   Le profil moddeur ne donne pas accès aux fonctions internes
    réservées au studio.

### Performance

-   La caméra peut se déplacer dans une grande carte sans exiger le
    chargement de tous les secteurs.
-   Les tâches lourdes ne bloquent pas durablement l'interface.
-   Les outils de diagnostic identifient les principaux coûts.
-   Les opérations locales ne reconstruisent pas inutilement les
    secteurs éloignés.

------------------------------------------------------------------------

## 20. Décisions techniques à prendre avant implémentation

Les points suivants demandent une décision d'architecture. Ils ne
doivent pas être tranchés implicitement pendant le développement.

1.  **Version de Bevy :** Bevy 0.20.1 est la version cible déjà intégrée au
    dépôt. Toute montée de version fera l'objet d'une évaluation de
    compatibilité, d'un changelog et d'un plan de migration explicite.
2.  **UI de l'éditeur :** choisir le toolkit et vérifier qu'il permet
    docking, panneaux personnalisés et viewport intégré.
3.  **Format de scène :** définir le format source, le format runtime et
    les migrations.
4.  **Terrain :** choisir heightmap, terrain maillé, représentation
    hybride ou autre approche.
5.  **Découpage du monde :** définir la taille des secteurs et leur
    système de coordonnées.
6.  **Eau :** choisir l'approche de rendu, les limites de simulation et
    les profils qualité.
7.  **Courbes :** choisir la représentation mathématique et la méthode
    de génération des meshes.
8.  **Physique :** définir l'intégration et la représentation des corps
    physiques.
9.  **Lua :** choisir le runtime, les limites de sandbox et le modèle
    client/serveur.
10. **Modding :** définir les permissions, dépendances et règles de
    distribution.
11. **Import d'assets :** définir les formats pris en charge et le
    pipeline de conversion.
12. **Undo/Redo :** choisir une architecture de commandes et de deltas
    adaptée au terrain.
13. **Streaming :** définir les politiques de chargement, priorités et
    caches.
14. **Compatibilité :** définir la stratégie de versionnage des cartes
    et mods.

------------------------------------------------------------------------

## 21. Résumé de la cible

L'éditeur MCF doit être un **Eden Editor étendu à la création complète
d'un monde**.

L'utilisateur doit pouvoir ouvrir une carte, naviguer en 2D comme sur
une carte classique, passer en 3D pour observer le relief, tracer ses
routes et ses cours d'eau, peindre ses sols, sculpter son terrain,
placer ses bâtiments et objets, configurer leurs propriétés, leur
physique et leurs scripts, puis tester le résultat sous différentes
heures et conditions météo.

Le système d'eau doit être traité comme une fonctionnalité majeure du
moteur, avec une qualité visuelle élevée dès le départ et des données
exploitables par le gameplay maritime.

Enfin, l'éditeur doit être conçu dès le début pour deux publics :
l'équipe MCF Studio, qui dispose de tous les outils, et les
joueurs/moddeurs, qui utilisent une version contrôlée permettant de
créer et partager leurs propres cartes.

**La priorité architecturale est de séparer les données du monde, les
outils d'édition, les systèmes runtime et les formats de distribution.**
Cette séparation permettra de faire évoluer l'éditeur sans rendre le jeu
dépendant de l'éditeur, et d'ouvrir le modding sans exposer les
mécanismes internes du moteur.
