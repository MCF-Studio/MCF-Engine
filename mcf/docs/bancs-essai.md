# Bancs d'essai décisifs : grosse carte et réseau

> Ce sont **ces deux tests** qui décident si le jeu RP passe de Godot à MCF Engine (décision d'Aroun,
> 2026-11-18). Ils doivent être **reproductibles** (une commande, même résultat), **comparables** (même scène,
> même scénario sous Godot 4.7) et **honnêtes** (résultats bruts publiés ici, bons ou mauvais).

## Matériel de référence

À fixer par Aroun avant les mesures (proposition : son PC de dev + une config « joueur moyen » type
RTX 3060 / Ryzen 5 / 16 Go). Les deux sont mesurées.

## Test A — Grosse carte

### La scène (générée, identique pour Bevy et Godot)

Un générateur (`mcf/crates/mcf_big_map`, commande `cargo big-map`) produit des calques `.mcfmap` déterministes
(graine fixe ; mêmes octets à chaque génération), un par cellule de 500 m, plus `manifest.json` et
`catalogue.json`. Les quantités ci-dessous sont celles des paliers `depart` et `stress` du générateur :

| Élément | Quantité de départ | Palier « stress » |
|---|---|---|
| Terrain | 4 × 4 km, heightmap 4097², 8 matériaux | 8 × 8 km |
| Bâtiments (extérieur mid poly) | 800, ~30 modèles différents | 2 000 |
| Intérieurs visitables | 100 | 300 |
| Objets posés (mobilier, props) | 80 000, ~200 modèles différents | 200 000 |
| Arbres | 30 000 | 100 000 |
| Lampes la nuit | 2 000 (dont 50 avec ombre) | 5 000 |
| Personnages animés | 100 | 300 |
| Véhicules | 200 | 500 |

Modèles mid poly selon les budgets de `rendu-lod-objets.md`, avec LOD. Base de départ possible : l'exemple
`examples/large_scenes/bevy_city` de Bevy (ville procédurale), étendu.

### Le scénario

Un parcours caméra enregistré (même trajet, même durée, ~5 min) : survol de toute l'île, descente en ville,
rue la plus dense à pied, entrée dans un intérieur, conduite rapide (streaming), nuit.

### Les mesures

| Mesure | Comment |
|---|---|
| FPS moyen, **1 % low**, pire image | temps par image, CPU et GPU séparés |
| Saccades de streaming | nombre d'images > 33 ms pendant la conduite |
| Mémoire vidéo et RAM | pic et moyenne |
| Temps de chargement initial | lancement → contrôle du joueur |
| Appels de dessin, triangles affichés | diagnostics de Bevy |
| Effet de chaque option | occlusion culling on/off, LOD on/off, HLOD on/off |

Seuil de réussite proposé : ≥ 60 FPS en 1 % low sur la config « joueur moyen » au palier de départ, aucune
saccade > 50 ms en conduite, et **au moins aussi bien que Godot** sur la même scène.

### Générateur : état (2026-11-18)

- ✅ Éléments posés : bâtiments (dont intérieurs), objets, arbres, lampes (dont 50 avec ombre), points
  d'apparition des personnages, véhicules. Ville carrée au centre (30 % du côté), îlots de 100 m, rues de 20 m,
  bâtiments sur des parcelles de 20 m distinctes ; arbres hors de la ville, 70 % en forêts.
- Mesuré (Ryzen 7 3700X, profil `release`) : palier `depart` = 113 100 éléments, 72 fichiers, 40,4 Mio, 1,2 s ;
  palier `stress` = 307 800 éléments, 272 fichiers, 110,2 Mio, 3,8 s. Plus gros fichier : 1,78 Mio (limite
  du format : 8 Mio).
- ⬜ Terrain (heightmap 4097², 8 matériaux) : toutes les hauteurs valent 0 pour l'instant.
- ⬜ Routes en `.mcfmap` (les rues existent dans la disposition, pas encore comme `roads`).
- ⬜ Modèles mid poly derrière les identifiants `bench_*` (catalogue du banc) et chargeur Bevy.
- ⬜ Même carte chargée sous Godot 4.7 (le format `.mcfmap` est commun : seul le catalogue est à brancher).

Les objets « en ville » sont tirés dans les îlots, donc parfois dans l'emprise d'un bâtiment (comme du
mobilier intérieur) : acceptable pour mesurer le rendu, à revoir si le test l'exige.

## Test B — Réseau

### Le montage

- 1 serveur dédié headless (conteneur Docker, CPU / RAM limités comme en production).
- N clients **bots** headless (pas de rendu) qui jouent un scénario RP : marcher, courir, conduire, entrer
  dans un bâtiment, parler (événements de chat), échanger un objet, se regrouper sur une place.
- Réseau dégradé simulé : 80 ms de latence, 20 ms de gigue, 1 % de pertes.
- Paliers : 32, 64, 128, 256 joueurs.
- Deux bibliothèques comparées sur le même scénario : **`lightyear`** et **`bevy_replicon` + `renet`**
  (les deux existent en version compatible Bevy 0.20, vérifié le 2026-11-18).

### Les mesures

| Mesure | Comment |
|---|---|
| Temps de tick serveur (moyen, p99) | à 30 et 60 ticks/s |
| Bande passante par client, montante et descendante | moyenne et pic, avec et sans interest management |
| CPU et RAM du serveur | par palier de joueurs |
| Fluidité côté client | erreurs de prédiction visibles (téléportations), délai ressenti |
| Robustesse | déconnexions / reconnexions, client qui envoie n'importe quoi (le serveur ne plante pas, ne se laisse pas tromper) |

Seuil proposé : 128 joueurs sur un seul serveur, tick p99 < 50 % du budget, < 40 ko/s descendant par client
en ville dense, et meilleur que la solution réseau actuelle du jeu sous Godot sur le même scénario.

## Résultats

Rien n'a encore été mesuré. Chaque campagne ajoute ici : date, commit, matériel, tableau brut, conclusion.
