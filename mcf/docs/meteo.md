# Météo et eau (`mcf_meteo`)

Objectif : refaire le module Météo & Eau de SkyForge (C++/DX12/HLSL) en Rust/Bevy/WGSL, dans `mcf/crates/mcf_meteo`,
sans toucher à `crates/bevy_*`. Repère MCF : Y vers le haut, main droite, avant = -Z, mètres. Budget GPU visé : 4 ms.

## Découpage en PR (petites, testées)

| # | PR | Contenu | Équivalent SkyForge |
|---|----|---------|---------------------|
| 1 | Squelette et état météo | `McfMeteoPlugin`, ressource `EtatMeteo` (préréglages Clair, Nuageux, Pluie, Tempête, Neige), transitions lissées, tests unitaires | `Weather.h/.cpp` |
| 2 | Vent | Champ de vent 3D en compute (texture 3D), rafales, exposé aux autres systèmes | `wind.hlsl`, `WindField` |
| 3 | Pluie et neige | Particules GPU (compute + rendu instancié), occlusion par carte de hauteur vue du ciel | `rain.hlsl`, `rain_draw.hlsl` |
| 4 | Humidité et neige au sol | Carte RGBA16F (mouillé, flaques, neige), extension de matériau Bevy pour les surfaces mouillées | `wetness.hlsl`, `WetSurface.hlsli` |
| 5 | Océan FFT | Spectre JONSWAP 3 cascades, FFT Stockham en compute, mousse par jacobien, LOD Gerstner | `ocean_*.hlsl`, `Ocean.cpp` |
| 6 | Vagues de bord | Shoaling, déferlement McCowan, run-up, mousse Worley | `ShoreWaves.hlsli` |
| 7 | Nuages volumétriques | Bruit Perlin-Worley 3D, carte météo, ray-march (reprojection temporelle plus tard), intégration avec l'atmosphère Bevy | `cloud_*.hlsl`, `Clouds.cpp` |
| 8 | Son d'ambiance | Mer, ressac, vent, pluie, tonnerre via `bevy_audio` (ou crate validé par `cargo deny`) | `Audio.cpp` |
| 9 | Banc d'essai | Scène dans `mcf_bench` pour mesurer le coût GPU de chaque passe | profileur GPU |

## Différences attendues avec SkyForge

- Tonemapping et exposition : ceux de Bevy, à recaler pour retrouver l'ambiance.
- Textures en lecture-écriture limitées par wgpu : passage en ping-pong (deux textures) quand nécessaire.
- Le panneau ImGui est remplacé par l'inspecteur de l'éditeur MCF (types `Reflect`).
- Sable et rochers de la plage de démo : restent hors du moteur (c'est du contenu de scène, pas du module).

## État

- ✅ Étape 1 : `EtatMeteo`, `PresetMeteo`, `TransitionMeteo`, message `ChangerMeteo`, 10 tests (`cargo test -p mcf_meteo`).
- ✅ Étape 2 (partie CPU) : `ChampVent` (profil de couche limite, rafales, turbulence à divergence nulle), bruit
  de Perlin identique à SkyForge, transport bouclé sur 2 500 m pour rester précis sur des heures de jeu, 9 tests.
  Le volume 3D sur GPU viendra avec la pluie (étape 3), qui est la première à en avoir besoin.
- ✅ Étape 3 : pluie et neige sur le GPU (`McfPrecipitationsPlugin`, feature `rendu`). Particules sans état
  calculées dans le vertex shader (`precipitations.wesl`) : un seul maillage, aucune simulation par image. Boîte
  toroïdale autour de la caméra, vitesses quantifiées pour boucler exactement sur 600 s, traînées inclinées par le
  vent local (`ChampVent`), part de neige selon la température. Démo : `cargo run -p mcf_meteo --example
  demo_meteo` (touches 1 à 5), capture sans écran avec `MCF_CAPTURE=capture.png`. 4 tests CPU.
  Coût GPU non mesuré : rendu vérifié seulement avec un GPU logiciel (llvmpipe), à mesurer dans `mcf_bench`.
- ✅ Étape 4a : abris. Les entités marquées `AbriPrecipitations` (toits, ponts, véhicules) arrêtent la pluie et la
  neige. Carte de hauteur 128 × 128 cases de 0,75 m autour de la caméra, construite sur le CPU depuis les boîtes
  englobantes (opt-in : un terrain ou un arbre ne bloque pas la pluie), reconstruite seulement quand la caméra
  change de case ou qu'un abri bouge. `CarteAbris::a_l_abri(p)` pour le gameplay. 6 tests.
  Limite : un abri est sa boîte englobante (un toit en pente abrite comme un toit plat à sa hauteur maximale).
- ✅ Étape 4b : surfaces. `SurfacesMeteo` (sans rendu) : le sol se mouille sous la pluie, sèche avec le vent,
  le soleil et la chaleur, les flaques se remplissent une fois la surface saturée, la neige tient quand il gèle et
  son eau de fonte mouille le sol (modèle de SkyForge). `MateriauMeteo` = `StandardMaterial` + extension
  (`materiau_meteo(base, porosité, réception des flaques, &carte)`) : albédo assombri et rugosité réduite selon la
  porosité, flaques avec ondulations des gouttes sur les surfaces planes, neige sur les faces tournées vers le
  ciel ; les zones abritées restent sèches. Uniformes renvoyés au GPU seulement si l'écart est visible (1/256).
  10 tests. Limites : valeurs globales (pas de carte d'humidité par endroit), opt-in matériau par matériau.
- ✅ Étape 5a : océan. `Ocean` (sans rendu) : 4 vagues de Gerstner calées sur le pic du spectre JONSWAP (vent et
  fetch de la météo), pulsations quantifiées pour boucler sur 200 s, `Ocean::hauteur(x, z)` pour la flottaison
  (inversion du déplacement horizontal par point fixe). `McfOceanPlugin` : maillage radial 256 × 256 sur 6 km qui
  suit la caméra (aligné sur ses cellules), déplacé dans le vertex shader, normale recalculée par pixel + rides,
  écume sur les crêtes, éclairage PBR de Bevy (reflets de l'atmosphère avec `AtmosphereEnvironmentMapLight`).
  Exemple `demo_ocean` (bouée qui flotte). 6 tests.
  Limites : pas encore de cascades FFT (la mer de tempête manque de vagues moyennes et d'écume), pas de vagues de
  bord ni d'eau peu profonde, coût GPU non mesuré.
- ✅ Étape 5b : cascades FFT. 3 cascades 256 × 256 (250, 47, 9 m) du spectre JONSWAP, calculées sur le GPU à chaque
  image dans le graphe de rendu (spectre initial quand le vent change, évolution, IFFT lignes puis colonnes en
  mémoire partagée, assemblage avec écume par jacobien en ping-pong). La houle de Gerstner garde le pic du spectre,
  la FFT ajoute la mer du vent (à partir de 1,5 × k_pic). LOD : petite cascade hors des sommets au-delà de 50 m,
  FFT effacée vers 1,5 km, détails de normale estompés avec la distance. 3 tests CPU, rendu vérifié par capture.
  Limites : `Ocean::hauteur` n'inclut pas la FFT (la bouée de la démo s'enfonce dans les grosses vagues de
  tempête ; il faudra une relecture GPU asynchrone), coût GPU non mesuré, mémoire partagée à 16 Kio (limite par
  défaut de wgpu).
- ✅ Étape 7 (faite avant la 6) : nuages volumétriques (`McfNuagesPlugin`, composant `NuagesVolumetriques` sur la
  caméra). Bruits cuits une fois sur le GPU (forme 128³ Perlin-Worley, détail 32³, carte météo 512²), marche en
  demi résolution dans la couche 1 500 – 4 200 m (pas fins près de la caméra : on vole dedans), arrêt devant la
  géométrie (profondeur), éclairage en unités physiques par le soleil de la scène (Henyey-Greenstein double lobe,
  diffusion multiple, effet « powder ») et une ambiance du ciel. Composition au début de la passe transparente :
  les nuages couvrent le ciel de l'atmosphère, la pluie reste devant. Dérive avec le vent bouclée sur 90 km
  (multiple commun des motifs : pas de saut). Démo `demo_ocean` (flèches haut / bas pour monter dans les nuages,
  `MCF_CAMERA=ciel|nuages|dessus`). 5 tests CPU, rendu vérifié par captures.
  Écart assumé avec SkyForge : la forme de base ne variait qu'entre 0,77 et 0,89 (mesuré sur 1 500 points), si
  bien que la couverture 0,6 du préréglage Nuageux donnait un ciel entièrement couvert ; elle est étalée sur
  [0, 1] pour que la couverture soit la fraction du ciel couverte. L'ambiance est calée à l'œil (pas de référence
  mesurée).
  Limites : pas de reprojection temporelle (décalage fixe par pixel + filtre en tente, l'image ne scintille pas
  mais les bords sont un peu flous), les reflets de l'océan et l'éclairage d'ambiance de la scène ne voient pas
  les nuages (la carte d'environnement vient de l'atmosphère seule), pas d'ombre des nuages au sol, MSAA coupé
  sur la caméra des nuages (rétabli à l'étape 18), coût GPU non mesuré (llvmpipe seulement : à mesurer en priorité, c'est la passe la
  plus lourde du module).
- ✅ Étape 6 : vagues de bord (`rivage`, sans rendu pour la partie calcul). Le jeu donne la hauteur du sol par une
  `SourceFond` (une fonction ; plus tard le terrain de la carte). Autour de la caméra, une grille 256 × 256 cases
  de 2 m échantillonne le fond et calcule la distance signée au rivage (transformée de distance euclidienne
  exacte, Felzenszwalb) et la normale du rivage ; reconstruite quand la caméra a bougé d'un quart de grille
  (128 m) : 3,5 ms en profil dev, 2,8 ms en release sur la machine de test pour un fond simple (pic ponctuel,
  à surveiller avec un vrai terrain). Modèle de SkyForge : levée de la houle (loi de Green), déferlement de McCowan, crête penchée vers la
  plage, jet de rive, écume de déferlement et traînée, liseré qui respire, fond visible à travers l'eau peu
  profonde. `rivage::hauteur_eau` pour la flottaison près des côtes. Démo `demo_plage`. 9 tests.
  Ajout par rapport à SkyForge : la houle du large est aussi limitée à 0,35 × la profondeur (sans cela, les creux
  d'une houle de tempête découvraient le fond par 10 m d'eau).
  Limites : la grille ne suit qu'une caméra (la première `Camera3d`), le motif d'écume est en bruit de Worley
  calculé par pixel (coût à mesurer), pas de sable mouillé par le jet de rive (l'humidité reste globale), pas de
  réfraction réelle (le fond est une couleur réglable, `ReglagesOcean::couleur_fond`), coût GPU non mesuré.
- ✅ Étape 8 : ambiance sonore et éclairs. `son` (sans dépendance audio) : synthèse en temps réel portée de
  SkyForge (mer en bruit brun filtré, ressac calé sur la phase des vagues de bord au rivage et panoramique vers
  lui, vent qui suit vitesse et rafales avec sifflement, pluie en souffle + gouttes, étouffée sous un abri,
  tonnerre avec claquement et roulement), filtres biquad RBJ, écrêtage doux. `Orage` : éclairs toutes les 7 à
  29 s par temps d'orage, message `Eclair` (délai du tonnerre = distance / 343 m/s), `Orage::eclat()` pour le
  flash. `McfSonPlugin` (feature `son`, dans les features par défaut) : source `bevy_audio` infinie, rendue par
  blocs de 10 ms sur le fil audio ; l'auditeur est l'entité marquée `AuditeurMeteo`. Exemple `ecoute_meteo` :
  écrit un WAV de 20 s par préréglage. Coût mesuré : 0,41 % d'un cœur en release (4,1 ms de calcul par seconde
  de son), 0,52 % en dev. 8 tests (niveaux, aigus de la pluie, vent, ressac calé sur les vagues, délai du
  tonnerre, éclairs, entrées).
  Limites : pas encore d'enregistrements (SkyForge pouvait lire des WAV à la place de la synthèse), pas de son
  spatialisé par source (un seul mixage stéréo autour de l'auditeur), écoute réelle non vérifiée ici (pas de
  carte son sur la machine de test : vérifié par mesures et fichiers WAV).
- ✅ Étape 9 : banc d'essai GPU (outil prêt, **mesure sur vrai GPU à faire**). `mcf_bench` écrit maintenant
  `passes.csv` : temps GPU de chaque passe de rendu (moyenne, p50, p99, pic), seulement quand une nouvelle
  mesure est arrivée (une passe qui ne tourne qu'une fois, comme la cuisson du bruit des nuages, n'est plus
  recomptée à chaque image). Passes météo nommées : `ocean_fft`, `nuages_bruit`, `nuages_marche` (la
  composition des nuages et la pluie sont dans `main_transparent_pass_3d`, l'océan dans `main_opaque_pass_3d`).
  Exemple `bench_meteo`, 4 variantes cumulatives pour isoler chaque brique : `base` (atmosphère, plage, ombres),
  `ocean`, `nuages`, `complet` (+ pluie / neige et son). Parcours caméra rejouable de 40 s : plage → ras de
  l'eau → montée dans la couche nuageuse → au-dessus → retour.

  ```text
  cargo bench-meteo -- --variante base    --preset tempete
  cargo bench-meteo -- --variante ocean   --preset tempete
  cargo bench-meteo -- --variante nuages  --preset tempete
  cargo bench-meteo -- --variante complet --preset tempete
  ```

  Options : `--preset clair|nuageux|pluie|tempete|neige`, `--duree <s>`, `--largeur`, `--hauteur` (1920×1080
  par défaut). Résultats dans `mcf/bench/results/<date>_meteo_<variante>/`.

  **Mesure sur vrai GPU (2026-11-28, RTX 3070, 1920 × 1080, tempête, MSAA ×4, commit `ed04e3832`)** : le
  module coûte ~0,8 ms GPU en moyenne en variante complet (`nuages_marche` 0,34 ms, `nuages_ciel` 0,20 ms,
  `ocean_fft` 0,15 ms, pluie et composition 0,08 ms, reste < 0,04 ms chacun), sous le budget de 4 ms ; 270 FPS
  moyens, GPU 2,4 ms par image. Variante ocean relancée (le 1er passage, à 66 FPS, était
  faussé) : 360 FPS, GPU 1,74 ms. Tableau brut et anomalie restante (p99 vers 17 ms partout) dans
  `bancs-essai.md`.
  Première mesure, faite ici : GPU **logiciel** (llvmpipe, 640×360, profil dev, tempête). Elle prouve que l'outil
  fonctionne, elle ne dit rien du coût réel. À titre brut : 7,7 / 5,2 / 4,4 / 4,2 images/s pour base / ocean
  / nuages / complet ; passes les plus lourdes en variante complet (moyenne GPU) : `nuages_marche` 32,8 ms,
  `atmosphere_luts` 32,2 ms, `ocean_fft` 31,3 ms, `lightprobe_irradiance_map` 31,2 ms. La cuisson du bruit des
  nuages (unique, au démarrage) a pris 613 ms sur llvmpipe.
- ✅ Étape 10 : flottaison sur la mer du vent. `mer_du_vent` (sans dépendance au rendu, donc aussi sur le
  serveur) reconstruit sur le CPU les cascades FFT du GPU à l'identique (mêmes graines, même spectre, mêmes
  fréquences quantifiées, même règle de recalcul quand le vent change) et garde les `modes_flottaison` modes les
  plus énergétiques (512 par défaut), sommés directement au point voulu. `rivage::hauteur_eau` prend la mer en
  option ; l'inversion du déplacement horizontal est amortie (le point fixe simple divergeait en tempête : 2 m
  d'écart horizontal après 4 itérations).
  Vérification contre le GPU (exemple `verifier_fft`, relecture de la texture, clair / nuageux / tempête) :
  écart RMS de 0,04 % sur les 3 cascades, soit la précision de la texture en demi-flottants. Cette
  vérification a trouvé un défaut du shader : la ligne et la colonne de Nyquist rompaient la symétrie
  hermitienne et faisaient déborder la pente sur le déplacement en z de la petite cascade (6 % d'erreur). Elles
  sont maintenant annulées.
  Erreur due aux modes gardés (vertical, RMS, 200 points, 512 modes) : 7 cm sur 18 cm (clair), 12 cm sur 59 cm
  (nuageux), 13 cm sur 93 cm (pluie), 12 cm sur 4,8 m (tempête) ; il manque surtout des rides de quelques cm.
  Coût (release, tempête) : 6 µs par image pour la mise à jour, 38 µs par objet pour `hauteur_eau`
  (29 µs avec 384 modes, 79 µs avec 1024). Recalcul du spectre quand le vent change : 1,1 à 1,3 ms.
  (Erreur et coût mesurés avant le calibrage de l'étape 11 ; après : voir étape 11.)
- ✅ Étape 11 : calibrage de la hauteur des vagues. Mesures avant correction (variance moyenne exacte,
  Hs = 4 × écart type de la hauteur) : la mer du vent seule dépassait la hauteur visée et la houle de Gerstner
  portait déjà toute la mer. Hs total : 21,6 m en tempête pour 10,0 m visés (vent 22 m/s, fetch 800 km), 2,9 m
  par temps nuageux pour 1,7 m. Trois causes :
  1. amplitude des modes de SkyForge `sqrt(2 S dk²)` au lieu de `sqrt(S dk² / 2)` (le mode et son conjugué
     comptent deux fois, et ξ a une variance 2) : mer du vent deux fois trop haute (corrigé dans le shader et
     la réplique CPU) ;
  2. en tempête, le pic (k ≈ 0,015) est plus près de 0 que la maille de la première cascade (0,025) : quelques
     modes portent tout l'étalement directionnel et la variance sortait 1,9 fois trop forte en amplitude. La
     première cascade saute maintenant au moins 3 mailles (`ANNEAUX_MIN`) ;
  3. la houle de Gerstner portait Hs entier : elle ne garde plus que la variance que la mer du vent n'a pas
     (`houle::part_houle`, `Ocean::variance_mer_du_vent`, mis à jour depuis la `MerDuVent`).
  Après (test `hauteur_significative_respectee`) : Hs 0,52 / 1,67 / 2,57 / 9,62 / 0,33 m pour 0,54 / 1,73 /
  2,59 / 10,05 / 0,34 m visés (clair / nuageux / pluie / tempête / neige). Vérification GPU toujours à 0,04 %.
  Flottaison avec 512 modes : erreur verticale RMS 3,5 / 6,1 / 6,4 / 5,7 cm ; `hauteur_eau` 33 µs par objet en
  tempête (release).
  Effet visible : la tempête est beaucoup plus calme qu'avant au premier plan (longue houle de 400 m, peu de
  clapot) et l'écume des crêtes a presque disparu (elle dépend du repliement de la mer du vent). Réglage de
  l'écume et du clapot de tempête à faire au goût.
- ✅ Étape 12 : pluie et neige éclairées par la scène. Leur luminance était fixe (2 500 cd/m²) : sous un soleil
  physique (130 000 lux, exposition EV100 13), gouttes et flocons sortaient presque noirs. Elle suit maintenant
  la `DirectionalLight` la plus forte : albedo × éclairement × (0,2 + 0,8 × soleil) / π.
- ✅ Étape 13 : écume calée sur la loi de Monahan (part de mer couverte de moutons selon le vent,
  W = 3,84·10⁻⁶ U^3,41). Après le calibrage de l'étape 11, l'écume avait disparu (0 % mesuré). Le seuil du
  jacobien est maintenant calculé depuis le spectre (écart type de la divergence de chaque cascade, loi
  normale) pour viser W, corrigé par une table mesurée (`persistance_ecume`) ; l'écume naît plus franchement
  (`(seuil − J) × 6` au lieu de × 1,5). Mesure (exemple `mesurer_ecume` : relecture de l'écume du GPU et
  formule du matériau sur 250 m × 250 m, intensité ≥ 0,5, llvmpipe, moyenne de 4 instants de 18 à 47 s ;
  `MCF_DUREE` choisit l'instant) : 0,10 / 0,68 / 1,32 / 14,4 % pour 0,09 / 0,69 / 1,37 / 14,5 % visés
  (5 / 9 / 11 / 22 m/s), écart entre instants ±0,04 point à 9 m/s, ±0,14 à 11 m/s, ±0,45 à 22 m/s. La table a été réglée
  sur ces mêmes instants (3 réglages, le dernier par interpolation) : à revérifier sur un vrai GPU.
- ✅ Étape 14 : reprojection temporelle des nuages. La marche ne calcule plus qu'un pixel sur 4 (bloc 2 × 2,
  ordre de Bayer) ; une passe `nuages_reconstruction` reprend les 3 autres dans l'image précédente (position
  monde = rayon × distance moyenne du nuage, ancienne `clip_from_world`), bornée au min / max des 3 × 3 blocs
  frais voisins. Hors écran ou sans historique : valeur fraîche. Désactivable (`reprojection: false`,
  `MCF_REPROJECTION=0` dans `demo_ocean`). Mesure (`cargo bench-meteo` variante nuages, 640 × 360, llvmpipe,
  2 passages) : `nuages_marche` 32,0 / 29,6 ms → 9,5 / 10,0 ms en moyenne (p50 18,4 → 8,2–8,7 ms, p99 120–140 →
  37–45 ms), + 1,2 ms de reconstruction ; image 222–224 → 188–195 ms, GPU total 168–170 → 144–149 ms.
  Captures fixes (ciel, dans les nuages, dessus) identiques à l'œil ; caméra qui tourne de 0,5° par image
  (`MCF_MOUVEMENT=1`) : pas de traînée visible, écart moyen 1,4 / 255 (ciel) et 0,6 / 255 (dans la couche).
  Non mesuré : vol rapide à travers la couche et vrai GPU.
- ✅ Étape 15 : ombre des nuages au sol et sur la mer (`ombre_nuages`, dans `McfNuagesPlugin`). Une passe
  `nuages_ombre` écrit chaque image la transmittance des nuages vue du soleil (carte 128 × 128 sur 8 km centrée
  sur la caméra, 62 m par texel, 16 pas dans la couche, densité grossière de la marche). La carte est posée sur
  le soleil comme `DirectionalLightTexture` : Bevy atténue le soleil sur **tous** les matériaux (océan, sable,
  `StandardMaterial`) sans toucher à leurs shaders ; la lumière du ciel reste. Translation et échelle du soleil
  sont réglées par le module (calées sur les texels : pas de scintillement), sa rotation reste au jeu.
  Conditions : feature Bevy `pbr_light_textures` (ajoutée aux exemples) et tableaux de textures sur le GPU (pas
  Metal) ; sans elles, aucune ombre et rien d'autre ne change. Le soleil ne doit pas avoir de parent.
  Réglages `ReglagesOmbreNuages` (active, demi-taille, pas). `demo_ocean` : `MCF_CAMERA=survol`,
  `MCF_SOLEIL=haut`, `MCF_OMBRE=0`, `MCF_OMBRE_PGM` (relecture de la carte).
  Mesure (`cargo bench-meteo` nuages, nuageux, 640 × 360, llvmpipe, 2 passages chacun) : `nuages_ombre`
  1,78–1,89 ms (6,8 ms avec une carte 256² et 24 pas, d'où la réduction) ; passe opaque 14,8 / 15,6 ms contre
  13,9 / 14,3 ms sans ombre ; image 192 / 199 ms contre 186 / 187 ms (écart plus grand que la somme des passes :
  bruit de llvmpipe probable).
  Constat : avec le préréglage nuageux, 17,5 % de la carte est au soleil par soleil à ~55°, presque rien par
  soleil bas (16°) : les rayons rasants traversent ~10 km de couche. C'est physiquement cohérent, mais plus
  sombre que la part de ciel bleu visible (la densité grossière, sans érosion, gonfle un peu les nuages). À juger
  en jeu ; sinon baisser `EtatMeteo::couverture_nuageuse` ou ajouter une force d'ombre réglable.
- ✅ Étape 16 : nuages dans les reflets et la lumière ambiante (`ciel_nuages`). La carte d'environnement de
  l'atmosphère de Bevy ne voyait que le ciel : sous la tempête, la mer reflétait un ciel bleu. La marche des
  nuages calcule maintenant aussi, depuis la caméra, un cube de nuages 32 × 32 par face, une face par image
  (`marcher_ciel`, même code que la marche de l'écran) ; après la passe opaque, la carte de l'atmosphère est
  copiée puis réécrite en nuages + transmittance × ciel, et Bevy la filtre à l'image suivante (une image de
  retard). Sans modification de Bevy. Actif quand la caméra porte `NuagesVolumetriques { ciel: true }` (par
  défaut) et `AtmosphereEnvironmentMapLight`. `demo_ocean` et `bench_meteo` : `MCF_CIEL=0` pour comparer.
  Effet mesuré sur la moitié basse de l'image (la mer), RVB moyen 0–255, avec / sans : tempête 42 / 45 / 46
  contre 76 / 96 / 116 (mer grise au lieu de bleue) ; nuageux 60 / 66 / 75 contre 63 / 83 / 102.
  Coût (`cargo bench-meteo` nuages, nuageux, 640 × 360, llvmpipe, 2 passages) : `nuages_ciel` 1,31 / 1,36 ms,
  `nuages_ciel_composition` 0,83 / 0,89 ms. Avec 64 × 64 par face : 3,7 / 3,8 ms pour une image identique à
  l'œil, d'où 32. Temps d'image trop bruité sur llvmpipe pour conclure (187 à 204 ms avec ou sans).
  À noter : le filtrage de la carte d'environnement par Bevy coûte ~50 ms sur llvmpipe
  (`lightprobe_irradiance_map` ~30 ms, `lightprobe_radiance_map` ~20 ms), avec ou sans nuages : à surveiller
  sur un vrai GPU.
- ✅ Étape 17 : sable mouillé par les vagues de bord (`MateriauMeteo`). L'humidité restait globale : par beau
  temps, le sable était sec jusqu'au bord de l'eau. Le matériau météo lit maintenant la grille du rivage de
  l'océan (s'il y en a un) et mouille le sol à moins de 60 m du rivage et sous la hauteur atteinte par l'eau :
  `rivage::hauteur_mouillee` = jet de rive + 10 cm, avec un fondu de 25 cm et une limite légèrement ondulée.
  Le jet suit le vent (0,29 m par temps clair, 0,45 m en tempête, réglage `ParametresRivage::jet_de_rive`) :
  sur la plage de démo (pente 4,5 %) par temps clair, sable mouillé sur ~3 m puis humide jusqu'à ~9 m du
  bord de l'eau au repos. Les vagues mouillent aussi sous un
  abri. Le sable de `demo_plage` est maintenant un `MateriauMeteo` (`MCF_SABLE=0` : sable simple).
  Premier essai écarté : en ajoutant la moitié de l'amplitude des vagues et 15 cm, la bande faisait 14 m et
  couvrait tout le premier plan ; un bord ondulé de ±12 cm faisait des taches sur des dizaines de mètres.
  Coût non mesuré (une lecture de texture et un bruit de plus par pixel du sol). 2 tests.
  Limites : pas de mémoire de l'humidité (le sable ne sèche pas après une grosse vague), pas de flaques
  laissées par la mer.
- ✅ Étape 18 : nuages compatibles avec le MSAA. Les nuages coupaient d'office le MSAA de leur caméra (la marche
  lisait une profondeur non multi-échantillonnée) : dans le jeu, les bords des objets crénelaient. La marche et
  la composition ont maintenant une variante par nombre d'échantillons, créée à la demande : profondeur
  `texture_depth_multisampled_2d` (échantillon 0, définition `MULTISAMPLED`) et composition dessinée avec le
  même nombre d'échantillons que la passe transparente. Le MSAA de la caméra est laissé tel quel (×4 par défaut
  dans Bevy). `demo_ocean` et `bench_meteo` : `MCF_MSAA=0` pour comparer.
  Mesure (`cargo bench-meteo` nuages, nuageux, 640 × 360, llvmpipe, 2 passages) : `nuages_marche` 12,9 / 12,2 ms
  avec MSAA ×4 contre 12,1 / 11,6 ms sans ; passe opaque 20,8 / 20,5 contre 15,8 / 16,7 ms (coût du MSAA lui-même) ;
  image 210 / 208 contre 197 / 202 ms. Captures : bords de la bouée lissés, nuages identiques à l'œil.
  Attention pour comparer avec les mesures précédentes : avant cette étape, les variantes `nuages` et `complet`
  du banc tournaient sans MSAA (coupé par les nuages), `base` et `ocean` avec.
- ✅ Étape 19 : grille du rivage reconstruite en tâche de fond. Mesure sur RTX 3070 (`bancs-essai.md`,
  2026-11-28 (2)) : 45 images de 13 à 16,6 ms en 40 s dans toutes les variantes avec océan, une à chaque
  reconstruction de la grille (256 × 256 appels au fond + transformée de distance, dans l'image). Le rendu
  demande maintenant la grille (`CarteRivage::demande`), la construit sur l'`AsyncComputeTaskPool` et garde
  l'ancienne, qui couvre encore la caméra, jusqu'à la fin (`CarteRivage::installer`). Première grille et
  changement de source ou de réglages : toujours construites tout de suite. `CarteRivage::mettre_a_jour`
  (calcul sur place) est inchangé pour le serveur et les tests. 1 test. Vérifié sur RTX 3070 (`bancs-essai.md`, 2026-11-28 (3)) : plus aucune image au-dessus de 10 ms, pire
  image 16,6 → 5,9 ms (ocean) et 16,5 → 6,8 ms (complet), coût GPU inchangé.
  Limite : à plus de ~100 m/s près d'une côte, la nouvelle grille arrive une ou deux images après le passage
  du quart de grille (sans effet visible attendu : l'ancienne couvre encore 128 m de marge).

Ce travail est **hors feuille de route** : il ne passe pas devant la phase 2. Le crate n'est branché ni au client
ni à l'éditeur tant que l'équipe ne l'a pas décidé.
