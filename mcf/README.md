# `mcf/` — le code de MCF Studio

Tout ce qui est propre à MCF Engine vit ici. Le reste du dépôt est Bevy, gardé aussi proche que possible de
l'amont (voir `ROADMAP.md`, principes).

```
mcf/
├── crates/   (phase 1) mcf_core, mcf_map, mcf_bench, mcf_editor, mcf_server, mcf_client
├── bench/    (phase 2) scénarios de mesure : grosse carte, réseau
├── lang/     🅿️ lecture du langage .mcf / .uimcf (garé)
└── docs/     specs et analyses
    ├── rendu-lod-objets.md   comment Bevy gère objets / culling / LOD, budgets mid poly
    └── bancs-essai.md        les deux tests qui décident de la migration
```
