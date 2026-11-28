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
| ocean (1er passage, faussé) | 2 655 | 66,4 | 31,8 | 15,066 | 16,585 | 30,921 | 32,526 | 2,954 | 6,614 | 7,520 |
| ocean (relance) | 14 407 | 360,2 | 52,4 | 2,776 | 1,941 | 16,962 | 32,285 | 1,738 | 5,276 | 8,264 |
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
- **Variante ocean, 1er passage faussé** : 66 FPS, p50 à 16,6 ms (cadence de 60 Hz) pour 2,95 ms de GPU.
  Relancée seule à 06:20 UTC, fenêtre au premier plan sans y toucher : **360 FPS**, p50 1,94 ms, GPU 1,74 ms.
  Le premier passage était donc un accident de mesure (cause exacte non établie : focus de la fenêtre le plus
  probable). L'océan coûte ~0,33 ms GPU de plus que la base (1,74 contre 1,41 ms en moyenne). On garde la ligne
  faussée dans le tableau pour la traçabilité.
- Toutes les variantes ont un p99 vers 16–17 ms et un 1 % low de 48–57 FPS. Lecture de `frames.csv` image par
  image : ce ne sont pas des à-coups isolés mais des **blocs de plusieurs secondes calés à 60 Hz** (chaque image
  16,2–17,1 ms, GPU 1,6–4 ms) : base 284 images > 10 ms, presque toutes entre 19 et 25 s ; ocean (relance) 703,
  entre 0–5 s et 30–40 s ; complet 688, presque toutes entre 0 et 15 s. Même phénomène que le 1er passage ocean,
  en plus court, extérieur au rendu (présentation Windows, focus ou fenêtre recouverte : non établi). Les temps
  GPU restent valables ; **FPS moyens, p99 d'image et 1 % low sont pollués et ne se comparent pas entre
  variantes**.
  Le banc note maintenant, pour chaque image, si la fenêtre a le focus et si le système la dit recouverte
  (colonnes `focus` et `masquee` de `frames.csv`, totaux `images_sans_focus` et `images_fenetre_masquee` du
  résumé) : à la prochaine mesure, on saura si ces blocs coïncident avec une perte de focus.
- Pire image de base : 110 ms (une seule au-dessus de 33 ms), probablement au début de la mesure.

### 2026-11-28 (2) — module météo, fenêtre au premier plan vérifiée

- Branche `bench/focus-fenetre` (commit `a2984b47b`, même code de rendu que la mesure précédente ; le banc note
  en plus le focus de la fenêtre). Même machine, mêmes réglages. `images_sans_focus = 0` et
  `images_fenetre_masquee = 0` pour les 4 lignes.

| Variante | Images | FPS moyen | FPS 1 % low | ms moyen | ms p50 | ms p99 | ms pire | GPU moyen | GPU p99 | GPU pic |
|---|---|---|---|---|---|---|---|---|---|---|
| base | 21 818 | 545,4 | 350,9 | 1,833 | 1,715 | 2,694 | 3,343 | 1,431 | 1,973 | 2,580 |
| ocean | 18 268 | 456,7 | 140,2 | 2,190 | 2,014 | 4,382 | 16,614 | 1,721 | 3,432 | 4,878 |
| nuages | 13 988 | 349,7 | 124,9 | 2,860 | 2,691 | 4,997 | 16,767 | 2,285 | 3,961 | 5,200 |
| complet | 13 661 | 341,5 | 118,6 | 2,928 | 2,756 | 5,125 | 16,496 | 2,333 | 4,063 | 5,498 |

Passes rejetées : un passage nuages entièrement sans focus (2 401 images sur 2 401, 60,0 FPS pile), un autre qui
perd le focus à 6 s (117 FPS). Le passage gardé est le troisième (06:59 UTC).

Constats :
- **Les blocs calés à 60 Hz de la première mesure venaient de la perte de focus** : 100 % des images du passage
  rejeté sont à `focus = 0`, aucune n'est « masquée ». Fenêtre au premier plan, base n'a plus aucune image
  au-dessus de 10 ms. Les FPS de la première mesure sont à oublier ; ses temps GPU étaient justes.
- Coût GPU moyen par rapport à la base : océan +0,29 ms, nuages (avec l'océan) +0,85 ms, complet +0,90 ms.
- **À-coup réel, variantes avec océan** : 44 à 45 images de 13 à 16,6 ms, focus présent, GPU à 1,6–2,5 ms, aux
  mêmes instants dans ocean, nuages et complet (7,02 / 9,23 / 10,77 / 12,08 s…), aucune dans base. Cause : la
  reconstruction de la grille du rivage, faite dans l'image à chaque quart de grille (128 m) parcouru par la
  caméra. Le parcours du banc rejoué hors rendu donne 45 reconstructions aux mêmes instants (7,00 / 9,21 /
  10,75 / 12,06 s…), 17,5 à 23,6 ms chacune sur la machine de développement (le fond du banc appelle un bruit de
  Perlin par case ; l'étape 6 annonçait 2,8 ms pour un fond simple). Corrigé à l'étape 19 de `meteo.md`
  (construction en tâche de fond) : **à remesurer**.

### 2026-11-28 (3) — après l'étape 19 (grille du rivage en tâche de fond)

- Branche `meteo/rivage-arriere-plan` (commit `3cd9d365b`), même machine, mêmes réglages, 0 image sans focus.
  Fenêtre réelle 1920 × 1080 cette fois (1920 × 1009 aux mesures précédentes : ~7 % de pixels en plus).

| Variante | Images | FPS moyen | FPS 1 % low | ms moyen | ms p50 | ms p99 | ms pire | GPU moyen | GPU p99 | GPU pic |
|---|---|---|---|---|---|---|---|---|---|---|
| ocean | 18 735 | 468,4 | 217,7 | 2,135 | 1,989 | 4,149 | 5,890 | 1,680 | 3,359 | 4,925 |
| complet | 13 630 | 340,7 | 183,9 | 2,935 | 2,800 | 4,983 | 6,778 | 2,347 | 4,042 | 5,497 |

Constats :
- **Les 45 à-coups ont disparu** : plus aucune image au-dessus de 10 ms (45 avant dans chaque variante), rien aux
  instants 7,02 / 9,23 / 10,77 s. Pire image 16,6 → 5,9 ms (ocean) et 16,5 → 6,8 ms (complet) ; 1 % low
  140 → 218 FPS et 119 → 184 FPS. Coût GPU moyen inchangé (1,72 → 1,68 ms et 2,33 → 2,35 ms).
- Images restantes au-dessus de 5 ms (30 en ocean, 131 en complet, toutes ≤ 6,8 ms) : groupées à 0–0,7 s, vers
  8 s et à 39,6–40 s, avec un GPU à 3,4–5,5 ms. Ce sont des vues chargées du parcours, pas des à-coups CPU.
- base et nuages non remesurées sur cette branche.

