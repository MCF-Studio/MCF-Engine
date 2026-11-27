<p align="center"><img src="branding/logo_512.png" alt="Logo MCF Engine" width="256"></p>

# `mcf/` — le code de MCF Studio

Logo du moteur : `branding/` (`logo_source.png` original, `logo_512.png` pour les docs, `icone_256.png` incluse
dans l'éditeur comme icône de fenêtre).

Tout ce qui est propre à MCF Engine vit ici, dans un **workspace Cargo séparé** de celui de Bevy (racine du
dépôt). Ainsi, le `Cargo.toml` de Bevy reste intact (synchronisation amont sans conflit), et on a nos propres
profils de compilation et notre `Cargo.lock`. Bevy est une dépendance par chemin (`..`).

```
mcf/
├── crates/
│   ├── mcf_map      format .mcfmap : lecture, écriture atomique, validation stricte (sans Bevy)
│   ├── mcf_geo      noyau de géométrie de carte (tracés, profils de route, rubans) : seule source du Plan et de la 3D (sans Bevy)
│   ├── mcf_core     briques communes jeu / éditeur / serveur, passage .mcfmap ↔ Transform
│   ├── mcf_meteo    météo et eau (hors feuille de route, voir docs/meteo.md)
│   ├── mcf_bench    mesures : temps par image, 1 % low, parcours caméra rejouable, CSV
│   ├── mcf_big_map  générateur déterministe de la grosse carte du test A (calques .mcfmap par cellule)
│   ├── mcf_editor   éditeur de base : hiérarchie, inspecteur, plan 2D, scène 3D et calques `.mcfmap`
│   ├── mcf_client   jeu (squelette)
│   └── mcf_server   serveur dédié headless (squelette)
├── bench/results/   résultats bruts des mesures (non versionnés)
├── bench/maps/      cartes générées par mcf_big_map (non versionnées, régénérables à l'identique)
├── lang/            🅿️ lecture du langage .mcf / .uimcf (garé)
└── docs/
    ├── rendu-lod-objets.md   comment Bevy gère objets / culling / LOD, budgets mid poly
    ├── bancs-essai.md        les deux tests qui décident de la migration
    └── meteo.md              portage de la météo et de l'eau (SkyForge), état des étapes
```

## Travailler

Toutes les commandes se lancent **depuis `mcf/`** (c'est là qu'est `.cargo/config.toml` : linker rapide, alias).

| Commande | Effet |
|---|---|
| `cargo editor` | Lance l'éditeur en mode dev (Bevy en bibliothèque dynamique : recompilation rapide) |
| `cargo editor -- chemin\\vers\\calque.mcfmap` | Lance l'éditeur avec un calque validé, visible simultanément en carte orthographique 2D et scène 3D |
| `Ctrl+S` dans l'éditeur | Réécrit atomiquement le calque ouvert après validation stricte ; aucun fichier n'est écrit tant qu'une destination n'est pas définie |

Depuis la scène de démonstration, le bouton **Créer un calque local** prépare une carte en mémoire. Indiquez ensuite un chemin finissant par `.mcfmap`, cliquez **Utiliser ce chemin**, puis **Enregistrer le calque** : aucune écriture n'a lieu avant cette dernière action explicite.

Pour ouvrir une carte convertie sans relancer l'application, collez son chemin dans **Chargement de carte** puis cliquez **Ouvrir ce calque**. Le fichier est validé avant de remplacer la scène ; si le calque courant a des modifications non enregistrées, l'éditeur demande d'abord de les enregistrer ou de les annuler.

L'éditeur permet de sélectionner un objet dans la hiérarchie, le plan 2D ou la scène 3D, puis de modifier sa position, sa rotation et son échelle. Faites glisser un objet sélectionné dans le plan 2D ou la scène 3D pour le déplacer (avec magnétisme si activé) : un geste complet correspond à une seule annulation `Ctrl+Z`. Dans le plan 2D, un glisser dans le vide encadre les objets ; maintenez `Ctrl` pour étendre la sélection. `Ctrl+A` sélectionne tout et `Échap` vide la sélection ; l'inspecteur conserve le dernier objet choisi comme objet principal. Le catalogue visible reprend les références présentes dans le calque et peut en poser de nouvelles instances.

L'inspecteur permet aussi de **verrouiller** l'objet principal : il reste visible et sélectionnable, mais ses déplacements, transformations, duplications et suppressions sont désactivés jusqu'au déverrouillage.

Le bouton **Masquer/Afficher** retire temporairement l'objet de la scène 3D, du plan 2D et des sélections, sans supprimer sa donnée de carte.

Caméra 3D : `WASD` pour se déplacer, `Shift` pour accélérer et clic droit maintenu pour regarder autour. Plan 2D : flèches pour le panoramique, `Q`/`E` ou la molette au-dessus du plan pour le zoom. La conception de carte 2D ↔ 3D est spécifiée dans [`../docs/SPEC_CONCEPTION_CARTE_2D_3D.md`](../docs/SPEC_CONCEPTION_CARTE_2D_3D.md).

Les routes ont 7 classes (`sentier`, `chemin_terre`, `route`, `ruelle`, `rue`, `avenue`, `voie_rapide`) dont le profil (chaussée, trottoirs ou accotements) vient de `mcf_geo` : le plan 2D et la 3D affichent exactement les mêmes bandes, continues dans les virages. Elles peuvent être sélectionnées directement sur leur ruban, en 2D comme en 3D.

Dans l’inspecteur d’une route, **+ Point final** et **− Point final** modifient le tracé (au minimum deux points), régénèrent les rubans 2D/3D, puis sont sauvegardés dans le `.mcfmap` lors de l’enregistrement explicite.
| `cargo client` | Lance le jeu en mode dev |
| `cargo server` | Lance le serveur dédié (`MCF_TICK_RATE=60 cargo server` pour changer la fréquence) |
| `cargo bench-scene -- --objets 100000` | Scène de calibrage, mesure puis écrit `bench/results/` |
| `cargo big-map -- --palier stress` | Génère la grosse carte du test A dans `bench/maps/` (`depart` par défaut ; `--graine`, `--cellule`, `--sortie`) |
| `cargo test --workspace` | Tous les tests |
| `cargo editor-profil -- <dossier>` | Éditeur profilé en direct dans **Tracy** (voir « Profiler ») |
| `cargo editor-trace -- <dossier>` | Éditeur qui écrit une trace `trace-<horodatage>.json`, à ouvrir dans https://ui.perfetto.dev |

### Profiler

Chaque système (les nôtres et ceux de Bevy) et chaque étape du rendu sont mesurés image par image.

- **Tracy (en direct)** : `cargo editor-profil -- <dossier>`, puis ouvrir l'application Tracy et se connecter. La version
  de Tracy doit correspondre à `tracy-client` (0.19.0 aujourd'hui : `cargo tree -p mcf_editor --features profil -i
  tracy-client`) d'après le tableau de https://github.com/nagisa/rust_tracy_client#version-support-table.
- **Trace dans un fichier** : `cargo editor-trace -- <dossier>`, quelques secondes sur la vue à étudier, fermer, puis
  glisser le fichier dans https://ui.perfetto.dev. Le format est bavard (≈ 1,8 Mo par image) : captures courtes.
  Les fichiers `mcf/trace-*.json` sont ignorés par git.
- Première compilation de chaque variante : Bevy est recompilé avec l'instrumentation (≈ 10 min sur le PC, `-j 6`).

Mesures de référence (2026-11-26, `ma_carte` 32 km, 23 466 bâtiments modélisés, mode dev) : ≈ 16 ms par image en vue
d'ensemble comme en ville ; chargement de la scène ≈ 10 s.
| `cargo clippy --workspace --all-targets -- -D warnings` | Lints (comme la CI) |
| `cargo fmt` | Mise en forme (sans `--all`, qui toucherait aussi Bevy) |

Prérequis : Rust ≥ 1.97.1 (MSVC sous Windows). Linux : `libasound2-dev libudev-dev libwayland-dev
libxkbcommon-dev`.

## Profils

- `dev` : notre code en `opt-level = 1`, Bevy et les dépendances en `3` (compilation rapide, jeu fluide).
- `release` : LTO « thin », 1 unité de compilation.
- `bench-release` : comme `release`, avec les numéros de ligne pour profiler. **Toutes les mesures se font dans ce
  profil**, jamais en `dev`.
