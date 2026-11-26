# `mcf/` — le code de MCF Studio

Tout ce qui est propre à MCF Engine vit ici, dans un **workspace Cargo séparé** de celui de Bevy (racine du
dépôt). Ainsi, le `Cargo.toml` de Bevy reste intact (synchronisation amont sans conflit), et on a nos propres
profils de compilation et notre `Cargo.lock`. Bevy est une dépendance par chemin (`..`).

```
mcf/
├── crates/
│   ├── mcf_map      format .mcfmap : lecture, écriture atomique, validation stricte (sans Bevy)
│   ├── mcf_core     briques communes jeu / éditeur / serveur, passage .mcfmap ↔ Transform
│   ├── mcf_meteo    météo et eau (hors feuille de route, voir docs/meteo.md)
│   ├── mcf_bench    mesures : temps par image, 1 % low, parcours caméra rejouable, CSV
│   ├── mcf_editor   éditeur (squelette ; interface en phase 3)
│   ├── mcf_client   jeu (squelette)
│   └── mcf_server   serveur dédié headless (squelette)
├── bench/results/   résultats bruts des mesures (non versionnés)
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
| `cargo client` | Lance le jeu en mode dev |
| `cargo server` | Lance le serveur dédié (`MCF_TICK_RATE=60 cargo server` pour changer la fréquence) |
| `cargo bench-scene -- --objets 100000` | Scène de calibrage, mesure puis écrit `bench/results/` |
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
