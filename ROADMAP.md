# MCF Engine — feuille de route

> Moteur de MCF Studio, construit sur **Bevy 0.20.1** (MIT / Apache-2.0), pour **un seul projet** : le jeu
> RP 3D de MCF Studio (île, ville, multijoueur sur serveur dédié), visé en **mid poly** et **le plus optimisé
> possible**.
> Une phase n'est terminée que lorsque son **critère de fin** est vérifié (test automatique ou démonstration
> enregistrée). Les cases ✅ ne sont cochées qu'avec la preuve associée.

## Cap (décisions d'Aroun, 2026-11-18)

- **Objectif final : migrer le jeu RP** de Godot vers MCF Engine. **Sans précipitation** : on travaille le moteur
  d'abord, la transition vient ensuite.
- **Ce sont les tests qui décident.** Les deux points qui feront basculer ou non : **la gestion d'une grosse
  carte** et **le réseau**. On les mesure donc **tôt** (phase 2), avant d'investir lourdement dans le reste.
- Il faut une **vraie interface** : gestion des scènes, des scripts, des animations, et l'éditeur de carte.

| Phase | Objectif | État |
|---|---|---|
| 0 | Fondations : fork, dépôt, règles, synchro amont | ✅ fork 0.20.1, dépôt privé, CI amont retirée |
| 1 | Squelette MCF, CI légère, builds rapides, outils de mesure | ✅ fenêtre, CI verte (Windows compris), recompilation ≈ 1,8 s |
| 2 | **🎯 Bancs d'essai décisifs : grosse carte + réseau** (comparés à Godot) | 🟡 générateur de la grosse carte |
| — | **Décision : on continue vers la migration, on réoriente, ou on arrête** | ⬜ |
| 3 | **Éditeur MCF** : scènes, scripts, animations, carte | ⬜ |
| 4 | Terrain, sol, routes, végétation | ⬜ |
| 5 | Monde ouvert de production : streaming, LOD, HLOD, occlusion | ⬜ |
| 6 | Réseau de production et serveur dédié | ⬜ |
| 7 | Briques RP (personnages, véhicules, intérieurs, voix) | ⬜ |
| 8 | 🧪 La petite folie : lecture du langage `.mcf` / `.uimcf` | 🟡 prototype fusionné (PR #2) : `mcf_lang_bridge` affiche un `.uimcf` (clic, saisie, survol) ; licence D67 tranchée ; pas branché au jeu (voir `mcf/lang/`) |
| 9 | Production : pipeline d'assets, packaging, launcher | ⬜ |
| 10 | **Transition du jeu** Godot → MCF Engine | ⬜ |

Documents de référence :
- [`mcf/docs/rendu-lod-objets.md`](mcf/docs/rendu-lod-objets.md) : comment Bevy gère objets, culling, LOD,
  scènes ; budgets mid poly.
- [`mcf/docs/bancs-essai.md`](mcf/docs/bancs-essai.md) : protocole et seuils des deux tests décisifs.

---

## Principes

1. **On ne réécrit pas Bevy, on l'étend.** Notre code vit dans `mcf/`. On ne modifie `crates/bevy_*` que pour une
   optimisation **mesurée**, en commit isolé préfixé `[bevy-patch]`, si possible proposée en amont. Objectif :
   suivre les versions de Bevy en quelques heures, pas en semaines.
2. **Une seule vérité pour les coordonnées.** Bevy et Godot ont le même repère (Y vers le haut, main droite,
   avant = −Z, mètres) : aucune conversion entre l'éditeur, le jeu actuel et le futur jeu.
3. **Les fichiers de carte sont des données, jamais du code** (voir « Menaces »).
4. **Mesurer avant d'optimiser.** Toute optimisation part d'un scénario reproductible (`mcf/bench/`) et se
   termine par le chiffre avant / après.
5. **Mid poly bien fait > high poly mal géré.** Budgets par catégorie, LOD systématiques, maillages et matériaux
   partagés (c'est ce qui permet à Bevy de regrouper les objets automatiquement).

---

## Phase 0 — Fondations ✅

- Dépôt privé `MCF-Studio/MCF-Engine`, branche `main` partie du tag `v0.20.1`, remote `upstream` =
  `bevyengine/bevy` (historique complet pour les fusions).
- CI de Bevy retirée (`.github/workflows`, `dependabot.yml`, `FUNDING.yml`) : énorme, et payante en dépôt privé.
- Présentation : `.github/README.md` (le `README.md` de Bevy reste intact). Règles IA : `CLAUDE.md` / `AGENTS.md`.

**Mise à jour vers une nouvelle version de Bevy** (à scripter en phase 1) :

```sh
git fetch upstream --tags
git switch -c sync/bevy-0.21 main
git merge v0.21.0
# conflits « modify/delete » sur la CI amont : on garde notre suppression
git rm -r --cached .github/workflows .github/dependabot.yml .github/FUNDING.yml 2>/dev/null
cargo check --workspace && cargo test -p mcf_*   # puis PR vers main
```

---

## Phase 1 — Squelette MCF ✅

| Livrable | Détail |
|---|---|
| `mcf/crates/mcf_core` | Plugins communs jeu + éditeur + serveur (catalogue, calques, identifiants stables) |
| `mcf/crates/mcf_map` | Format `.mcfmap` (lecture, écriture, **validation stricte**), sans dépendance au rendu |
| `mcf/crates/mcf_bench` | Diagnostics (temps CPU/GPU par image, 1 % low, mémoire, appels de dessin), export CSV, parcours caméra rejouable |
| `mcf/crates/mcf_editor`, `mcf_server`, `mcf_client` | Binaires (vides au départ) |
| CI `mcf-ci.yml` | `fmt`, `clippy -D warnings`, tests `mcf_*`, `cargo deny`, build Windows, **seulement** nos crates |
| Builds rapides | `opt-level` 1 pour nous / 3 pour les dépendances, `bevy/dynamic_linking`, linker `rust-lld`, essai Cranelift |

Critère de fin : `cargo run -p mcf_editor` ouvre une fenêtre, la CI est verte, une recompilation après
modification d'un fichier de `mcf_editor` prend **< 10 s** sur le PC d'Aroun (chiffre noté ici).

**État (2026-11-18)** :
- ✅ Workspace `mcf/`, crates `mcf_map` / `mcf_core` / `mcf_bench` / `mcf_editor` / `mcf_client` / `mcf_server`.
- ✅ `.mcfmap` porté de l'éditeur Godot à l'identique (mêmes champs, bornes, messages) ; 32 tests au total.
- ✅ CI verte (PR #1) : format, clippy `-D warnings`, tests Linux, `cargo deny`.
- ✅ Serveur headless : 59,1 ticks/s mesurés pour 60 visés (première fenêtre de 10 s, démarrage compris).
- ✅ Recompilation en mode dev (Bevy en bibliothèque dynamique) : **≈ 1,8 s** après modification du contenu de
  `mcf_editor/src/main.rs` (3 essais : 1,88 / 1,80 / 1,80 s, `-j 6`). Première recompilation après le build
  complet : 11,2 s. Build complet initial : 11 min 45 s, réussi après redémarrage du PC (17 Go de mémoire
  virtuelle libre, contre ~9 Go lors des deux échecs précédents).
- ✅ `cargo editor` ouvre la fenêtre « MCF Editor » (RTX 3060 Ti, Vulkan). Erreurs Vulkan sans conséquence au
  démarrage : couche Vulkan de TikTok LIVE Studio déclarée mais introuvable sur le disque.
- ✅ Scène de calibrage exécutée (`cargo bench-scene`, réglages par défaut, commit `56de9fb11`, 2026-11-18) :
  100 000 objets (20 modèles, 8 matériaux), 200 lampes sans ombre, zone de 2 km, soleil sans ombre, 1920×1080,
  sans vsync, 40 s de parcours après 5 s de chauffe. Ryzen 7 3700X, 16 Go, RTX 3060 Ti (pilote 610.88, Vulkan).
  | Mesure | Moyenne | p99 | Pic |
  |---|---|---|---|
  | Image (ms) | 5,59 (179 FPS) | 9,06 | 13,4 |
  | Rendu GPU (ms) | 3,83 | 7,54 | 8,27 |
  | Triangles | 21,9 M | 40,7 M | 41,0 M |
  | Mémoire du processus (Gio) | 0,68 | 0,69 | 0,69 |
  1 % low : 103,7 FPS ; 0 image au-delà de 33 ms. Build `bench-release` complet : 11 min 55 s (`-j 6`).
  Lecture honnête : ordre de grandeur seulement (formes simples, **aucun LOD**, d'où jusqu'à 41 M triangles).
  Le « rendu CPU » relevé (0,07 ms) ne couvre que l'encodage des passes de rendu, pas tout le monde de rendu :
  il ne faut pas le lire comme le coût CPU du rendu. CPU système jusqu'à 97 % pendant la mesure : d'autres
  programmes tournaient, à refaire machine au repos pour le test A. Appels de dessin non exposés par Bevy 0.20.1.
- ✅ PR #1 fusionnée dans `main` (`25ea93547`) ; CI de `main` verte, **Build Windows compris** (exécution
  44341704135). Critère de fin vérifié : phase 1 terminée le 2026-11-18.
- Constaté : erreur Windows 1455 (« fichier de pagination insuffisant ») avec 16 compilations en parallèle →
  infos de débogage de Bevy réduites dans le profil `dev`, et `-j 6` conseillé sur 16 Go.

---

## Phase 2 — 🎯 Bancs d'essai décisifs

Protocole complet, scènes, paliers et seuils : [`mcf/docs/bancs-essai.md`](mcf/docs/bancs-essai.md).

- **Test A — grosse carte** : île générée de 4 × 4 km (jusqu'à 8 × 8), 800 bâtiments, 80 000 objets,
  30 000 arbres, 2 000 lampes, 100 personnages animés ; parcours caméra rejoué ; même scène sous Godot 4.7.
  On y prototype déjà la **génération de LOD**, le **HLOD par cellule**, l'**occlusion culling** et un
  **streaming** simple, car ce sont eux qui feront la différence.
- **Test B — réseau** : serveur dédié headless + 32 → 256 bots, réseau dégradé simulé ; `lightyear` contre
  `bevy_replicon` + `renet` ; comparaison avec le réseau actuel du jeu Godot.

**État (2026-11-18)** : générateur déterministe de la grosse carte (`cargo big-map`) fait et testé, détail dans
`bancs-essai.md`. Suivant : terrain, chargeur Bevy des calques + modèles mid poly, parcours caméra du test A.
À fixer par Aroun : la machine de référence (et une config « joueur moyen » si disponible).

Critère de fin : les deux rapports sont écrits dans `bancs-essai.md` (bruts + conclusion), et Aroun prend la
**décision** : continuer, réorienter ou arrêter.

---

## Phase 3 — Éditeur MCF (la vraie interface)

Bevy n'a **pas d'éditeur** (l'équipe Bevy en prépare un, pas prêt). On construit le nôtre, utilisable par des
personnes **sans connaissance technique** pour la carte, et assez complet pour les devs.

**Outillage IA** (demande d'Aroun, 2026-11-24) : un **MCP `mcf-engine`** qui donne accès au moteur en cours
d'exécution (état exact, capture d'une vue, caméra, commandes annulables de l'éditeur, mesures), sur le Bevy Remote
Protocol en local avec un jeton, feature `remote` absente des builds distribués ; et un **LSP** (rust-analyzer limité
à `mcf/` tout de suite, puis un `mcf_lsp` pour les fichiers de carte). Spec, permissions et menaces :
[`docs/SPEC_MCP_LSP_MOTEUR.md`](docs/SPEC_MCP_LSP_MOTEUR.md). Prévu, rien de codé.

### Technique retenue

- Interface : **`bevy_egui` + `egui_dock`** (panneaux ancrables, onglets, mise en page sauvegardée), versions
  compatibles Bevy 0.20 et entre elles (egui 0.36, vérifié le 2026-11-18). Inspecteur générique inspiré de
  `bevy-inspector-egui` (réflexion de Bevy : tout composant enregistré est éditable sans code d'UI).
- L'éditeur fait tourner **le vrai moteur** dans la même fenêtre : ce qu'on voit est ce que le jeu affiche.
- L'interface **du jeu** n'utilise pas egui : `bevy_ui`, puis `.uimcf` (phase 8).

### Modules

| Module | Contenu |
|---|---|
| **Scènes** | Arborescence des entités, inspecteur de composants, **modèles d'objets** réutilisables (prefabs) avec surcharges, onglets multi-scènes, navigateur d'assets avec vignettes. Enregistrement au format réflexion de Bevy (RON) aujourd'hui, **`.bsn`** quand Bevy publiera le format de fichier |
| **Scripts** | Le gameplay est en Rust : chaque comportement est un composant enregistré, **ajouté et réglé depuis l'inspecteur**. Bouton « ouvrir dans VS Code ». Rechargement du code à chaud (feature `hotpatching` de Bevy, expérimentale : à valider). Plus tard, petits scripts `.mcf` pour les non-devs (phase 8) |
| **Animations** | Visionneuse de clips avec timeline (lecture, défilement image par image), éditeur de **graphe d'animation** (`AnimationGraph` de Bevy : mélanges, transitions), événements d'animation (pas, son), animations simples créées dans l'éditeur (portes, barrières, ascenseurs) |
| **Carte** | Façon Eden Editor, cahier des charges repris de `MCF-Map-Editor/EDITEUR_DE_CARTE.md`, format `.mcfmap` v1 conservé : caméra libre + carte 2D, catalogue avec vignettes et recherche, poser collé au sol, gizmo déplacer / tourner / échelle avec magnétisme, annuler / refaire illimité, calques, fiche de l'élément, sauvegarde auto, budget de performance affiché par calque |
| **Conception de carte 2D ↔ 3D** | Vue Plan dans l'éditeur (pas d'outil séparé, ADR-0001) : relief, réseau routier dessiné (7 classes), îlots et quartiers, bâti en blocs qui « pousse », exports plan stylisé / satellite / minimap. Noyau `mcf_geo` partagé, exactitude 2D = 3D testée. Spec et jalons J0–J8 : `docs/SPEC_CONCEPTION_CARTE_2D_3D.md`. Recoupe la phase 4 (terrain, routes) : les deux suivent cette spec |
| **Essayer** (Ctrl+P) | Jouer immédiatement la scène / la carte, puis revenir exactement à l'état d'édition (instantané du monde) |

**État conception 2D ↔ 3D (2026-11-23)** : `cargo editor` ouvre la vue Conception branchée sur `mcf_carte` (carte de
test par défaut, `cargo editor -- <dossier>` pour un projet ; un `.mcfmap` ouvre l'ancien éditeur de calque). Plan
orthographique + 3D sur les mêmes maillages, étapes Cadre / Relief (5 pinceaux) / Réseau (tracé, poignées, 7 classes) /
Quartiers (îlots, zones libres) / Bâti (lotir, déplacer, verrou, niveaux) / Export (aperçu, tuiles, minimap, graphe).
Lancé une fois sur le PC (scène reconstruite en 116 ms, mode dev) ; outils pas encore essayés un par un à la main, budgets
du §12 pas encore mesurés en `bench-release`.
2026-11-24 : vue 3D avec ciel physique (`Atmosphere`), soleil en lux réels et textures Poly Haven ; calques de référence
(J7, partie « calques image » : import PNG validé, calage par distance, déplacement, enregistrés). Compilés et testés,
**pas encore vus à l'écran**.
2026-11-24 : relevé assisté sur un calque calé (eau → relief creusé et niveau d'eau, taches → zones libres, routes →
tracés), chaque proposition validée ou rejetée à la main ; noyau `mcf_carte::releve` testé, essayé sur `map_1.png`
(détail dans `todo.md`). Corrigé : l'éditeur paniquait au démarrage (sonde du ciel de Bevy 0.20.1 sans vue 3D
active) ; il tourne maintenant, **interface toujours pas vue à l'écran**.

Critères de fin :
- une personne qui n'a jamais vu l'outil pose, déplace et enregistre 20 objets en moins de 10 minutes avec la
  seule aide intégrée (test fait avec un vrai contributeur) ;
- un dev crée un modèle d'objet « porte animée » (scène + comportement + animation) sans quitter l'éditeur,
  sauf pour écrire le code Rust du comportement.

---

## Phase 4 — Terrain, sol, routes, végétation

Bevy n'a **pas de terrain**.

- Heightmap en tuiles, LOD par **clipmap** (GPU), sculpture dans l'éditeur (monter, creuser, lisser, aplatir).
- Peinture du sol : splatmap (jusqu'à 16 matériaux Poly Haven), mélange par hauteur, triplanar sur les pentes.
- Collision dérivée de la heightmap : **le serveur charge la collision, pas les visuels**.
- Routes sur spline : tracé en quelques clics, terrain déformé sous la route, carrefours, marquages.
- Végétation : herbe en instancing GPU avec densité peinte, arbres en instances + imposteurs.

Critère de fin : l'île actuelle (heightmap exportée de Terrain3D) rechargée à la même échelle, mêmes routes,
avec les performances validées en phase 2.

---

## Phase 5 — Monde ouvert de production

Industrialiser ce que la phase 2 a prototypé (détail dans `rendu-lod-objets.md`) :

- **LOD automatiques** à l'import (simplification `meshopt`, LOD faits main prioritaires), composant `McfLod`
  qui pose seul les `VisibilityRange` selon la catégorie du catalogue.
- **Imposteurs** pour bâtiments et arbres lointains ; **HLOD par cellule** généré à l'enregistrement de la carte.
- **Streaming par cellules** (ex. 128 m) : chargement asynchrone, priorité selon distance et direction, budget
  mémoire.
- **Occlusion culling** activé là où il est mesuré utile (ville, intérieurs) ; portails pour les intérieurs.
- Budgets affichés dans l'éditeur, alerte si un calque les dépasse.

---

## Phase 6 — Réseau de production et serveur dédié

La bibliothèque est choisie par le test B.

- Serveur **autoritaire** et headless (`MinimalPlugins`, aucune dépendance GPU), Docker, config par variables
  d'environnement, de 1 à N instances sans changement de code.
- **Interest management** par cellules : un client ne reçoit que ce qui est proche (bande passante, anti-triche,
  discrétion RP).
- Prédiction pour le joueur et les véhicules, interpolation pour les autres.
- Authentification avec les comptes existants (rp.mcfstudio.fr) par jeton court signé, révocable côté serveur.
- Persistance (personnages, inventaires, biens) en **PostgreSQL**, migrations versionnées, backups testés.

---

## Phase 7 — Briques RP

- Personnages : skinning GPU, **LOD d'animation** (mise à jour moins fréquente au loin), vêtements en couches,
  morph targets du visage.
- **Tissu des vêtements** : solveur commun avec MCF Character Studio, crate `mcf-tissu` (dépôt **local**
  `G:\MCF-Tissu`, pas de distant pour l'instant, décision d'Aroun du 2026-11-24 ; prendre au moins le tag `v0.1.1`, durci contre les données `tissu.v1` piégées). Intégration :
  `mcf-tissu = { git = "file:///G:/MCF-Tissu", tag = "…", features = ["glam"] }` (glam 0.33, comme Bevy 0.20.1),
  plugin Bevy mince `mcf_tissu_bevy` qui appelle `AnimatedCloth::step` (le même pas que le précalcul du Studio),
  contrat `tissu.v1` validé par `ClothAssetV1::validate()` de la crate. Coût mesuré par le Studio : ~3 µs par
  particule et par pas sur un cœur (jupe de 1 566 particules ≈ 4,5 ms) → LOD obligatoire (simulation près de la
  caméra, skinning seul au loin, repli en morph targets), plafond de tissus simulés à l'écran, rien côté serveur.
  À trancher à l'intégration : une dépendance `file://` ne passe pas dans la CI GitHub du moteur (vendoring ou
  dépôt distant le moment venu).
  **Profil jeu v1, fixé par le moteur** (Aroun, 2026-11-24 : « que ça suive les mouvements, bon rendu, pas de
  réalisme ») et imposé au Studio, qui prévisualise et exporte dans ce profil pour un rendu identique : 30 pas/s
  fixes (jamais plus, rendu interpolé), 4 sous-pas, auto-collision désactivée, ≤ 600 particules par vêtement et
  ≤ 1 000 par personnage, rayons de liberté ≤ 0,10 m (≤ 0,20 m pour les pans libres), corps collisionneur
  simplifié ≤ 1 500 triangles, sol activé. En jeu : 6 personnages simulés au plus, à moins de 12 m de la caméra,
  skinning seul au-delà ; budget total 2 ms par image sur un thread de travail. Un vêtement hors profil est refusé
  au chargement. Si le coût mesuré dépasse le budget, on baisse le plafond de particules, pas la fréquence.
  **Profil jeu v2** (2026-11-24, après mesure du Studio : ~1,85 µs par particule et par pas, et non 0,7) : la plupart
  des vêtements ne sont **pas simulés** (skinning pur : casquette, haut, pantalon, chaussures) ; seuls les pans libres
  (jupe, robe, cape, manteau long) le sont, **un seul par personnage**, via un **maillage de simulation allégé
  ≤ 350 particules** qui pilote le maillage de rendu (lien calculé dans `mcf-tissu`, identique Studio / moteur).
  6 × 350 × 30 × 1,85 µs ≈ 1,95 ms par image. Attendu : `mcf-tissu` v0.3.0 (`GAME_PROFILE_V2`).
  **Disponible** (2026-11-24) : `mcf-tissu` v0.4.0 (minimum relevé ensuite à v0.6.0) (collisions après la flexion : la robe ne
  rentre plus dans le corps, résultats différents des 0.3.x ; ~1,3 µs par particule et par pas). Le proxy s'applique
  au rendu skinné par `render_offsets` (v0.3.1) : skinning au GPU, seuls les décalages sont transmis.
  **Capsules de repoussement** (feu vert d'Aroun, 2026-11-24) : contre les traversées restantes du rendu (ourlet /
  tibia ~31 mm, avant-bras / flancs ~35 mm), les sommets de rendu sont repoussés hors de capsules des membres
  (≤ 16, 8 visées), **dans le vertex shader** après skinning + décalages ; le CPU ne pose que les extrémités depuis
  les os. Fonction CPU de référence dans `mcf-tissu` (Studio, tests ; 45 ns par sommet avec 8 capsules sur un cœur,
  trop cher pour le jeu), test de concordance WGSL ↔ référence avec tolérance. Capsules dans un bloc versionné du
  contrat, validées strictement ; vêtement sans capsules valide ; correctif visuel seulement, appliqué aussi aux
  personnages seulement skinnés. Consigne d'Aroun : tout ce qui peut tourner sur la carte graphique y tourne, rendu
  semi-réaliste, 60 à 80 joueurs sur une carte.
  **Livré** : `mcf-tissu` v0.5.0 (contrat `tissu.capsules.v1`, bloc `skins[0].extras.mcfTissuCapsules` du `.glb`,
  0 à 16 capsules, spécification à porter en WGSL dans `docs/CAPSULES.md` du dépôt, jeu de référence à 1e-5 m ;
  2 × vec4 par capsule, 256 octets par personnage pour 8). Mesuré par le Studio sur la robe : traversées tibia
  31 → 5,3 mm, avant-bras 35 → 5,6 mm. Côté moteur : à porter en phase 7. Les capsules s'appliquent à tous les
  maillages du skin 0 (vêtements simulés ou skinnés), **sauf** les nœuds marqués `extras.mcfTissuRole == "body"`
  (le corps, que le Studio marque à l'export). Le Studio en écrit 11 (membres, mains, une capsule de torse
  horizontale) : nombre et ordre lus dans le bloc, jamais supposés. Robe sur la marche : pire traversée 25,6 → 5,2 mm.
  **Coutures** qui s'ouvraient (jusqu'à 2 cm) : corrigé dans `mcf-tissu` **v0.6.0, minimum à prendre** (refermées
  après les collisions : 0 ouverte). **Tache claire sur le devant des robes** : la cuisse avant traverse la robe
  (vérifié à l'écran par le Studio, poids d'os), pas la main ni les coutures ; le proxy (espacement 6,3 cm) laisse passer la
  cuisse entre ses particules. Corrigé par une **marge de collision du proxy** = espacement² / (8 × rayon de cuisse
  minimal), ≤ 15 mm, **seulement pour les vêtements dont le proxy est traversé** (robe : 8,2 mm ; proxy traversé
  5 393 → 0, tache disparue à l'écran ; coût en jeu inchangé). Champ facultatif `proxy.collisionMarginM` du bloc
  tissu.v2, validé strictement (borne haute : rayon de membre minimal 4 cm). **Minimum à prendre : `mcf-tissu`
  v0.7.0.** Reste à regarder : jupe traversée de 42 mm par la cuisse en course. La décision d'Aroun « bras écartés » (2026-11-24) reste en réserve, sans besoin
  mesuré ; mesure prête (`min_arm_abduction_deg`).
  À trancher en phase 7 : les décalages du proxy déplacent les positions, pas les normales (normales du skinning →
  éclairage faux là où le tissu s'écarte) ; décalage de normale par sommet ou normales recalculées.
- Véhicules : physique (`avian3d` ou `rapier`, tous deux compatibles 0.20), véhicule à raycast, réplication.
- **Intérieurs visitables** : portes animées, portails d'occlusion, lumières intérieures sans ombres par défaut.
- Cycle jour / nuit : atmosphère de Bevy, budget de lampes à ombre la nuit.
- Audio : `bevy_audio` est trop simple → `bevy_kira_audio` (compatible 0.20) ou équivalent (bus, réverbération,
  spatialisation) ; **chat vocal de proximité** (Opus, spatialisé), absent de Bevy, indispensable au RP.

---

## Phase 8 — 🧪 La petite folie : lire du `.mcf`

**Garé** tant que le compilateur (`MCF-Studio/MCF-MyLangage`) n'est pas abouti. Usage visé : les **interfaces du
jeu** en `.uimcf` (HUD, téléphone, inventaire, menus), puis de petits scripts `.mcf`. Conditions, intégration et
vigilance (dont la **licence GPL-3.0**) : [`mcf/lang/README.md`](mcf/lang/README.md).

---

## Phase 9 — Production

- Pipeline d'assets : traitement d'assets de Bevy → textures **KTX2/Basis**, LOD générés, fichiers `.meta`
  versionnés ; export Blender en glTF selon les règles du studio.
- Packaging Windows (serveur Linux), intégration au `mcf-launcher`, mises à jour différentielles.
- Rapports de crash (avec consentement), logs structurés côté serveur, métriques, alertes.

---

## Phase 10 — Transition du jeu

Préparée pendant les phases précédentes, lancée seulement quand le moteur est prêt :

- Import des cartes : les `.mcfmap` passent tels quels ; outil de conversion des scènes `.tscn` de l'île
  (positions identiques, même repère).
- Réimport des modèles (glTF depuis Blender) avec LOD, au budget mid poly.
- Portage du gameplay GDScript → Rust, système par système, en gardant le jeu Godot jouable jusqu'au bout.
- Bascule des serveurs quand la version MCF Engine fait au moins aussi bien sur les bancs d'essai.

---

## Ce qui mérite d'être optimisé (liste de travail)

Classé par impact estimé pour un RP 3D multijoueur mid poly. Chaque ligne = un ticket avec sa mesure.

| # | Sujet | État dans Bevy 0.20.1 | Pourquoi c'est critique | Piste |
|---|---|---|---|---|
| 1 | Grosse carte : streaming | Chargement asynchrone seulement | Une île entière ne tient pas en mémoire GPU | Cellules + budgets (phases 2, 5) |
| 2 | Réseau / réplication | Absent | Le RP est multijoueur | `lightyear` ou `replicon` + interest management (phases 2, 6) |
| 3 | LOD | `VisibilityRange` manuel, pas de génération, pas propagé aux enfants | Ville = des dizaines de milliers d'objets | LOD auto à l'import, `McfLod`, imposteurs |
| 4 | HLOD | Possible, aucun outil | Réduire les appels de dessin au loin | Fusion par cellule à l'enregistrement |
| 5 | Occlusion culling | Existe, désactivé par défaut, incompatible rendu différé | Ville dense, intérieurs | Activer et mesurer ; rester en rendu *forward* |
| 6 | Éditeur | Absent | Personne d'autre qu'un dev ne construit le jeu | Phase 3 |
| 7 | Terrain | Absent | Toute l'île en dépend | Clipmap GPU (phase 4) |
| 8 | Physique | Absente | Personnages, véhicules, collisions serveur | `avian3d` / `rapier` |
| 9 | Chat vocal spatial | Absent | Cœur du RP | Opus + spatialisation |
| 10 | Compression glTF | Pas de meshopt / Draco / quantization | Taille du jeu, temps de chargement | Pré-traitement d'assets dans notre format |
| 11 | Textures | KTX2/Basis disponibles, pas branchés par défaut | Mémoire GPU, chargement | `compressed_image_saver` dans le pipeline |
| 12 | Animation de foule | Correcte, sans LOD d'animation | Places publiques bondées | LOD d'animation, mises à jour échelonnées |
| 13 | Lampes la nuit | Clustered forward OK, ombres coûteuses | Ville éclairée | Lampes sans ombre par défaut, `ShadowLodOrigin` |
| 14 | Temps de compilation | Longs | Productivité quotidienne | Dynamic linking, lld, Cranelift, crates découpées |
| 15 | Audio | Basique | Ambiances, véhicules, intérieurs | `bevy_kira_audio` ou équivalent |
| 16 | UI du jeu | `bevy_ui` fonctionnel mais verbeux | Téléphone, inventaire, HUD | Composants maison, puis `.uimcf` |
| 17 | Serveur headless | Possible (`MinimalPlugins`) | Coût d'hébergement, nb de joueurs | Collision seule côté serveur |
| 18 | Meshlets (façon Nanite) | Expérimental, coût de base élevé | — | **Écarté** : pensé pour le très haute densité, pas le mid poly |

---

## Capacités moteur exigées par le jeu

Le game design vit dans [`docs/jeu/`](docs/jeu/README.md), **séparé** du moteur. Ici, seulement ce que le moteur
doit savoir faire pour lui, sans gameplay. Ces points sont à prévoir dans les phases indiquées, pas avant.

| Capacité | Phase | Exigée par |
| --- | --- | --- |
| Plusieurs zones par serveur (repère local chacune), transfert de joueurs / véhicules décidé par le serveur | 6 | [Centre spatial](docs/jeu/CENTRE_SPATIAL.md) |
| Ciel à l'échelle (*scaled space*) : caméra de fond pour objets célestes lointains | 7 | Centre spatial |
| Planète vue de l'espace (sphère + atmosphère vue de l'extérieur + nuages), depuis l'export satellite de la carte | banc à prévoir | Centre spatial |
| Gravité par zone, contrôleur de personnage en apesanteur | 7 | Centre spatial |
| Plafond d'altitude par type de véhicule, vérifié par le serveur | 7 | Centre spatial |
| Trajectoires mises en scène (véhicule sur chemin serveur, passagers à bord) | 7 | Centre spatial |

---

## Modèle de permissions (qui voit / fait quoi)

| Acteur | Peut | Ne peut pas |
|---|---|---|
| Aroun (admin) | Tout : code, intégrer un calque, publier | — |
| Dev (Joaquine…) | Code, éditeur complet, branches + PR | Pousser sur `main` sans PR, publier |
| Contributeur de carte (personne de confiance) | Utiliser l'**exécutable** de l'éditeur en mode carte, créer / ouvrir des `.mcfmap`, les envoyer à Aroun | Accéder au code, au serveur, aux comptes ; intégrer lui-même un calque |
| Joueur | Lancer le client, jouer | Recevoir les données d'autres joueurs hors de sa zone (interest management) |
| Serveur de jeu | Lire / écrire la base de son instance | Accéder aux autres instances (une base / un rôle SQL par serveur) |

L'éditeur **ne se connecte jamais** à un serveur de production : il ne travaille que sur des fichiers locaux.

## Menaces principales et mesures

| Menace | Mesure |
|---|---|
| Fichier `.mcfmap` piégé ou abîmé (code, chemins `../`, valeurs extrêmes) | Format de **données** uniquement ; validation stricte à l'import (schéma, version, identifiants du catalogue connus, bornes de position / échelle, taille max) ; aucune référence de fichier libre |
| Fichier de scène (RON / `.bsn`) piégé | Seuls les types enregistrés et autorisés sont désérialisés ; scènes externes validées comme les `.mcfmap` |
| Script `.mcf` / `.uimcf` malveillant (phase 8) | Bac à sable : aucune API fichier / réseau / processus par défaut, capacités explicites, limites de temps et mémoire |
| Client modifié (triche) | Serveur autoritaire, validation de chaque action côté serveur, interest management |
| Vol de session joueur | Jetons courts signés + renouvellement, révocables côté serveur, transport chiffré |
| Dépendance Rust compromise | `cargo deny` (failles + licences) en CI, `Cargo.lock` versionné, revue avant ajout de crate |
| Fuite de secrets | Aucun secret dans le dépôt ; variables d'environnement et gestionnaire de secrets côté serveur |
| Divergence avec Bevy amont (dette) | Code isolé dans `mcf/`, `[bevy-patch]` rares et documentés, synchro à chaque version |
