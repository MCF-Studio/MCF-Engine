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
| 7 | Nuages volumétriques | Bruit Perlin-Worley 3D, carte météo, ray-march + reprojection temporelle, intégration avec l'atmosphère Bevy | `cloud_*.hlsl`, `Clouds.cpp` |
| 8 | Son d'ambiance | Mer, ressac, vent, pluie, tonnerre via `bevy_audio` (ou crate validé par `cargo deny`) | `Audio.cpp` |
| 9 | Banc d'essai | Scène dans `mcf_bench` pour mesurer le coût GPU de chaque passe | profileur GPU |

## Différences attendues avec SkyForge

- Tonemapping et exposition : ceux de Bevy, à recaler pour retrouver l'ambiance.
- Textures en lecture-écriture limitées par wgpu : passage en ping-pong (deux textures) quand nécessaire.
- Le panneau ImGui est remplacé par l'inspecteur de l'éditeur MCF (types `Reflect`).
- Sable et rochers de la plage de démo : restent hors du moteur (c'est du contenu de scène, pas du module).

## État

- ✅ Étape 1 : `EtatMeteo`, `PresetMeteo`, `TransitionMeteo`, message `ChangerMeteo`, 10 tests (`cargo test -p mcf_meteo`).
- ⬜ Étapes 2 à 9.

Ce travail est **hors feuille de route** : il ne passe pas devant la phase 2. Le crate n'est branché ni au client
ni à l'éditeur tant que l'équipe ne l'a pas décidé.
