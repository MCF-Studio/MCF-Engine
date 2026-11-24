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
  - [ ] Étape 2 : relevé assisté sur un calque calé — trait de côte / eau par la couleur (→ niveau d'eau, zones),
    taches urbaines (→ quartiers à affecter), proposition de tracés routiers à valider un par un. Rien d'automatique
    sans validation.
  - À trancher (Aroun) : `map_1.png` couvre **32 × 32 km**, la carte est plafonnée à **16 km** de côté (relief en
    mémoire). Soit on vise une portion, soit on relève le plafond (à mesurer). À 1 254 pixels pour 32 km, l'image vaut
    ~25 m par pixel : bonne pour les grands axes, villes et côtes, trop grossière pour les rues (les plans de ville
    de `map/city/` serviront pour ça).
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

Texte d'origine (Aroun) :

Pour la vue 2D 3D, le mix 2D 2D, il faudrait en fait que la vue 2D soit comme une petite fenêtre et qu'on puisse choisir de la grandir ou pas. En gros si on clique dessus elle se met là comme elle est actuellement et qu'on puisse la rapetissir mais qu'on ait toujours une vue de la minimap quand même et je pense que ça pourrait rendre la chose pas mal.
Quand on sélectionne un élément, il faudrait que dans l'inspecteur, on tombe directement sur ce que c'est. Par exemple, quand on est en train de faire du relief et qu'on a une route qui ne nous plaît pas, si je sélectionne la route, il faut que ça aille directement dans la catégorie réseau et que ça me sélectionne la bonne route que j'ai choisie. Comme ça, ça évite là, parce que ça pose des petits soucis de l'expérience utilisateur et du oui. du iux qui sont pas terribles
Tant qu'on est à ce niveau-là de l'éditeur, j'ai prévu d'avoir les bâtiments et les quartiers, donc les villes, avec des compteurs électriques. Et j'ai prévu que du coup on puisse avoir un compteur général qui puisse couper tout un quartier. On va tout de suite faire en sorte d'ajouter dans Réseau le réseau électrique, donc que je puisse poser des gaines, alors pas forcément du réseau aérien visible, mais qu'on voie une ligne qui déterminera que c'est le réseau électrique.
et il faudra prévoir que l'on puisse importer des cartes 2D qui ont des tracés et que l'éditeur les convertisse pour pouvoir afficher les tracés correctement et refaire la map 2D aux conventions de l'éditeur. Ce qu'il faut c'est vraiment pouvoir relever le tracé des routes, les reliefs si on arrive à l'éditeur. Et les endroits où sont les bâtiments. C'est entre si on te donne une satmap ou une map un peu stylisée qu'on arrive à pouvoir s'en servir comme modèle. Je t'ai mis un chemin en référence du type de carte. Je l'ai bien dit du type de carte, c'est pas spécialement ça mais on pourrait obtenir avec l'éditeur si on pouvait obtenir ça. envoyé dans l'éditeur justement pour servir de modèle "G:\MCF-Remember-this-life\docs\map"
Et bien entendu, il faudra travailler un peu le rendu parce que là la luminosité est vraiment pas ouf. Il faudra mettre quand même une skybox, on ait quand même l'impression d'être dans un monde. Et essayer de trouver des textures, mettre des textures un peu plus qualitatives. Si tu veux, donne moi un prompt, je peux faire générer des textures par ton grand copain qui s'appelle ChatGPT.
et puis il faudrait qu'avec A et E on puisse monter et descendre avec la caméra
