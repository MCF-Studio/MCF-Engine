# Centre spatial et station — note de JEU

> **Conception du jeu, pas du moteur.** Brouillon du 2026-11-24, à partir des réponses d'Aroun. Les besoins moteur
> qui en découlent sont résumés à la fin et repris dans `ROADMAP.md` (section « Capacités moteur exigées par le
> jeu »). Rien ici n'est à coder pendant les phases moteur.

## Idée

Un **easter egg** dans le monde RP (style GTA RP / Altis Life) : un centre spatial d'où l'on peut **réellement**
partir dans l'espace, jusqu'à **une seule station**. Pas d'exploration spatiale, pas de système solaire : une
fusée, une station, une petite activité, et la plus belle vue possible sur notre monde.

## Ce que vit le joueur

### 1. Au sol : « c'est quoi ce truc dans le ciel ? »

- La station est **visible depuis le sol** : un point lumineux qui traverse le ciel, surtout à l'aube et au
  crépuscule (comme l'ISS dans la réalité), plus ou moins brillant selon l'éclairage du soleil.
- Avec des jumelles ou un téléobjectif, on distingue sa **silhouette**.
- Elle passe régulièrement (orbite accélérée : un passage toutes les N minutes de jeu, N à régler), toujours sur la
  même trajectoire : les joueurs peuvent la guetter.

### 2. Le départ : uniquement en fusée

- **Seul moyen** d'aller dans l'espace : la fusée du centre spatial. Avions et hélicoptères ont un **plafond
  d'altitude** (l'air raréfié leur fait perdre la portance et la poussée), même s'ils montent très haut, ils ne
  passent jamais.
- Capacité de la fusée : **4 ou 5 joueurs** (à trancher).
- Montée **continue, sans écran de chargement**, de quelques minutes : le sol s'éloigne, le ciel passe du bleu au
  bleu nuit puis au noir, la courbure de la Terre apparaît, puis la station grossit jusqu'à l'amarrage.

### 3. La station : un lieu comme un autre

- **Pas de limite de joueurs propre à la station** : c'est un lieu de la carte ; qui peut y aller y va (seule la
  capacité de la fusée limite le débit).
- **Apesanteur** à l'intérieur : on flotte, on se tire aux poignées, on se pousse contre les parois.
- **Hublots et coupole** avec la plus belle vue possible sur notre monde : la carte du jeu vue d'en haut (villes,
  routes, côtes), nuages, lumières des villes la nuit, lever de soleil sur l'horizon courbe.

### 4. L'activité : le drone lunaire

- Un **poste de contrôle** dans la station : on prend la main sur un ordinateur.
- Depuis ce poste, on pilote un **petit drone sur la Lune** (zone lunaire réduite : cratères, régolithe) qui
  **récolte de la pierre lunaire**.
- La pierre récoltée arrive à la station (stock limité) ; il faut ensuite **la ramener sur Terre** (retour en
  capsule) pour qu'elle ait une valeur RP (vente, labo, musée, marché noir… à définir).

### 5. Le retour

- Une **capsule de retour** redescend joueurs et cargaison, avec atterrissage dans une zone prévue (en mer, à
  récupérer en bateau ? sous parachutes près du centre ? à trancher).

## Comment rendre ça pas cher (choix proposé)

Une vraie orbite à 400 km avec de la physique libre coûterait très cher (coordonnées géantes, planète entière à
rendre, physique instable loin de l'origine). Proposition qui donne le même effet pour bien moins cher :

- **Le ciel « à l'échelle » (technique du *scaled space*)** : une caméra de fond, dessinée avant la scène, montre
  la station et la Lune en **modèles réduits** placés dans un petit « ciel » à l'échelle 1/1000. Vue depuis le sol,
  une station réduite à 400 m de distance ressemble exactement à une vraie station à 400 km. Coût : une caméra et
  quelques centaines de triangles. C'est ce qui rend la station **visible en permanence** depuis le sol.
- **Trois lieux séparés**, chacun avec son propre repère proche de l'origine : le **monde** (la carte RP), la
  **station** (petite scène intérieure + vue), la **Lune** (petite zone pour le drone). Aucun objet jouable n'est
  jamais à 400 km de l'origine : pas besoin d'origine flottante.
- **La montée est une transition mise en scène** (trajectoire calculée par le serveur, pas pilotée librement) : la
  fusée monte dans le monde jusqu'à quelques km, puis les passagers basculent sans coupure dans la « vue de
  transition » (Terre courbe + ciel à l'échelle) jusqu'à la station. Même principe à la descente.
- **La Terre vue de l'espace** : une sphère texturée avec la **vue satellite exportée de notre carte** au centre
  (l'export satellite existe déjà dans l'éditeur), entourée de terres et d'océans génériques, avec l'atmosphère vue
  de l'extérieur et une couche de nuages.

Si le *scaled space* ne donne pas le rendu espéré au banc d'essai, repli : **échelle compressée** (station
réellement placée vers 80–100 km au-dessus de la carte).

## Qui peut faire quoi (permissions)

| Action | Qui | Contrôle |
| --- | --- | --- |
| Monter dans la fusée | Joueur présent au centre spatial, place libre | Serveur (places, distance, état de la fusée) |
| Lancer la fusée | À trancher : pilote désigné (métier ?), ou lancement automatique à heure fixe | Serveur |
| Entrer dans la zone station / Lune | **Uniquement** par le transfert déclenché par la fusée ou la capsule | Serveur |
| Piloter le drone lunaire | Joueur présent au poste de contrôle de la station, poste libre | Serveur |
| Charger la pierre dans la capsule | Joueur présent dans la station | Serveur (stock, poids) |
| Régler la station (orbite, horaires, stock) | Administrateurs du serveur | Audit log |

## Menaces principales et mesures

| Menace | Mesure |
| --- | --- |
| Joueur qui modifie sa position pour se téléporter dans la station | Le changement de zone n'existe **que** côté serveur, déclenché par la fusée / la capsule ; une position client hors de sa zone ou au-dessus du plafond est refusée |
| Avion / hélico modifié pour dépasser le plafond | Plafond vérifié par le **serveur** selon le type de véhicule, pas par le client |
| Duplication de pierre lunaire (déconnexion pendant le retour, double clic) | Transfert de cargaison **transactionnel** en base (PostgreSQL), identifiant unique par lot, idempotent |
| Spam de la fusée / de la station (embouteillage, grief) | Délai entre lancements, places limitées, règles RP et modération |
| Drone lunaire piloté par un client modifié (récolte accélérée) | Récolte calculée côté serveur (débit max, stock max) ; le client n'envoie que des commandes |

## Décisions ouvertes (pour Aroun)

1. Fusée : 4 ou 5 places ? Qui la lance (métier « astronaute / opérateur », lancement à heure fixe, payant) ?
2. Retour : capsule en mer (récupération en bateau, RP possible) ou atterrissage près du centre ?
3. Valeur de la pierre lunaire : qui l'achète, combien, limites par jour ?
4. Fréquence de passage de la station dans le ciel.
5. Accès : ouvert à tous, ou réservé (licence, métier, prix du billet) ?

## Capacités moteur exigées (résumé pour `ROADMAP.md`)

Formulées comme capacités génériques, sans gameplay :

1. **Plusieurs zones par serveur** (monde, intérieurs isolés, zones lointaines), chacune avec son repère local, et
   **transfert de joueurs/véhicules entre zones** décidé par le serveur → phase 6.
2. **Ciel à l'échelle** : caméra de fond pour objets célestes lointains (station, Lune, planète) → phase 7
   (cycle jour / nuit).
3. **Planète vue de l'espace** : sphère texturée + atmosphère vue de l'extérieur + nuages, alimentée par l'export
   satellite de la carte → à mesurer au banc (rendu et coût).
4. **Gravité par zone** et **contrôleur de personnage en apesanteur** → phase 7 (physique, personnages).
5. **Plafond d'altitude par type de véhicule**, vérifié côté serveur → phase 7 (véhicules) et menaces.
6. **Trajectoires mises en scène** (véhicule qui suit un chemin calculé par le serveur, passagers à bord) → phase 7.
