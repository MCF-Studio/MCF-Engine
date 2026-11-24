# Convention carte 2D → MCF

> **Obsolète depuis le 2026-11-22** (ADR-0001). Remplacée par `SPEC_CONCEPTION_CARTE_2D_3D.md`. Le convertisseur
> `mcf_map_import_2d` et l'exemple `port_demo` ont été retirés ; ce document n'est gardé que pour l'historique.
> Pour préparer une carte qui sert de modèle à l'éditeur : [`CONVENTION_CARTES_SOURCE.md`](CONVENTION_CARTES_SOURCE.md).

Ce document définit le contrat du futur outil de création de cartes 2D. Il
permet de transformer sans ambiguïté un plan, ses routes et ses zones en
données MCF 3D. La source 2D est une donnée d'auteur ; elle n'est jamais
chargée comme du code et ne remplace pas la validation stricte de `.mcfmap`.

## Repère unique

- Unité monde : **mètre**.
- Monde MCF : X vers la droite, Y vers le haut, Z vers le bas de l'écran quand
  le plan regarde le nord ; avant = −Z.
- Image source : origine en haut à gauche, X vers la droite, Y image vers le
  bas.
- Conversion d'un point image `(px, py)` :

  ```text
  x = (px - origin_px_x) × metres_per_pixel
  y = altitude_m
  z = -(py - origin_px_y) × metres_per_pixel
  ```

`origin_px` et `metres_per_pixel` sont obligatoires : aucune valeur implicite
ne doit dépendre de la taille du PNG ou de l'écran.

## Fichier source `.mcf2d.json`

Le futur outil écrit un JSON UTF-8. Les champs inconnus seront refusés par le
convertisseur afin que les cartes restent auditables.

Un exemple complet est fourni dans
[`exemples/port_demo.mcf2d.json`](exemples/port_demo.mcf2d.json).

```json
{
  "format": "mcf2d",
  "version": 1,
  "name": "port_demo",
  "metres_per_pixel": 0.5,
  "origin_px": [2048, 2048],
  "background": "port_demo.png",
  "roads": [
    {
      "id": "rue_port",
      "class": "avenue",
      "altitude_m": 0,
      "points": [[1800, 2100], [2048, 2100], [2500, 2250]]
    }
  ],
  "zones": [
    {
      "id": "quai_nord",
      "kind": "port",
      "polygon": [[1900, 1900], [2200, 1900], [2200, 2050], [1900, 2050]]
    }
  ]
}
```

Champs racine autorisés : `format`, `version`, `name`, `metres_per_pixel`,
`origin_px`, `background`, `roads`, `junctions`, `zones`. Limites recommandées : identifiants ASCII minuscules `a-z`, `0-9`, `_` et
`-`, 2 à 200 points par route, 3 à 200 points par zone et segments de route
inférieurs à 400 m après conversion. L'outil doit refuser les polygones
auto-intersectés et les identifiants dupliqués.

`altitude_m` est optionnel sur une route, un carrefour ou une zone ; sa valeur
par défaut est `0`. Un carrefour peut aussi préciser `yaw_deg` (orientation
du plan, en degrés, même convention que les bras) et un rond-point
`island_m` (rayon de l'îlot en mètres). Ces deux dernières valeurs doivent
être converties par l'outil, jamais interprétées par le runtime.

## Routes et carrefours

`roads[].class` se convertit directement vers `MapRoad.template` :

| Classe 2D | Template `.mcfmap` | Usage |
|---|---|---|
| `chemin` | `chemin` | piste, chemin piéton |
| `route` | `route` | rue standard |
| `avenue` | `avenue` | axe principal |

Les largeurs de référence, appliquées identiquement par l’éditeur aux rubans
2D et 3D, sont : `chemin` = 2 m, `route` = 4 m et `avenue` = 6 m. Elles ne
sont pas recalculées depuis les pixels afin de conserver le même bord de voie
dans les deux vues.

Les points deviennent `[x, altitude_m, z]` dans `roads[].points`. Une
intersection à trois branches ou plus devient un `junction` explicite : elle
ne doit pas être déduite silencieusement par le runtime. Son centre est le
point d'intersection converti et chaque branche indique sa classe.

Dans la source 2D, `junctions[].origin` reste en pixels et `arms[].angle` est
en degrés, avec `0` pointant vers le haut de l'image (nord), puis dans le sens
horaire. Le convertisseur applique le repère MCF et convertit ces bras vers
`MapJunction`.

## Zones

Le format `.mcfmap` actuel ne contient pas encore de `MapZone`. Les zones
restent donc dans la source `.mcf2d.json` et le convertisseur produit, en
attendant ce type natif, des **marqueurs non rendus** dont le catalogue suit la
convention `markers/zone/<kind>`. Chaque marqueur garde l'identifiant de zone
et sa géométrie dans un fichier d'accompagnement `.mcfzones.json` versionné.

Exemples de `kind` stables : `port`, `quartier`, `parking`, `interieur`,
`zone_ia`, `zone_interdite`. Les noms affichés et les couleurs sont des
métadonnées d'outil, jamais une logique exécutée par la carte.

## Sorties attendues du convertisseur

Pour `ville.mcf2d.json`, générer dans un dossier isolé :

```text
ville.mcfmap        objets, routes et carrefours validés
ville.mcfzones.json polygones de zones et métadonnées d'édition
ville.import.json   source, version, date, paramètres et avertissements
```

Le convertisseur ne doit jamais écraser une sortie existante sans demande
explicite. Il valide chaque sortie avec `mcf_map`, écrit atomiquement puis
rapporte séparément erreurs, avertissements et informations.

## Convertisseur disponible

Le convertisseur de référence est disponible dans le workspace Rust :

```powershell
cd mcf
cargo run -p mcf_map --bin mcf_map_import_2d -- `
  ..\docs\exemples\port_demo.mcf2d.json ..\sorties\port_demo
```

Il crée `port_demo.mcfmap`, `port_demo.mcfzones.json` et
`port_demo.import.json` dans le dossier de sortie. Toute sortie déjà présente
fait échouer la commande ; ajouter `--ecraser` constitue la demande explicite
de remplacement. Le convertisseur refuse les champs inconnus, les nombres non
finis, les identifiants non sûrs, les routes hors contrat et les polygones de
zone auto-intersectés.
