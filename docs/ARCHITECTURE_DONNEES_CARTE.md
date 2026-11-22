# Données de carte : source, dérivées et runtime

## Principe

Les données sources sont la vérité versionnée, éditable et validable. Les
données dérivées sont reconstructibles à partir des sources et de versions
d'outils connues. Le `World` ECS n'est jamais le format de persistance de
production.

## Classification

| Famille | Source persistée | Dérivés reconstructibles | Consommateurs |
|---|---|---|---|
| Objets posés | `.mcfmap` : UID, catalogue, transform, propriétés autorisées | Entités Bevy, meshes, matériaux, AABB, instances GPU | Éditeur, client, serveur (données utiles) |
| Routes / courbes | Points, tangentes, modèle, intersections | Mesh de route, marquages, collision, navigation | Éditeur, client, serveur (graphe utile) |
| Terrain | Tuiles de heightmap, matériaux, masques et version | Meshes de LOD, collision, végétation, caches GPU | Éditeur, client, serveur (collision) |
| Environnement | Heure, météo, éclairage et paramètres d'eau | Ciel, probes, nuages, vagues, réflexions | Éditeur, client |
| Zones de gameplay | UID, forme, tags et propriétés validées | Index spatial, volumes ECS, debug draw | Éditeur, client, serveur |

## Invariants

1. Une source possède une version et une migration explicite avant tout
   changement de schéma.
2. Tout fichier externe est validé avant conversion : bornes, champs connus,
   UID, références de catalogue et droits associés.
3. Un cache dérivé porte la version de l'outil et les empreintes de ses sources.
   Il est supprimable et ne doit jamais être l'unique copie d'une carte.
4. Le serveur ne charge que les dérivés nécessaires à la simulation : collision,
   index, zones et données gameplay. Il ne dépend ni de l'UI ni du renderer.
5. Les opérations d'éditeur passent par des commandes structurées ; elles sont
   la seule entrée de l'historique, de la sauvegarde et d'une collaboration
   future.

## État actuel

`mcf_map` porte aujourd'hui les calques v1 et les valide strictement. L'éditeur
reconstruit ses entités depuis un calque et les réécrit par une sauvegarde
atomique. Les formats terrain, courbes générales, environnement et zones ne
sont pas encore définis : ils devront suivre cette classification, sans
sérialisation directe du `World`.
