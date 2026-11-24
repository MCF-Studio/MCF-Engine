# Spécification — Conception de carte 2D ↔ 3D dans l'éditeur MCF

> Statut : **validée** par Aroun le 2026-11-22 (décisions au §13, ADR-0001). Implémentation : jalons du §11.
> Remplace l'idée d'un outil séparé « MCF-MapCreator » (Tauri 2 + Vue 3) : voir §1.
> Documents liés : `ARCHITECTURE_DONNEES_CARTE.md`, `CONVENTION_CARTE_2D_VERS_MCFMAP.md`,
> `MCF_Engine_Specification_Editeur_Scenes_Cartes.md`, jeu : `docs/05_MONDE_ET_VILLE.md`, ADR-008 (terrain).

## 0. Le besoin, en une phrase

Penser une vraie carte **vue de dessus** (relief, réseau routier hiérarchisé, quartiers, îlots, bâtiments) comme un
urbaniste, puis la voir et la retoucher **en 3D**, avec la garantie qu'un bâtiment posé en bord de route en 2D est
**au millimètre** en bord de route en 3D, et inversement.

## 1. Décision : un seul éditeur, pas deux outils

**Recommandation : intégrer la conception 2D dans `mcf_editor`**, sous forme d'une vue *Plan* et d'un déroulé de
travail guidé, au lieu d'un MapCreator Tauri/Vue séparé.

| Critère | Deux outils (Tauri/Vue + Bevy) | Un éditeur (Bevy) |
|---|---|---|
| Exactitude 2D = 3D | deux moteurs de géométrie (TS et Rust) à maintenir identiques : divergence quasi certaine à terme | **un seul noyau de géométrie Rust** ; la vue Plan est une caméra orthographique sur la *même* scène |
| Aller-retour | export → import → fusion des modifications faites des deux côtés, conflits à gérer | **aucun aller-retour** : les deux vues éditent le même document, en direct |
| Annuler / refaire | deux historiques | un historique (commandes structurées, invariant 5 de l'architecture) |
| Fluidité | changer d'application, recharger | un raccourci pour passer de Plan à 3D, ou vue partagée |
| Coût | une appli en plus à packager, sécuriser, maintenir | refonte de la vue 2D actuelle (§11, J1–J2) |

Ce qu'on garde de l'idée initiale :
- la **convention `.mcf2d.json`** et `mcf_map_import_2d` restent une **porte d'import** (croquis venus d'ailleurs) ;
- Tauri/Vue reste la pile des applis du studio, mais elle n'apporte rien ici. Si un jour il faut une **visionneuse
  web en lecture seule** des plans (wiki, équipe), elle consommera les exports d'image (§8), pas le document.

## 2. Le contrat d'exactitude (« pas de triche »)

1. **Une seule source de géométrie** : la crate `mcf_geo` (Rust pur, `f64`, sans Bevy, déterministe) calcule
   tout : axes de routes discrétisés, emprises (chaussée, trottoirs, accotements), carrefours, îlots, parcelles,
   empreintes de bâtiments, terrain final. Les vues ne font **que l'afficher**.
2. **La vue Plan n'est pas un dessin à part.** C'est une caméra orthographique verticale sur la même scène Bevy, avec
   un *style de rendu* « plan » (matériaux à aplats, calques de rendu). egui ne dessine plus de géométrie : seulement
   des poignées, des cotes et des étiquettes, placées par projection de positions monde.
3. **L'altitude ne déplace jamais le plan.** Le relief ne modifie que `y` ; `x` et `z` sont identiques en 2D et en
   3D. Les distances de conception (alignement, retraits, largeurs) sont des distances **horizontales**.
4. **Routes continues.** Un ruban unique par tronçon, avec des jonctions en onglet et des carrefours calculés comme des
   polygones. Les pavés par segment actuels (trous côté extérieur des virages, chevauchement côté intérieur) disparaissent.
5. **Preuve par les tests** : pour chaque bâtiment d'une carte de test, la distance entre sa façade et le bord de
   l'emprise de la route, mesurée sur le maillage 3D projeté, doit être égale à celle du noyau à **1 mm** près. Le
   test tourne en CI.

## 3. Déroulé de travail : une direction claire

L'éditeur propose six étapes dans l'ordre naturel de conception. Chacune a **sa barre d'outils courte** (2 à 5
outils). On peut revenir à une étape à tout moment : ce qui en dépend est recalculé, et le travail manuel est conservé (§6.4).

| Étape | On fait | Outils | Vue conseillée |
|---|---|---|---|
| 1. Cadre | créer la carte (emprise, nord), poser des **calques de référence** (croquis, image) | Nouvelle carte, Calque image | Plan |
| 2. Relief | collines, vallées, plateaux, littoral | Lever/Abaisser, Lisser, Aplanir à un niveau, Rampe, Relief naturel | Plan (courbes de niveau) + 3D |
| 3. Réseau | axes entre villes, avenues, rues, ruelles, chemins | Tracer (classe au choix), Éditer le tracé, Carrefour | Plan |
| 4. Quartiers | affecter les **îlots** (calculés) à une famille et une densité, zones libres | Affecter un îlot, Zone libre | Plan |
| 5. Bâti | faire **pousser les bâtiments** en blocs, ajuster | Lotir l'îlot, Déplacer/Redimensionner, Verrouiller | Plan + 3D |
| 6. Détail | objets du catalogue (calques `.mcfmap`), vérifier au sol | Catalogue, Transformations | 3D |

Puis **Exporter** : plan stylisé, vue satellite, données de jeu (§8, §9).

Raccourcis : `1`–`6` pour l'étape, `Tab` pour basculer entre Plan et 3D, `Maj+Tab` pour la vue partagée. Sélection,
déplacement, annulation et verrou fonctionnent à l'identique dans les deux vues (règle déjà en place).

## 4. Réseau routier

### 4.1 Modèle
Un **graphe** : des nœuds (extrémités, points de passage, carrefours) et des **tronçons**, chacun avec une classe et
une polyligne d'axe dont chaque sommet peut avoir un **rayon de courbure**. Le noyau discrétise les arcs avec un
écart de corde ≤ 5 cm, et cette même discrétisation sert aux deux vues.

- Tracer une route qui en croise ou en touche une autre crée le nœud automatiquement (accroche à 2 m, désactivable
  avec `Alt`).
- Un nœud de degré ≥ 3 devient un **carrefour explicite**, enregistré dans le fichier. Le runtime ne déduit rien,
  comme le veut la convention existante. Les types sont `carrefour` et `rond_point` (rayon d'îlot), avec des rayons
  de bordure par classe.

### 4.2 Classes et profils (valeurs **par défaut**, réglables par carte)
Largeurs « façade à façade » cohérentes avec le doc 05 du jeu (rue ≈ 14 m, avenue ≈ 24 m).

| Classe | Usage | Chaussée | Côtés | Emprise totale | Pente max | Couleur plan |
|---|---|---|---|---|---|---|
| `sentier` | piéton, parc | 1,5 m terre | — | 1,5 m | 25 % | pointillé brun |
| `chemin_terre` | campagne, champs | 3,5 m terre | 2 × 0,5 accotement | 4,5 m | 15 % | brun clair |
| `route` | interurbaine, 2 voies | 6,5 m | 2 × 1,5 accotement | 9,5 m | 8 % | jaune |
| `ruelle` | desserte urbaine | 5 m | 2 × 1,5 trottoir | 8 m | 12 % | blanc |
| `rue` | rue standard, 2 voies | 7 m | 2 × 3,5 trottoir | 14 m | 10 % | blanc bordé gris |
| `avenue` | axe urbain, 4 voies + stationnement | 19 m | 2 × 2,5 trottoir | 24 m | 6 % | orange |
| `voie_rapide` | **voie express** : liaison rapide, 2 × 2 voies sans séparation | 17 m | 2 × 2,5 accotement | 22 m | 5 % | rouge |
| `autoroute` | **autoroute** : 2 × 2 voies, bandes d'arrêt d'urgence, terre-plein central (TPC) | 2 × 10,5 m + 3 m TPC = 24 m | 2 × 3 accotement | 30 m | 5 % | bleu |

Le terre-plein central (`terre_plein_m` du profil, 0 par défaut) est compris dans la chaussée : une bande à part,
posée dessus, qui sépare les deux sens (muret béton en 3D, vert gris sur le plan). L'identifiant `voie_rapide` est
historique et reste stable dans les fichiers ; une carte enregistrée avant l'ajout de l'autoroute reçoit son profil
par défaut à l'ouverture.

Les profils sont des **données**, pas du code. La table ci-dessus est écrite dans le manifeste de chaque carte à sa
création ; on peut ensuite la corriger carte par carte (largeurs, pentes, couleurs), et tout ce qui en dépend est
régénéré. Les largeurs de voie suivent l'usage français : 3,5 m par voie, 2,5 m de stationnement, trottoirs de 1,5 à
3,5 m.

**Nouvelle convention, sans reprise de l'existant** : les classes v1 (`chemin` 2 m, `route` 4 m, `avenue` 6 m) et
l'import `.mcf2d.json` sont retirés dès J1. Les routes et carrefours quittent `.mcfmap` pour `reseau.json` au jalon
J3. Aucune carte réelle n'en dépend.

### 4.3 Une route se **dessine**, elle n'est pas un modèle 3D posé
On ne pose pas de morceaux de route modélisés à l'avance (droite, virage à 30°, croisement…). Ces kits obligent à
« tricher » sur les angles, collent mal au relief et ne suivent pas un tracé libre. On **trace** l'axe (clics, puis
virages arrondis par les rayons), et l'éditeur **génère** ce qui se voit à partir du tracé :

| Ce qu'on voit | Comment c'est produit |
|---|---|
| Chaussée, trottoirs, bordures | maillage **généré** le long du tracé, collé au terrain terrassé, avec bords nets et bordure de 15 cm |
| Marquages (lignes, passages piétons, flèches) | décalques générés depuis la classe et les carrefours, sans modèle |
| Chemins de terre, sentiers | **peints dans le matériau du terrain** (bords fondus, naturels) ; pas de maillage |
| Carrefours, ronds-points | polygones calculés, même procédé que la chaussée |

Pourquoi ne pas tout peindre dans le terrain ? Le terrain a une résolution de 1 m : une route asphaltée peinte
aurait des bords flous, pas de bordure en relief et des lignes blanches illisibles. La règle est donc la suivante :
**tracé dessiné → géométrie générée** pour le revêtu, **peinture au sol** pour la terre. Dans les deux cas, la
donnée source est uniquement le tracé et sa classe : modifier un virage régénère tout.

### 4.4 Routes et relief
Le profil en long d'un tronçon suit le terrain, lissé pour respecter la pente maximale de sa classe. Si la pente est
dépassée, la portion est signalée en rouge sur le plan. Sous l'emprise, le terrain final est **terrassé** : plat en
travers, raccordé par des talus à 1 pour 2. Les ponts et tunnels sont réservés pour plus tard : un tronçon pourra
porter un drapeau `ouvrage`.

## 5. Relief

- **Source** : tuiles de heightmap 16 bits de 256 m, à **1 m par pixel** (257 × 257 avec bord partagé). La plage
  d'altitude est déclarée dans le manifeste, par exemple −64 à +448 m, soit une précision de 7,8 mm. Pour 5 km, cela
  fait 20 × 20 tuiles.
- **Terrain final = dérivé**, calculé comme *source + terrassements des routes + plateformes de bâtiments*. On peut
  donc retracer une route sans détruire le relief sculpté dessous (invariant 3 de l'architecture). Le recalcul est
  **local** : il se limite aux tuiles touchées.
- **Lecture en vue Plan** : teintes hypsométriques, courbes de niveau (5 m, maîtresses à 25 m), estompage. Le tout
  est calculé dans le shader du maillage de terrain, sans image séparée.
- **Eau** : un niveau de mer ou de lac global en v1, et des zones `eau`. Les rivières suivront la spec de l'eau
  (§8 de la spec de l'éditeur).
- **Trous** : le masque de trous est réservé pour les entrées de souterrains (ADR-008) ; il n'est pas édité en v1.

## 6. Quartiers, îlots et bâtiments

### 6.1 Îlots (automatiques)
Un **îlot** est une face fermée du graphe routier, moins les emprises. Il est recalculé quand le réseau change et
garde un identifiant stable tant que sa forme évolue peu (appariement par recouvrement > 50 %).

### 6.2 Zonage
Il suffit de cliquer sur un îlot pour lui affecter une **famille** et une **densité** (faible, moyenne, forte, et
`centre` pour le Downtown). Familles et teintes du plan : résidentiel (vert pâle), commerce (bleu), activité
(violet), services publics (rouge pâle), nature / loisirs (vert, pas de bâti), eau (bleu foncé, pas de bâti).

**Gabarits par défaut**, réglables par carte dans le manifeste comme les profils routiers. Ils sont calés sur le doc
05 du jeu (Downtown façon Manhattan, couronne façon Île-de-France). R+n signifie rez-de-chaussée + n étages.

| Famille · densité | Exemple | Niveaux | Façade | Profondeur | Retrait d'alignement | Mitoyen |
|---|---|---|---|---|---|---|
| Résidentiel · faible | pavillons | R+0 à R+1 | 10–16 m | 9–12 m | 4–6 m | non |
| Résidentiel · moyenne | petits collectifs, brownstones | R+2 à R+5 | 12–30 m | 12–15 m | 0–3 m | oui |
| Résidentiel · forte | barres et tours HLM | R+5 à R+14 | 30–80 m | 12–16 m | 5–10 m | non |
| Résidentiel · centre | tours d'habitation | R+15 à R+45 | 25–40 m | 20–30 m | 0 m | oui |
| Commerce · faible | zone commerciale, grande surface | R+0 à R+1 | 40–120 m | 30–80 m | 15–30 m (parking) | non |
| Commerce · moyenne | rez commercial + logements | R+2 à R+6 | 10–25 m | 12–18 m | 0 m | oui |
| Commerce · forte | bureaux, grands magasins | R+6 à R+20 | 20–50 m | 20–40 m | 0 m | oui |
| Commerce · centre | tours de bureaux | R+20 à R+60 | 30–50 m | 30–50 m | 0–5 m | non |
| Activité (toutes) | entrepôts, ateliers | 1 niveau de 8 m | 30–120 m | 25–80 m | 8–15 m | non |
| Services publics (toutes) | équipement unique | R+1 à R+4 | 30–60 m | 20–40 m | 5–10 m | non |

Hauteur d'étage par défaut : 3 m pour le logement, 4 m pour un rez commercial, 3,5 m pour les bureaux.

Des **zones libres** (polygones) couvrent ce qui n'est pas bordé de rues : zone industrielle, port, champs. Les zones
de **gameplay** (`zone_interdite`, `zone_ia`, …) forment un calque distinct, qui n'est pas mélangé à l'urbanisme.

### 6.3 Lotissement : les bâtiments « poussent »
**Lotir l'îlot** découpe les façades sur rue en parcelles selon les règles de la famille et de la densité : largeur
de façade, profondeur, retrait d'alignement, mitoyenneté, nombre d'étages minimum et maximum. Chaque parcelle reçoit
ensuite un **bloc** (prisme : empreinte × étages × 3 m), posé sur une plateforme au niveau du terrain. Le résultat
est déterministe : une graine par îlot donne toujours le même découpage.

### 6.4 Règle d'or : on ne perd jamais le travail manuel
Chaque bâtiment porte une origine, `genere` ou `manuel`. Un bloc qu'on déplace, redimensionne ou verrouille passe en
`manuel` ; relancer le lotissement ne touche **que** les blocs `genere`. Si une route modifiée coupe un bâtiment
`manuel`, il est **signalé**, jamais supprimé en silence.

### 6.5 Bâtiments modélisés, gizmo
Un bâtiment peut référencer une **fiche de catalogue** (bâtiment modélisé, `.glb`). L'empreinte reste la vérité :
pour un bâtiment modélisé, c'est le rectangle de la fiche (largeur × profondeur, mesurées sur le modèle sans ses
maillages de collision `-col`) tourné de son **cap** autour de son centre. Routes, conflits et terrain la voient
comme un bloc ; le modèle s'y pose (sol au centre de l'empreinte, comme un bloc).

- **Champ** `modele` facultatif du bâtiment : `{ fiche, cap_deg }`. Fichiers de bâti en **version 2** (la
  version 1, sans modèles, se lit toujours ; migration explicite).
- **Cap** : rotation autour de +Y (main droite, vue de dessus : +X tourne vers −Z), en degrés, ramené à
  [0 ; 360[. Cap 0 = le modèle tel qu'exporté (avant = −Z).
- **Catalogue** (`mcf_editor/assets/batiments/catalogue.json`, versionné ; les `.glb` sont des assets du jeu, non
  versionnés) : `id` sûr, nom, famille, fichier `.glb` (relatif, confiné au dossier), largeur, profondeur, hauteur,
  centre de l'emprise dans le repère du modèle. Validé strictement. Une fiche ou un `.glb` absent n'empêche pas
  d'ouvrir la carte : le bâtiment s'affiche en bloc et l'éditeur le signale.
- **Gizmo** (bâtiments sélectionnés, Plan et 3D) : flèches X et Z (déplacement sur un axe), carré central
  (déplacement libre), anneau (rotation autour du centre de la sélection, crans de 5°, Alt : libre). Un geste =
  une annulation. Un bloc tourne son empreinte ; un bâtiment modélisé tourne son cap. Le verrou bloque les deux.
- **Inspecteur** : position du centre et cap saisis au clavier.
- **Pose** : liste des bâtiments du catalogue (étape Bâti), clic pour poser au curseur, `R` tourne de 90°
  pendant la pose ; le bâtiment posé est `manuel`.
- **Plus tard** : catalogue commun avec le jeu (`mcf_map::Catalog`), kits de façade, portes animées.

## 7. Édition croisée 2D ↔ 3D

Comme il n'y a qu'un document et qu'un historique, cela découle directement de la fusion :
- déplacer un bâtiment ou un point de route dans le Plan le déplace dans la 3D à l'image suivante, et inversement ;
- la sélection est partagée, et `Ctrl+Z` annule un geste entier, quelle que soit la vue ;
- l'accroche (aux emprises, à l'alignement, aux autres bâtiments) est calculée par le noyau, donc identique dans les
  deux vues.

## 8. Rendus : plan stylisé, vue satellite, calques

Les deux rendus sont faits par la **caméra orthographique sur la scène réelle**, puis exportés en tuiles PNG
(1024², à 0,5 m/pixel, soit 10 240² pour 5 km) avec une vue d'ensemble réduite. Ce sont des **dérivés**, régénérables,
signés par l'empreinte des sources.

- **Plan stylisé** : routes colorées par classe, quartiers teintés, bâtiments à aplat avec une ombre légère, courbes
  de niveau, noms des rues et des quartiers.
- **Vue satellite** : matériaux réalistes (textures Poly Haven choisies par famille de sol, voir `CLAUDE.md`
  d'Aroun), toits, ombres d'un soleil fixe, végétation simple.
- **Calques de référence** : n'importe quelle image (croquis, ancienne satmap, plan dessiné) peut être posée et
  géoréférencée (origine, m/pixel, rotation, opacité) pour décalquer dessus. Un calque n'est **jamais** une source de
  vérité. Une satmap exportée peut donc revenir en calque.
- **Minimap et GPS du jeu** (décidé) : le plan stylisé est exporté en **pyramide de tuiles** (plusieurs niveaux de
  zoom), avec un style « jeu » simplifié : routes, bâti, eau et noms, sans courbes de niveau. Le jeu affiche ces
  tuiles plutôt qu'une caméra vue du dessus, ce qui ne coûte presque rien en rendu. Le GPS calcule ses itinéraires
  sur le **graphe routier** exporté, et non sur l'image.

## 9. Données : un dossier « projet de carte »

Les sources sont versionnées et découpées par famille, comme dans `ARCHITECTURE_DONNEES_CARTE.md`. Elles sont en
mètres, dans le repère MCF (nord = −Z, en haut du plan). Les pixels n'existent plus que dans les calques image.

```text
cartes/<nom>/                      (dans le dépôt du jeu MCF-Remember-this-life)
  carte.mcfcarte.json      manifeste : version, emprise, cellule 256 m, plage d'altitude, graine, profils routiers
  relief/manifest.json     tuiles, empreintes
  relief/t_<x>_<z>.png     heightmap source 16 bits (Git LFS, comme les .glb)
  reseau.json              nœuds, tronçons (classe, points, rayons), carrefours explicites
  quartiers.json           affectation des îlots, zones libres, règles de lotissement
  batiments/c_<x>_<z>.json blocs par cellule (limite les conflits Git entre Aroun et Joaquine)
  gameplay.json            zones de gameplay
  calques/*.mcfmap         objets posés (format existant, inchangé)
  references.json + references/*.png    calques image
  .derive/                 NON versionné : terrain final, maillages, rendus, caches (supprimable)
  exports/                 plan, satellite, livraison jeu
```

Chaque fichier a sa version et sa **migration explicite**. Tout est validé strictement avant d'être chargé : champs
connus seulement, bornes, identifiants sûrs, polygones non auto-intersectés, taille plafonnée. L'écriture est
atomique et la validation a lieu **avant** chaque écriture.

**Jeu et serveur** : le client charge le terrain final, les routes, le bâti et les calques. Le serveur charge la
collision (terrain et blocs), le **graphe routier** (IA, GPS) et les zones, sans dépendre du rendu. `mcf_geo` étant
sans Bevy, le serveur et `mcf_big_map` l'utilisent tels quels.

## 10. Permissions, menaces, isolation

### 10.1 Qui voit et fait quoi
| Acteur | Lit | Écrit |
|---|---|---|
| Membre du studio (Aroun, Joaquine) via l'éditeur | le projet de carte ouvert | ce projet uniquement, par action explicite (enregistrer, exporter) |
| Éditeur (programme) | les fichiers **sous la racine du projet** | idem ; jamais en dehors, jamais en écrasant un export sans confirmation |
| Client de jeu | les données dérivées validées | rien |
| Serveur | collision, graphe, zones validés | rien (autorité sur le gameplay, pas sur la carte) |
| Mods (futur) | — | des cartes = données non fiables, même validation, aucun code ; l'île de base n'est modifiable que par `base_edits` validés |

### 10.2 Menaces et parades
| Menace | Parade |
|---|---|
| Fichier de carte piégé ou corrompu (mod, téléchargement) | validation stricte avant toute conversion, refus des champs inconnus, bornes, aucune exécution |
| Image piège (PNG géant, bombe de décompression) | dimensions lues dans l'en-tête et plafonnées **avant** décodage, poids maximal par fichier |
| Chemin qui sort du projet (`..`, chemin absolu, lien) | chemins relatifs canonisés et confinés à la racine, liens refusés |
| Perte de travail (plantage, mauvaise manip) | écriture atomique, journal d'autosauvegarde, Git, dérivés jamais seule copie |
| Divergence silencieuse entre 2D et 3D | noyau unique + test d'exactitude en CI (§2.5) |
| Régénération qui écrase du travail manuel | origine `genere`/`manuel`, verrou, conflits signalés (§6.4) |
| Conflits d'édition à deux | fichiers par famille et par cellule ; verrou de projet en option plus tard |
| Carte énorme qui gèle l'éditeur | limites (sommets, tronçons, bâtiments, tuiles), calculs lourds en tâche de fond |
| Dépendance compromise | `cargo deny` et revue avant tout ajout de crate (règle du dépôt) |
| Fiche de catalogue piégée (chemin vers un autre fichier, dimensions absurdes) | identifiants sûrs, fichier `.glb` relatif sans `..` ni séparateur absolu, confiné au dossier du catalogue, dimensions bornées (0,5 à 200 m), refus des champs inconnus |
| Bâtiment modélisé incohérent (empreinte ne correspondant pas à la fiche) | l'empreinte est **recalculée** depuis la fiche et le cap à chaque modification ; la validation vérifie un rectangle |

### 10.3 Isolation
Il n'y a pas de clients tiers : c'est un outil interne. L'isolation porte sur deux frontières :
- **entre projets de carte** : aucune référence inter-projets, tout chemin est relatif et confiné ;
- **entre la carte officielle et les mods** : un mod ne lit et n'écrit que ses propres calques, validés.

## 11. Jalons et critères de fin

| Jalon | Contenu | Fini quand |
|---|---|---|
| J0 | cette spec validée, ADR « éditeur unique 2D/3D » | Aroun a tranché les D-x |
| J1 | découpage de `mcf_editor` (fichier unique de 2 150 lignes → modules), crate `mcf_geo`, rubans continus | route en L sans trou ni chevauchement ; test d'exactitude vert |
| J2 | vue Plan = caméra ortho + style plan ; fin du dessin de géométrie en egui | capture Plan superposée à la 3D vue de dessus : écart nul |
| J3 | projet de carte (manifeste, réseau, quartiers, bâtiments), validation, tests de fuzz ; routes et carrefours v1 sortis de `.mcfmap` vers `reseau.json` | projet créé, rouvert à l'identique ; fichiers piégés refusés |
| J4 | outil Réseau (classes, accroche, carrefours auto, rayons) | maquette d'un quartier de 400 × 400 m tracée en < 15 min |
| J5 | relief (tuiles, pinceaux, affichage plan, terrassement routes) | route sur colline : pentes conformes, talus propres |
| J6 | îlots, zonage, lotissement, blocs, origine/verrou | relancer le lotissement ne touche aucun bloc `manuel` |
| J7 | exports plan / satellite, calques image | satmap 5 km exportée, réimportée en calque, calée au mètre |
| J8 | carte de test modérée (~1,5 km, 1 ville), puis 5 km et 3 villes reliées | budgets du §12 mesurés en `bench-release`, résultats bruts |

## 12. Budgets (proposés, à mesurer honnêtement)
- Vue Plan, carte de 5 km entière : ≥ 60 i/s sur le PC de dev.
- Modifier un tronçon : terrain, îlots et lotissement locaux recalculés en < 100 ms.
- Lotir un îlot : < 50 ms. Export satellite de 5 km : < 2 min.
- Ordre de grandeur pour 5 km et 3 villes : ~3 000 tronçons, ~5 000 blocs, 400 tuiles de relief.

## 13. Décisions prises (Aroun, 2026-11-22)
Aroun a délégué les choix techniques de cartographie (« carte blanche ») en demandant qu'ils restent **réglables**.

| # | Sujet | Décision |
|---|---|---|
| D-1 | Profils routiers | valeurs du §4.2 par défaut, réglables par carte dans le manifeste |
| D-2 | Gabarits de bâti | valeurs du §6.2 par défaut, réglables par carte |
| D-3 | Ancienne convention | **pas de reprise** : nouvelle convention propre, routes et carrefours v1 sortis de `.mcfmap` vers `reseau.json` (J3) |
| D-4 | Minimap / GPS | oui : pyramide de tuiles du plan stylisé + graphe routier (§8) |
| D-5 | Emplacement des cartes | dépôt du jeu, `cartes/<nom>/` |
| D-6 | Git LFS | oui pour les tuiles de relief et les calques image (`cartes/**/*.png`) |
| D-7 | Outil séparé | abandonné : tout dans `mcf_editor` (ADR-0001) |

Première carte de test : modérée (~1,5 km, une ville), puis la carte de 5 km avec trois villes reliées par des voies
rapides.
