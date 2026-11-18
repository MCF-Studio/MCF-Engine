# MCF Engine — feuille de route

> Moteur de MCF Studio, construit sur **Bevy 0.20.1** (MIT / Apache-2.0), orienté vers **un seul projet** : le jeu
> RP 3D de MCF Studio (île, ville, multijoueur sur serveur dédié).
> Une phase n'est terminée que lorsque son **critère de fin** est vérifié (test automatique ou démonstration
> enregistrée). Les cases ✅ ne sont cochées qu'avec la preuve associée.

| Phase | Objectif | État |
|---|---|---|
| 0 | Fondations : fork, dépôt, règles, synchro amont | ✅ fork 0.20.1, dépôt privé, CI amont retirée |
| 1 | Squelette MCF (crates, CI légère, builds rapides) | ⬜ |
| 2 | **Éditeur de carte v1** (interface graphique, façon Eden Editor) | ⬜ |
| 3 | Terrain, peinture du sol, routes | ⬜ |
| 4 | Monde ouvert : streaming, LOD, occlusion | ⬜ |
| 5 | Réseau et serveur dédié | ⬜ |
| 6 | Briques RP (personnages, véhicules, intérieurs, voix) | ⬜ |
| 7 | 🧪 La petite folie : lecture du langage `.mcf` / `.uimcf` | 🅿️ garé (voir `mcf/lang/`) |
| 8 | Production : pipeline d'assets, packaging, launcher | ⬜ |

**Jalon de décision (fin de phase 2 + prototype réseau de la phase 5)** : le jeu RP existe aujourd'hui sous Godot
(`MCF-2D-RP`, branche `prototype-3d`). Passer le jeu sur MCF Engine est un chantier lourd. On ne le décide
qu'après une **tranche verticale** : un quartier de l'île édité dans l'éditeur, 2 clients + 1 serveur dédié,
un personnage qui marche, une voiture, une porte. Si les chiffres (FPS, temps de chargement, bande passante,
temps de dev) ne sont pas meilleurs, on le dit et on arrête ou on réoriente.

---

## Principes

1. **On ne réécrit pas Bevy, on l'étend.** Notre code vit dans `mcf/` (crates séparées). On ne modifie les crates
   `crates/bevy_*` que pour une optimisation mesurée, en commit isolé, préfixé `[bevy-patch]`, et si possible
   proposée en amont. Objectif : pouvoir suivre les versions de Bevy (0.21, 0.22…) en quelques heures, pas en
   semaines.
2. **Une seule vérité pour les coordonnées.** Bevy et Godot ont le même repère (Y vers le haut, main droite,
   avant = −Z, mètres). Une position de l'éditeur = la position dans le jeu, sans conversion.
3. **Les fichiers de carte sont des données, jamais du code.** Rien de ce qu'un contributeur envoie ne peut
   exécuter quoi que ce soit (voir « Menaces »).
4. **Mesurer avant d'optimiser.** Chaque optimisation de la liste ci-dessous démarre par un scénario de mesure
   reproductible (`mcf/bench/`), et se termine par le chiffre avant / après.

---

## Phase 0 — Fondations ✅

- Dépôt privé `MCF-Studio/MCF-Engine`, branche `main` partie du tag `v0.20.1`.
- Remote `upstream` = `bevyengine/bevy` (historique complet conservé pour les fusions).
- CI de Bevy retirée (`.github/workflows`, `dependabot.yml`, `FUNDING.yml`) : elle est énorme et consommerait
  les minutes Actions d'un dépôt privé. Notre CI arrive en phase 1.
- Présentation du dépôt : `.github/README.md` (le `README.md` de Bevy reste intact pour éviter les conflits).
- Règles pour les IA : `CLAUDE.md` / `AGENTS.md`.

**Mise à jour vers une nouvelle version de Bevy** (procédure, à scripter en phase 1) :

```sh
git fetch upstream --tags
git switch -c sync/bevy-0.21 main
git merge v0.21.0
# conflits « modify/delete » sur .github/workflows : on garde notre suppression
git rm -r --cached .github/workflows .github/dependabot.yml .github/FUNDING.yml 2>/dev/null
cargo check --workspace && cargo test -p mcf_*   # puis PR vers main
```

---

## Phase 1 — Squelette MCF

| Livrable | Détail |
|---|---|
| `mcf/crates/mcf_core` | Plugins communs jeu + éditeur + serveur (unités, calques, catalogue, identifiants stables) |
| `mcf/crates/mcf_map` | Format `.mcfmap` (lecture, écriture, **validation stricte**), sans dépendance au rendu |
| `mcf/crates/mcf_editor` | Binaire de l'éditeur (phase 2) |
| `mcf/crates/mcf_server` | Binaire serveur dédié headless (phase 5) |
| `mcf/crates/mcf_client` | Binaire du jeu |
| CI `.github/workflows/mcf-ci.yml` | `cargo fmt`, `clippy -D warnings`, tests `mcf_*`, `cargo deny` (licences + failles), build Windows ; cache `Swatinem/rust-cache` ; **seulement** sur nos crates |
| Builds rapides en dev | profil `dev` avec `opt-level = 1` pour notre code et `3` pour les dépendances, feature `bevy/dynamic_linking`, linker `rust-lld`, option backend Cranelift |

Critère de fin : `cargo run -p mcf_editor` ouvre une fenêtre vide, la CI est verte, une recompilation après
modification d'un fichier de `mcf_editor` prend **< 10 s** sur le PC d'Aroun (chiffre mesuré et noté ici).

---

## Phase 2 — Éditeur de carte v1 (l'interface graphique)

Bevy n'a **pas d'éditeur**. L'équipe Bevy en prépare un (widgets `bevy_feathers`, inspection dans
`bevy_dev_tools`), mais il n'est pas prêt. On construit le nôtre, pensé pour des personnes **sans connaissance
technique**, sur le modèle d'ergonomie déjà spécifié dans `MCF-Map-Editor/EDITEUR_DE_CARTE.md` (Eden Editor
d'Arma 3). On **reprend ce cahier des charges et son format `.mcfmap` v1** pour que le travail déjà fait reste
valable.

### Choix de l'interface

| Option | Pour | Contre |
|---|---|---|
| **`bevy_egui` + `egui_dock`** (recommandé pour l'éditeur) | Mature, panneaux ancrables, champs, arbres, recherche : très rapide à produire et intuitif | Look « outil », pas celui du jeu |
| `bevy_feathers` (officiel Bevy) | Futur éditeur officiel, même techno que le jeu | Encore jeune, beaucoup à construire nous-mêmes |

Recommandation : **egui pour l'éditeur**, `bevy_ui` (puis `.uimcf`, phase 7) pour l'interface **du jeu**.
À valider par Aroun avant la phase 2.

### Fonctions v1 (dans l'ordre de développement)

1. **Caméra libre** 3D (ZQSD/AZERTY + souris, Maj = rapide) et **carte 2D** vue de dessus (touche M), même outils.
2. **Catalogue** (panneau de droite) : catégories, sous-catégories, recherche, **vignettes générées
   automatiquement** (rendu hors écran de chaque modèle, mises en cache).
3. **Poser** par glisser-déposer, **collé au sol** (raycast), option « aligner sur la pente ».
4. **Sélectionner** au clic (`bevy_picking`), rectangle de sélection, **gizmo** déplacer / tourner / échelle,
   magnétisme (grille, angle), saisie numérique dans la fiche.
5. **Annuler / refaire** illimité (journal de commandes), copier / coller / dupliquer.
6. **Calques** (panneau de gauche) : afficher / masquer, verrouiller, un calque = un fichier `.mcfmap`.
7. **Fiche de l'élément** : nom, position, rotation, réglages **déclarés sûrs** par le catalogue (couleur,
   lumière allumée la nuit, porte verrouillée…).
8. **Enregistrer / ouvrir** `.mcfmap`, sauvegarde automatique toutes les 2 min + récupération après crash.
9. **Essayer** (Ctrl+P) : se promener à pied dans la carte, puis revenir à l'édition exactement où on était.
10. Barre d'état : coordonnées, pas de grille, nombre d'objets, budget de performance du calque (voir phase 4).

Critère de fin : une personne qui n'a jamais vu l'outil pose, déplace et enregistre 20 objets en moins de
10 minutes avec seulement la page d'aide intégrée (test fait avec un vrai contributeur, noté ici).

---

## Phase 3 — Terrain, sol, routes

Bevy n'a **pas de terrain**. À faire (ou reprendre une crate maintenue après revue) :

- Terrain par **heightmap en tuiles** avec LOD par **clipmap** (GPU), sculpture (monter, creuser, lisser,
  aplatir) dans l'éditeur.
- **Peinture du sol** : splatmap (jusqu'à 16 matériaux), matériaux PBR Poly Haven (bibliothèque locale), mélange
  par hauteur, triplanar sur les pentes.
- Collision dérivée de la heightmap (`heightfield` du moteur physique) : **le serveur charge la collision, pas
  les visuels**.
- **Routes** sur spline : tracé en quelques clics, déformation du terrain sous la route, carrefours simples,
  marquages.
- Végétation : herbe en instancing GPU avec densité peinte, arbres en instances + imposteurs au loin.

Critère de fin : l'île actuelle (heightmap exportée depuis Terrain3D) est rechargée dans MCF Engine à la même
échelle, avec les mêmes routes, à ≥ 60 FPS sur la config cible.

---

## Phase 4 — Monde ouvert

- **Streaming par cellules** (ex. 128 m) : chargement / déchargement asynchrone des calques, priorités selon la
  distance et la direction de la caméra, budget mémoire.
- **HLOD** : `VisibilityRange` de Bevy (fondu entre niveaux) + génération automatique des LOD à l'import
  (meshopt) ; imposteurs pour les bâtiments lointains.
- **Occlusion culling** GPU (expérimental dans Bevy) à fiabiliser : capital en ville et dans les intérieurs.
- Budgets affichés dans l'éditeur par calque : triangles, draw calls, matériaux, lumières avec ombres.

---

## Phase 5 — Réseau et serveur dédié

Bevy n'a **pas de réseau**. Choix à faire par prototype comparatif : `lightyear` (prédiction, interpolation,
interest management intégrés) ou `bevy_replicon` + transport `renet`/QUIC.

- Serveur **autoritaire** et headless (`MinimalPlugins`, aucune dépendance GPU), conteneurisé (Docker), config
  par variables d'environnement.
- **Interest management** par cellules : un client ne reçoit que ce qui est proche de lui (bande passante,
  anti-triche, discrétion RP).
- Prédiction côté client pour le joueur et les véhicules, interpolation pour les autres.
- Authentification avec les comptes existants (rp.mcfstudio.fr) par jeton court signé, jamais de mot de passe
  dans le client.
- Persistance (personnages, inventaires, biens) en **PostgreSQL** côté serveur, migrations versionnées.

Critère de fin : 1 serveur + 50 clients simulés (bots headless) sur l'île, mesure de la bande passante par
client et du temps de tick serveur, notés ici.

---

## Phase 6 — Briques RP

- Personnages : skinning GPU, **LOD d'animation** (mise à jour moins fréquente au loin), personnalisation
  (vêtements en couches, morph targets du visage).
- Véhicules : physique (`avian3d` ou `rapier`, à trancher en phase 5), véhicule à raycast, réplication.
- **Intérieurs visitables** : portes animées, portails d'occlusion, lumières intérieures sans ombres par défaut.
- Cycle jour / nuit : atmosphère de Bevy, nombre de lumières à ombre limité la nuit en ville.
- Audio : `bevy_audio` est trop simple → crate plus complète (bus, réverbération, spatialisation) ;
  **chat vocal de proximité** (Opus, spatialisé) — absent de Bevy, indispensable au RP.

---

## Phase 7 — 🧪 La petite folie : lire du `.mcf`

**Garé dans un coin** tant que le compilateur (`MCF-Studio/MCF-MyLangage`) n'est pas abouti. Usage visé : écrire
**les interfaces du jeu** (HUD, téléphone, inventaire, menus) en `.uimcf`, et plus tard de petits scripts
`.mcf`. Détails, conditions de démarrage et points de vigilance (dont la **licence GPL-3.0** du compilateur) :
[`mcf/lang/README.md`](mcf/lang/README.md).

---

## Phase 8 — Production

- Pipeline d'assets : traitement d'assets de Bevy (`AssetProcessor`) → textures **KTX2/Basis**, maillages
  compressés, LOD générés, fichiers `.meta` versionnés ; export Blender en glTF selon les règles du studio.
- Packaging Windows (puis Linux serveur), intégration au `mcf-launcher`, mises à jour différentielles.
- Rapports de crash (avec consentement), logs structurés côté serveur, métriques et alertes.

---

## Ce qui mérite d'être optimisé (liste de travail)

Classé par impact estimé pour un RP 3D multijoueur. Chaque ligne = un ticket à ouvrir avec sa mesure.

| # | Sujet | État dans Bevy 0.20 | Pourquoi c'est critique pour nous | Piste |
|---|---|---|---|---|
| 1 | Éditeur de carte | Absent | Sans lui, personne d'autre qu'un dev ne construit la carte | Phase 2 |
| 2 | Réseau / réplication | Absent | Le RP est multijoueur par nature | Phase 5, crate tierce + interest management |
| 3 | Terrain | Absent | Toute l'île en dépend | Phase 3, clipmap GPU |
| 4 | Streaming de monde | Absent (chargement asynchrone seulement) | Une île entière ne tient pas en mémoire GPU | Cellules + budgets |
| 5 | LOD des maillages | `VisibilityRange` seulement, pas de génération | Ville = des milliers d'objets | LOD auto à l'import (meshopt), imposteurs |
| 6 | Occlusion culling | Expérimental | Ville dense, intérieurs | Fiabiliser, portails pour intérieurs |
| 7 | Physique | Absente | Personnages, véhicules, collisions serveur | `avian3d` / `rapier` |
| 8 | Chat vocal spatial | Absent | Cœur du RP | Opus + spatialisation audio |
| 9 | Audio | Basique | Ambiances ville, véhicules, intérieurs | Crate audio avancée |
| 10 | Temps de compilation | Longs | Productivité quotidienne | Dynamic linking, lld, Cranelift, découpage des crates |
| 11 | Animation de dizaines de personnages | Correct, sans LOD d'animation | Places publiques bondées | LOD d'animation, mise à jour échelonnée |
| 12 | Lumières la nuit | Clustered forward OK, ombres coûteuses | Ville éclairée | Budget d'ombres, lumières sans ombres par défaut |
| 13 | UI du jeu | `bevy_ui` fonctionnel mais verbeux | Téléphone, inventaire, HUD | Composants maison, puis `.uimcf` (phase 7) |
| 14 | Textures | PNG/JPG chargés tels quels par défaut | Mémoire GPU, temps de chargement | KTX2/Basis via le traitement d'assets |
| 15 | Serveur headless | Possible (`MinimalPlugins`) | Coût d'hébergement, nb de joueurs | Aucune dépendance rendu côté serveur, collision seule |
| 16 | Démarrage / chargement | Correct | Confort joueur | Pré-traitement des assets, chargement par priorité |

---

## Modèle de permissions (qui voit / fait quoi)

| Acteur | Peut | Ne peut pas |
|---|---|---|
| Aroun (admin) | Tout : code, intégrer un calque à l'île, publier | — |
| Contributeur de carte (personne de confiance) | Utiliser l'**exécutable** de l'éditeur, créer / ouvrir des `.mcfmap`, les envoyer à Aroun | Accéder au code, au serveur, aux comptes ; intégrer lui-même un calque |
| Joueur | Lancer le client, jouer | Voir les données d'autres joueurs hors de sa zone (interest management) |
| Serveur de jeu | Lire / écrire la base de son instance | Accéder aux autres instances (isolation : une base / un rôle SQL par serveur) |

L'éditeur **ne se connecte jamais** à un serveur de production : il travaille uniquement sur des fichiers locaux.

## Menaces principales et mesures

| Menace | Mesure |
|---|---|
| Fichier `.mcfmap` piégé ou abîmé (code, chemins `../`, valeurs extrêmes) | Format de **données** uniquement ; validation stricte à l'import (schéma, version, identifiants du catalogue connus, bornes de position / échelle, taille max) ; aucune référence de fichier libre |
| Script `.mcf` / `.uimcf` malveillant (phase 7) | Bac à sable : aucune API fichier / réseau / processus par défaut, capacités accordées explicitement, limite de temps et de mémoire |
| Client modifié (triche) | Serveur autoritaire, validation de chaque action côté serveur, interest management |
| Vol de session joueur | Jetons courts signés + renouvellement, révocables côté serveur, TLS / QUIC chiffré |
| Dépendance Rust compromise | `cargo deny` (failles + licences) en CI, `Cargo.lock` versionné, revue avant ajout de crate |
| Fuite de secrets | Aucun secret dans le dépôt ; variables d'environnement et gestionnaire de secrets côté serveur |
| Divergence avec Bevy amont (dette) | Code isolé dans `mcf/`, patches `[bevy-patch]` rares et documentés, synchro à chaque version |
