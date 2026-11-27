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
  sur la caméra des nuages, coût GPU non mesuré (llvmpipe seulement : à mesurer en priorité, c'est la passe la
  plus lourde du module).
- ⬜ Étapes 6, 8 et 9.

Ce travail est **hors feuille de route** : il ne passe pas devant la phase 2. Le crate n'est branché ni au client
ni à l'éditeur tant que l'équipe ne l'a pas décidé.
