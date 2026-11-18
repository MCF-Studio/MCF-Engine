# Comment Bevy 0.20.1 gère les objets, le rendu et les LOD

> Analyse faite dans le code du dépôt (`crates/bevy_camera`, `bevy_render`, `bevy_pbr`, `bevy_gltf`,
> `bevy_scene`), le 2026-11-18. Ce qui est noté « à mesurer » n'a **pas** encore été vérifié en chiffres : ce
> sont les bancs d'essai (`bancs-essai.md`) qui trancheront.

## En résumé

| Sujet | Ce que Bevy fait | Ce qu'il nous manque |
|---|---|---|
| Regroupement des objets | Automatique : même maillage + même matériau ⇒ un seul appel de dessin (instancing) ; rendu piloté par le GPU (`multi_draw_indirect`) ; maillages rangés dans de gros tampons partagés ; matériaux *bindless* | Rien de bloquant. Règle pour nous : **partager maillages et matériaux** (voir budgets) |
| Frustum culling | CPU par défaut + GPU (prétraitement) ; `NoCpuCulling` pour tout laisser au GPU | — |
| LOD | **Manuel** : composant `VisibilityRange` (plages de distance, fondu tramé sans « pop »). À poser **sur chaque entité**, il ne se propage pas aux enfants | **Génération des LOD** à l'import, et un composant MCF qui pose les plages tout seul |
| HLOD (regrouper plusieurs objets au loin) | Possible avec `VisibilityRange` (un maillage fusionné remplace un groupe) | Outil qui **fabrique** les maillages fusionnés par cellule |
| Occlusion culling | Oui, GPU, en deux passes (Hi-Z). **Désactivé par défaut**, exige un *depth prepass*, **incompatible avec le rendu différé** | À activer et **à mesurer** en ville |
| Meshlets (géométrie virtuelle, façon Nanite) | Expérimental. Vulkan/Metal seulement, GPU récents (atomiques 64 bits), pas de MSAA, matériaux limités, **coût de base élevé** | ❌ Pas pour nous : fait pour du très haute densité, nous visons du mid poly |
| Ombres | Cascades pour le soleil ; `ShadowLodOrigin` pour choisir la distance des ombres des lampes | Budget de lampes à ombres par zone |
| glTF | Matériaux PBR, lampes, animations, morph targets. **Pas** : `EXT_meshopt_compression`, Draco, `KHR_mesh_quantization`, `EXT_mesh_gpu_instancing`, `KHR_texture_basisu` (KTX2 marche, mais pas via l'extension glTF) | L'instancing vient de **notre** format de carte (un identifiant du catalogue ⇒ un même maillage chargé une fois) |
| Textures compressées | KTX2 / Basis (features `ktx2`, `basis-universal`), sauvegarde compressée par le traitement d'assets (`compressed_image_saver`) | Brancher le pipeline d'import dessus |
| Scènes | `bevy_scene` : scènes composables en code (**BSN**, macro `bsn!`), avec surcharges champ par champ. **Le format de fichier `.bsn` n'est pas encore sorti.** `bevy_world_serialization` : sauvegarde / chargement d'un monde par réflexion | L'éditeur enregistre en format réflexion (RON) aujourd'hui, et passera au `.bsn` quand Bevy le publiera |
| Précision des coordonnées | `f32` : ~1 mm de précision à 8 km de l'origine | OK pour une île de quelques km. Au-delà : origine flottante (`big_space`, pas encore en 0.20) |
| Rechargement à chaud | Assets (fichiers) rechargés à chaud ; code Rust : feature `hotpatching` (expérimental) | À tester pour l'éditeur (phase « Éditeur ») |

## Ce qu'on construit pour les LOD (phase 4, prototypé dès les bancs d'essai)

1. **À l'import** d'un modèle (glTF venu de Blender) : génération automatique de LOD1 et LOD2 par simplification
   (`meshopt`), en respectant les bords d'UV et les normales ; option « LOD fait main » si l'artiste fournit
   `_LOD1`, `_LOD2` dans le fichier (prioritaire sur l'automatique).
2. **Imposteur** (image du modèle sous plusieurs angles) pour les bâtiments et arbres lointains.
3. Composant **`McfLod`** : on pose l'objet, il crée lui-même les entités LOD avec les bonnes
   `VisibilityRange` (distances par catégorie, réglables dans le catalogue). Personne n'a à le faire à la main.
4. **HLOD par cellule** : au-delà d'une distance, tous les petits objets d'une cellule de la carte sont remplacés
   par un seul maillage fusionné, généré à l'enregistrement de la carte.
5. Ombres : les LOD bas projettent les ombres lointaines ; petits objets sans ombre au-delà d'une distance.

## Budgets « mid poly » (proposition de départ, à valider par les bancs d'essai)

Triangles au LOD0 ; LOD1 ≈ 50 %, LOD2 ≈ 20 %, puis imposteur ou disparition.

| Catégorie | LOD0 (triangles) | Distances LOD1 / LOD2 / fin | Textures |
|---|---|---|---|
| Petit objet (bouteille, chaise) | 300 – 2 000 | 15 / 35 / 80 m | atlas partagé, 512 px |
| Mobilier urbain (lampadaire, banc, poubelle) | 1 000 – 5 000 | 25 / 60 / 150 m | 1 k, matériaux partagés |
| Véhicule | 30 000 – 60 000 | 30 / 80 / 300 m | 2 k |
| Personnage (corps + vêtements) | 25 000 – 45 000 | 15 / 40 / 150 m | 2 k |
| Bâtiment (extérieur) | 10 000 – 50 000 | 80 / 200 m / imposteur | trim sheets 2 k, Poly Haven |
| Intérieur (par pièce) | 20 000 – 60 000 | chargé seulement si joueur proche / dedans | idem |
| Arbre | 5 000 – 15 000 | 40 / 100 m / imposteur | 1 k |

Règles qui comptent plus que le nombre de triangles :

- **Partager maillages et matériaux** : c'est ce qui permet le regroupement automatique de Bevy.
- **Peu de matériaux par objet** (idéalement 1–2) : chaque matériau d'un objet = une partie à dessiner.
- Textures en **KTX2 compressé** avec mipmaps (jamais de PNG brut en jeu).
- Lampes : **sans ombre par défaut**, l'ombre est une exception budgétée.
