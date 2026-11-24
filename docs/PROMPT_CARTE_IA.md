# Consignes pour produire une carte source de l'éditeur MCF (à donner à une IA)

Ce texte est copié par le bouton « 📋 Copier la convention » de l'éditeur (Cadre → Calques de référence). Il résume
`docs/CONVENTION_CARTES_SOURCE.md`. Remplacer les valeurs entre crochets avant de l'envoyer.

---

Je prépare une carte pour un éditeur de jeu. J'ai besoin de **deux images au même cadrage exact** : même zone du
monde, même nord (en haut), mêmes bords, même taille en pixels. Le pixel (x, y) des deux images doit désigner le
même endroit.

Zone : [description ou image fournie]. Taille réelle : [côté en km]. Taille de l'image : [côté en pixels, 8 192 au
plus].

## Image 1 : carte couleurs

- Aplats unis, **une couleur exacte par catégorie** : aucun dégradé, ombrage, texture, relief ombré.
- **Aucun texte, pictogramme, légende, quadrillage, échelle ni bordure** dans l'image.
- Routes en **traits continus d'épaisseur constante**, sans contour ni pointillés, une couleur par classe ;
  épaisseur 2 pixels au moins, idéalement la largeur réelle à l'échelle (autoroute 30 m, route 10 m, rue 14 m).
- Couleurs (sRGB) :

| Catégorie | Couleur | sRGB |
| --- | --- | --- |
| Eau (mer, lacs, rivières larges) | bleu | 64, 150, 220 |
| Campagne (fond) | vert clair | 184, 217, 155 |
| Forêt | vert foncé | 90, 150, 90 |
| Ville | rouge saumon | 232, 131, 107 |
| Industrie, logistique | mauve | 186, 160, 210 |
| Agriculture | jaune | 240, 222, 110 |
| Autoroute | rouge vif | 215, 38, 61 |
| Route interurbaine | orange | 242, 165, 65 |
| Rue | blanc | 255, 255, 255 |

## Image 2 : carte de relief

- **Niveaux de gris uniquement** : noir = point le plus bas ([altitude du noir, ex. niveau de la mer 0 m]), blanc
  = point le plus haut ([altitude du blanc, ex. 2 400 m]), linéaire entre les deux.
- Dégradés continus, **aucun texte, trait, contour, courbe de niveau, ombrage ni relief ombré**.
- Mer en noir uniforme (ou transparente si on ne veut pas toucher au relief à cet endroit).
- Chaque lac en **gris uniforme** à son altitude.
- Transparent = pas de donnée (le relief existant n'y est pas modifié).

## Rappel

Les deux images doivent se superposer exactement. Si tu ne peux pas garantir le même cadrage, dis-le.
