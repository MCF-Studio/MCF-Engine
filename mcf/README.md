# `mcf/` — le code de MCF Studio

Tout ce qui est propre à MCF Engine vit ici. Le reste du dépôt est Bevy, gardé aussi proche que possible de
l'amont (voir `ROADMAP.md`, principes).

```
mcf/
├── crates/   (phase 1) mcf_core, mcf_map, mcf_editor, mcf_server, mcf_client
├── bench/    (phase 1) scénarios de mesure reproductibles
├── lang/     🅿️ lecture du langage .mcf / .uimcf (garé)
└── docs/     specs détaillées par phase (permissions, menaces, isolation)
```
