# Bancs d'essai décisifs : grosse carte et réseau

> Ce sont **ces deux tests** qui décident si le jeu RP passe de Godot à MCF Engine (décision d'Aroun,
> 2026-11-18). Ils doivent être **reproductibles** (une commande, même résultat), **comparables** (même scène,
> même scénario sous Godot 4.7) et **honnêtes** (résultats bruts publiés ici, bons ou mauvais).

## Matériel de référence

À fixer par Aroun avant les mesures (proposition : son PC de dev + une config « joueur moyen » type
RTX 3060 / Ryzen 5 / 16 Go). Les deux sont mesurées.

## Test A — Grosse carte

### La scène (générée, identique pour Bevy et Godot)

Un générateur (`mcf/bench/big_map`) produit un fichier `.mcfmap` déterministe (graine fixe) :

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
| Temps GPU par passe | `passes.csv` (`mcf_bench`), pour savoir quelle passe coûte |
| Effet de chaque option | occlusion culling on/off, LOD on/off, HLOD on/off |

Seuil de réussite proposé : ≥ 60 FPS en 1 % low sur la config « joueur moyen » au palier de départ, aucune
saccade > 50 ms en conduite, et **au moins aussi bien que Godot** sur la même scène.

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

Les tests A et B n'ont pas encore été mesurés. Chaque campagne ajoute ici : date, commit, matériel, tableau brut,
conclusion.

### 2026-11-28 — module météo (`cargo bench-meteo`), hors tests A / B

- Commit `ed04e3832` (branche `meteo/nuages-msaa`) ; le résumé indique `commit = ?` car git n'est pas dans le
  PATH de la machine.
- Matériel : PC de Panic_xXx_, NVIDIA GeForce RTX 3070 (pilote 610.88, Vulkan), AMD Ryzen 7 9800X3D, 31 Go,
  Windows 11 Home. Profil `bench-release`, `AutoNoVsync`, MSAA ×4, préréglage tempête, parcours caméra de 40 s
  après 8 s de chauffe. Fenêtre demandée 1920 × 1080, réelle 1920 × 1009 (barre des tâches).

| Variante | Images | FPS moyen | FPS 1 % low | ms moyen | ms p50 | ms p99 | ms pire | GPU moyen | GPU p99 | GPU pic |
|---|---|---|---|---|---|---|---|---|---|---|
| base | 19 819 | 495,5 | 57,5 | 2,018 | 1,677 | 16,469 | 110,573 | 1,409 | 2,336 | 6,921 |
| ocean | 2 655 | 66,4 | 31,8 | 15,066 | 16,585 | 30,921 | 32,526 | 2,954 | 6,614 | 7,520 |
| nuages | 9 696 | 242,3 | 48,2 | 4,127 | 2,689 | 17,097 | 31,870 | 2,355 | 5,061 | 8,481 |
| complet | 10 818 | 270,4 | 56,2 | 3,698 | 2,734 | 17,032 | 31,660 | 2,416 | 4,600 | 9,176 |

Passes du module météo, variante complet (ms GPU, moyenne / p50 / p99 / pic) :

| Passe | Moyenne | p50 | p99 | Pic |
|---|---|---|---|---|
| `nuages_marche` | 0,3357 | 0,2683 | 0,9615 | 1,8217 |
| `nuages_ciel` | 0,1970 | 0,2068 | 0,5939 | 1,3568 |
| `ocean_fft` | 0,1473 | 0,1413 | 0,2294 | 0,9861 |
| `main_transparent_pass_3d` (pluie + composition des nuages) | 0,0787 | 0,0778 | 0,1167 | 0,9236 |
| `nuages_reconstruction` | 0,0358 | 0,0338 | 0,0604 | 0,6758 |
| `nuages_ombre` | 0,0257 | 0,0236 | 0,0369 | 0,5161 |
| `nuages_ciel_composition` | 0,0136 | 0,0133 | 0,0215 | 0,5028 |
| **Somme des moyennes** | **0,835** | | | |

Passes de Bevy les plus lourdes, même variante : `lightprobe_irradiance_map` 0,49 ms, `atmosphere_luts`
0,38 ms, `lightprobe_radiance_map` 0,27 ms, `main_opaque_pass_3d` 0,13 ms (p99 1,68 ms, océan compris).

Constats :
- Le module météo coûte **~0,8 ms GPU** en moyenne (pire passe au p99 : `nuages_marche` 0,96 ms), sous le
  budget de 4 ms. Le filtrage de la carte d'environnement par Bevy (0,76 ms) coûte presque autant que tout le
  module.
- Les mesures sur llvmpipe (étapes 9 à 18) étaient 50 à 100 fois plus lentes et ne disent rien du coût réel.
- **Anomalie, variante ocean** : 66 FPS, p50 à 16,6 ms (cadence de 60 Hz) alors que le GPU ne travaille que
  2,95 ms par image, et que les variantes nuages et complet, plus lourdes, tournent à 240–270 FPS. Ce n'est pas
  le coût de l'océan ; cause probable : fenêtre non au premier plan ou cadence imposée pendant ce passage. À
  relancer.
- Toutes les variantes, base comprise, ont un p99 vers 16–17 ms et un 1 % low de 48–57 FPS : des images
  ponctuelles à la cadence de 60 Hz, indépendantes du module météo. À comprendre (présentation Windows ?).
- Pire image de base : 110 ms (une seule au-dessus de 33 ms), probablement au début de la mesure.
