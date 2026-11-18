# MCF Engine

Moteur de jeu de **MCF Studio**, construit sur [Bevy](https://github.com/bevyengine/bevy) **0.20.1**, et taillé
pour un seul objectif : notre **jeu RP 3D multijoueur**.

Ce que nous ajoutons à Bevy :

- 🗺️ un **éditeur de carte graphique**, simple et intuitif (inspiré de l'Eden Editor d'Arma 3) ;
- 🌍 terrain, routes, monde ouvert en streaming ;
- 🌐 réseau et serveur dédié autoritaire ;
- 🎭 les briques d'un RP : personnages, véhicules, intérieurs, chat vocal de proximité ;
- 🧪 un jour, la lecture du langage `.mcf` pour écrire les interfaces du jeu.

➡️ **[Feuille de route](../ROADMAP.md)** · code MCF dans [`mcf/`](../mcf/) · règles pour les IA dans
[`CLAUDE.md`](../CLAUDE.md)

## Démarrer

```sh
git clone https://github.com/MCF-Studio/MCF-Engine.git
cd MCF-Engine
cargo run --example 3d_scene      # vérifie que Bevy compile et s'affiche
```

Rust ≥ 1.97.1 requis (voir `Cargo.toml`).

## Licence

Bevy est distribué sous double licence MIT / Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) ; ces licences et les
crédits (`CREDITS.md`) sont conservés. Le code ajouté dans `mcf/` appartient à MCF Studio ; dépôt **privé**.
