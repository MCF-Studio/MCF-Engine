# `mcf/` — le code de MCF Studio

Tout ce qui est propre à MCF Engine vit ici, dans un **workspace Cargo séparé** de celui de Bevy (racine du
dépôt). Ainsi, le `Cargo.toml` de Bevy reste intact (synchronisation amont sans conflit), et on a nos propres
profils de compilation et notre `Cargo.lock`. Bevy est une dépendance par chemin (`..`).

```
mcf/
├── crates/
│   ├── mcf_map      format .mcfmap : lecture, écriture atomique, validation stricte (sans Bevy)
│   ├── mcf_core     briques communes jeu / éditeur / serveur, passage .mcfmap ↔ Transform
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
    └── bancs-essai.md        les deux tests qui décident de la migration
```

## Travailler

Toutes les commandes se lancent **depuis `mcf/`** (c'est là qu'est `.cargo/config.toml` : linker rapide, alias).

| Commande | Effet |
|---|---|
| `cargo editor` | Lance l'éditeur en mode dev (Bevy en bibliothèque dynamique : recompilation rapide) |
| `cargo editor -- chemin\\vers\\calque.mcfmap` | Lance l'éditeur avec un calque validé, visible simultanément en carte orthographique 2D et scène 3D |
| `cargo run -p mcf_map --bin mcf_map_import_2d -- plan.mcf2d.json sorties` | Convertit strictement un plan 2D conventionné en `.mcfmap`, zones et manifeste ; ajoutez `--ecraser` seulement pour remplacer les sorties |
| `Ctrl+S` dans l'éditeur | Réécrit atomiquement le calque ouvert après validation stricte ; aucun fichier n'est écrit tant qu'une destination n'est pas définie |

Depuis la scène de démonstration, le bouton **Créer un calque local** prépare une carte en mémoire. Indiquez ensuite un chemin finissant par `.mcfmap`, cliquez **Utiliser ce chemin**, puis **Enregistrer le calque** : aucune écriture n'a lieu avant cette dernière action explicite.

Pour ouvrir une carte convertie sans relancer l'application, collez son chemin dans **Chargement de carte** puis cliquez **Ouvrir ce calque**. Le fichier est validé avant de remplacer la scène ; si le calque courant a des modifications non enregistrées, l'éditeur demande d'abord de les enregistrer ou de les annuler.

L'éditeur permet de sélectionner un objet dans la hiérarchie, le plan 2D ou la scène 3D, puis de modifier sa position, sa rotation et son échelle. Faites glisser un objet sélectionné dans le plan 2D ou la scène 3D pour le déplacer (avec magnétisme si activé) : un geste complet correspond à une seule annulation `Ctrl+Z`. Dans le plan 2D, un glisser dans le vide encadre les objets ; maintenez `Ctrl` pour étendre la sélection. `Ctrl+A` sélectionne tout et `Échap` vide la sélection ; l'inspecteur conserve le dernier objet choisi comme objet principal. Le catalogue visible reprend les références présentes dans le calque et peut en poser de nouvelles instances.

L'inspecteur permet aussi de **verrouiller** l'objet principal : il reste visible et sélectionnable, mais ses déplacements, transformations, duplications et suppressions sont désactivés jusqu'au déverrouillage.

Le bouton **Masquer/Afficher** retire temporairement l'objet de la scène 3D, du plan 2D et des sélections, sans supprimer sa donnée de carte.

Caméra 3D : `WASD` pour se déplacer, `Shift` pour accélérer et clic droit maintenu pour regarder autour. Plan 2D : flèches pour le panoramique, `Q`/`E` ou la molette au-dessus du plan pour le zoom. La convention et le convertisseur de carte 2D sont documentés dans [`../docs/CONVENTION_CARTE_2D_VERS_MCFMAP.md`](../docs/CONVENTION_CARTE_2D_VERS_MCFMAP.md).

Les routes utilisent le même gabarit dans les deux vues : `chemin` (2 m), `route` (4 m) et `avenue` (6 m). Elles peuvent être sélectionnées directement sur leur ruban, en 2D comme en 3D.

Dans l’inspecteur d’une route, **+ Point final** et **− Point final** modifient le tracé (au minimum deux points), régénèrent les rubans 2D/3D, puis sont sauvegardés dans le `.mcfmap` lors de l’enregistrement explicite.
| `cargo client` | Lance le jeu en mode dev |
| `cargo server` | Lance le serveur dédié (`MCF_TICK_RATE=60 cargo server` pour changer la fréquence) |
| `cargo bench-scene -- --objets 100000` | Scène de calibrage, mesure puis écrit `bench/results/` |
| `cargo big-map -- --palier stress` | Génère la grosse carte du test A dans `bench/maps/` (`depart` par défaut ; `--graine`, `--cellule`, `--sortie`) |
| `cargo test --workspace` | Tous les tests |
| `cargo clippy --workspace --all-targets -- -D warnings` | Lints (comme la CI) |
| `cargo fmt` | Mise en forme (sans `--all`, qui toucherait aussi Bevy) |

Prérequis : Rust ≥ 1.97.1 (MSVC sous Windows). Linux : `libasound2-dev libudev-dev libwayland-dev
libxkbcommon-dev`.

## Profils

- `dev` : notre code en `opt-level = 1`, Bevy et les dépendances en `3` (compilation rapide, jeu fluide).
- `release` : LTO « thin », 1 unité de compilation.
- `bench-release` : comme `release`, avec les numéros de ligne pour profiler. **Toutes les mesures se font dans ce
  profil**, jamais en `dev`.
