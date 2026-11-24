- [x] nom pour nommer une rue/route ne fonctionne pas. → corrigé (le texte tapé était remplacé à chaque image) ; « Valider » ou Entrée ; renomme toute la voie qui portait le même nom. *Pas encore vérifié à l'écran.*
- [x] quand on passe en vue 2D à gauche et 3D à droite la map 2D n'est pas loadé → corrigé et vérifié.
- [x] rendre un peu plus intutif l'éditeur en mettant les étapes en cascade, grossir un peu la taille de la police → fait (étapes empilées, l'étape courante se déplie, police 15 px).
- [x] stylyser un peu l'éditeur en mettent de la couleur → première passe : une couleur par étape, accent orange, panneaux sombres.
- [x] je peux pas ajouter suprimer un point sur une route → Ctrl+clic sur la route sélectionnée : ajoute un point ; Ctrl+clic sur un point : le retire. *Pas encore vérifié à l'écran.*
- [x] Mini-carte : la 3D occupe l'écran avec le Plan en incrustation (repère rouge = où regarde la caméra) ; clic dessus → Plan + 3D ; « 3D + mini-carte » ou Tab pour la rapetisser. Vérifié.
- [x] Sélection → bonne étape : sélectionner une route ouvre Réseau, un bâtiment Bâti, un quartier Quartiers ; Maj+clic sélectionne depuis n'importe quel outil (ex. en sculptant le relief). Vérifié pour un quartier.
- [x] A / E pour monter / descendre la caméra 3D (touches physiques : Q / E en QWERTY). *Pas encore vérifié à l'écran.*
- [ ] Réseau électrique : gaines (lignes, pas forcément aériennes), compteurs de bâtiment, compteur général qui coupe tout un quartier.
- [ ] Import de cartes 2D (satmap ou plan stylisé, réf. `G:\MCF-Remember-this-life\docs\map`) comme modèle : relever routes, relief, emplacements de bâtiments, aux conventions de l'éditeur.
  - [x] **Étape 1 : calques de référence** (codée, testée ; **pas encore vue à l'écran**). Étape 1 « Cadre » →
    « Calques de référence » : chemin d'un PNG → « Importer l'image ». L'image se pose sur toute la largeur de la
    carte, à moitié transparente, dans le Plan. Réglages (opacité, échelle, rotation, position) en direct puis
    « Appliquer » ; « 📏 Caler » : cliquer deux repères de l'image (ex. les graduations 0 et 4 km de l'échelle), taper
    leur distance réelle, « Caler l'échelle » ; « ✋ Déplacer » : glisser le calque. Tout est annulable et enregistré
    avec le projet (`references.json` + `references/`). On décalque ensuite avec les outils habituels.
  - [x] **Étape 2 : relevé assisté** (codée, testée ; **pas encore vue à l'écran**). Étape 1 « Cadre », sous les
    calques : choisir « Eau / côte », « Taches (quartiers) » ou « Routes », puis « 🎯 Cliquer une couleur du calque » et
    cliquer dans le Plan. L'éditeur propose ; « ✔ Valider » / « ✖ Rejeter » une par une (◀ ▶ pour parcourir,
    « ◎ Voir » pour centrer). Rien n'est écrit avant validation, chaque validation s'annule (Ctrl+Z).
    - Eau : le relief est **abaissé** sous le niveau choisi (profondeur, rive en pente douce), jamais relevé ; le
      niveau d'eau de la carte est réglé. Les îles restent, les petits trous (textes) sont comblés.
    - Taches : zone libre au contour simplifié, avec l'usage et la densité choisis.
    - Routes : tracés de la classe choisie, accrochés au réseau existant.
    - Réglages : tolérance de couleur, « Combler » (textes et traits fins), aire ou longueur minimale, « Toutes les
      taches de cette couleur » ; « ↻ Relancer » refait l'analyse au même endroit.
    - Essai sur `map_1.png` (calée à ~34 m/pixel) : mer (313 km²) et Grand Lac (14,6 km²) propres en ~50 ms. Villes
      **morcelées** par les rues (Tolosa : plus grande tache 6,2 km² sur ~60) : monter « Combler ». A1 relevée **par
      tronçons** (4,4 km d'un clic) : le dessin est coupé aux panneaux et aux échangeurs. Les routes très fines ou
      en pointillés demandent un clic précis sur le trait.
  - [x] ~~À trancher : carte plafonnée à 16 km~~ → **plafond relevé à 32 km** (côté saisi librement ; relief à 1 m
    jusqu'à 8 km, 2 m jusqu'à 16 km, 4 m au-delà). À 1 254 pixels pour 32 km, `map_1.png` vaut ~25 m par pixel :
    bonne pour les grands axes, villes et côtes, trop grossière pour les rues (les plans de `map/city/` serviront).
  - [x] **Étape 3 : relief depuis une carte en niveaux de gris** (codée, testée de bout en bout ; **pas encore vue à
    l'écran**). Convention et exemple : `docs/CONVENTION_CARTES_SOURCE.md` (deux images au même cadrage : carte
    couleurs + carte de relief). Sur le calque de relief : « Même calage que… » la carte couleurs, altitudes du noir
    et du blanc, lissage, « ⛰ Appliquer comme relief ». Plage d'altitude réglable à la création de la carte.
    - Exemple `docs/exemples/ile_exemple_*.png` (4 km) : relief 1,0 s, mer 0,95 s, ville et autoroute relevées d'un
      clic (mesures brutes, `--release`). Projet complet : `cargo run -p mcf_carte --example ile_exemple --release
      -- <dossier>`. **Vu à l'écran** (2026-11-24) : île, massif, lac, mer, quartiers lotis, autoroute ; liste des
      classes avec « Voie express » et « Autoroute ».
    - Marches des 256 niveaux : arcs bruns le long des courbes de niveau avec un lissage faible. Lissage conseillé =
      25 × la marche (63 m pour l'exemple), proposé par l'éditeur ; 48 m les efface à l'écran.
    - [x] **Convertisseur** (2026-11-24) : « 🪄 Convertir une carte… » (Cadre) ou `cargo run -p mcf_carte --example
      convertir_carte --release -- <image>` : vue satellite / plan → carte couleurs + relief **proposé** (déduit).
      Essai sur `map_sat.png` : mer, lacs, rivières, massif, forêts, champs, villes reconnus en 0,3–0,4 s ; sud en
      partie pris pour de la ville ; routes non extraites. Bouton **pas encore vu à l'écran**.
    - [x] Retours d'Aroun (2026-11-24) : niveaux (gris du noir / du blanc, courbe), garde-fou « calque en couleurs »,
      plus de scintillement (calques étagés), pinceaux de relief jusqu'à 2 km. **Pinceau sur le calque de relief**
      (Éclaircir, Assombrir, Niveau, Adoucir, Effacer ; Ctrl+clic = prendre le gris ; annulable ; image réécrite à
      l'enregistrement) : codé et testé dans mcf_carte, boutons dans l'éditeur, **pas encore vu à l'écran**.
    - [x] Relevé assisté : il lit désormais le calque actif s'il est en couleurs, sinon le calque visible en
      couleurs le plus haut (« Relevé sur « … » » dans le panneau) ; plus de relevé sur le relief gris. Testé.
    - [x] Bouton « 📋 Copier la convention » (Cadre → Calques de référence) : copie `docs/PROMPT_CARTE_IA.md` à
      donner à une IA. **Pas encore vu à l'écran.**
    - [ ] **À FAIRE** :
      1. Vérifier à l'écran : pinceau de calque (rafraîchissement ≤ 10 Hz pendant le geste), niveaux, absence de
         scintillement.
    - Reste à voir : léger motif en pointillés sur les pentes fortes du massif (ombrage du maillage sous-échantillonné
      ou ombres ? non élucidé) ; terre-plein de l'autoroute pas encore regardé de près.
    - `map_1.png` et `map_sat.png` n'ont **pas** le relief en chiffres : demander une carte de relief au même cadrage
      (prompts dans la convention).
- [x] Rendu : luminosité, ciel, textures plus qualitatives → première passe codée. **Pas encore vue à l'écran**
  (compilée, testée, éditeur non lancé).
  - Ciel physique de Bevy (`Atmosphere`) au lieu du fond bleu uni et du brouillard : ciel, disque solaire, brume
    lointaine et lumière ambiante suivent le soleil (après-midi, sud-ouest, 38° de hauteur).
  - Soleil en unités réelles avec exposition « plein jour », ombres jusqu'à ~2 × le côté de la carte.
  - Textures Poly Haven (CC0) copiées depuis la bibliothèque locale : herbe (terrain, îlots), terre (chemins,
    accotements), asphalte, dalles de trottoir, enduit (murs, teinté par famille), bitume (toits). Couleur + normales
    + AO / rugosité, plaquées en mètres, mipmaps calculées au chargement. Terrain teinté selon l'altitude et la pente.
  - Le Plan n'a plus de tonemapping : ses aplats sont affichés avec leurs couleurs exactes (un peu plus vives
    qu'avant).
  - À voir à l'écran : réglage de l'exposition, taille des répétitions, sens des reliefs des normales.
  - Plus tard : textures en KTX2 compressées (16 Mo de PNG aujourd'hui), choix de l'heure du jour dans l'éditeur.
