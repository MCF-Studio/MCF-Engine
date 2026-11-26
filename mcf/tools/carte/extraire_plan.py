"""Extrait d'un plan illustré (carte couleurs de référence) les routes et les quartiers, pour peupler une carte MCF.

Entrée : l'image couleurs (mer bleue, villes saumon, zones d'activité violettes, routes rouge / orange / blanc) et son
calage (coin nord-ouest en mètres, mètres par pixel). Sortie : un JSON lu par
`cargo run -p mcf_carte --example peupler_carte --release -- <projet> <json>`, qui passe chaque élément par l'API
validée de `mcf_carte` (tracer_route, ajouter_zone, lotir).

- Routes : squelette de chaque couleur, découpé en tronçons entre carrefours, simplifié (Douglas-Peucker).
  rouge = autoroute ; orange = route ; blanc en ville = rue ; blanc hors ville = chemin de terre.
- Quartiers : taches saumon (ville) et violettes (activité), découpées en cases de `--case` m ; densité selon la part
  de bâti dans un rayon et la taille de la ville. Ce sont des emplacements plausibles, pas un relevé réel.

Usage : python extraire_plan.py <couleurs.png> <sortie.json> [--coin -16000 -16000] [--m-par-px 25.518...]
Dépendances : numpy, scipy, scikit-image, shapely, pillow.
"""

import argparse
import json

import numpy as np
from PIL import Image
from scipy import ndimage as ndi
from shapely.geometry import Polygon, box
from shapely.validation import make_valid
from skimage.measure import find_contours
from skimage.morphology import skeletonize


def masques(a):
    r, g, b = (a[..., i].astype(int) for i in range(3))
    eau = (b > 170) & (r < 110) & (g < 190)
    rouge = (r > 180) & (g < 90) & (b < 100)
    orange = (r > 215) & (g > 110) & (g < 200) & (b < 90)
    blanc = (r > 225) & (g > 225) & (b > 225)
    # Saumon et rose pâle (ville) ; le jaune des champs (bleu faible, vert proche du rouge) est exclu.
    saumon = (r > 215) & (g > 70) & (g < 225) & (b < 225) & (((r - g > 40) & (b > 50)) | ((b > 150) & (r - g > 15))) & ~orange
    violet = (r > 140) & (r < 215) & (g > 110) & (g < 175) & (b > 170) & (b - g > 25)
    return eau, rouge, orange, blanc, saumon, violet


def nettoyer(m, ouverture=0, min_px=0):
    if ouverture:
        m = ndi.binary_opening(m, iterations=ouverture)
    if min_px:
        lab, n = ndi.label(m, structure=np.ones((3, 3)))
        tailles = ndi.sum(m, lab, range(1, n + 1))
        garder = np.zeros(n + 1, bool)
        garder[1:] = tailles >= min_px
        m = garder[lab]
    return m


VOISINS = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)]


def graphe_squelette(sq):
    """Chemins de pixels entre nœuds (bouts et carrefours) d'un squelette."""
    h, w = sq.shape
    k = np.ones((3, 3), int)
    k[1, 1] = 0
    deg = ndi.convolve(sq.astype(int), k, mode="constant") * sq
    noeud = sq & (deg != 2)
    vus = set()
    chemins = []

    def voisins(y, x):
        for dy, dx in VOISINS:
            yy, xx = y + dy, x + dx
            if 0 <= yy < h and 0 <= xx < w and sq[yy, xx]:
                yield yy, xx

    for y, x in zip(*np.nonzero(noeud)):
        for v in voisins(y, x):
            if ((y, x), v) in vus:
                continue
            chemin = [(y, x), v]
            vus.add(((y, x), v))
            prec, cour = (y, x), v
            while not noeud[cour]:
                suivants = [n for n in voisins(*cour) if n != prec and n not in chemin[-3:]]
                if not suivants:
                    break
                prec, cour = cour, suivants[0]
                chemin.append(cour)
            vus.add((chemin[-1], chemin[-2]))
            chemins.append(chemin)
    # Boucles sans nœud (périphérique fermé) : on les coupe n'importe où.
    reste = sq.copy()
    for c in chemins:
        for p in c:
            reste[p] = False
    lab, n = ndi.label(reste, structure=np.ones((3, 3)))
    for i in range(1, n + 1):
        pts = list(zip(*np.nonzero(lab == i)))
        if len(pts) < 8:
            continue
        depart = pts[0]
        chemin, prec, cour = [depart], None, depart
        while True:
            suivants = [p for p in voisins(*cour) if lab[p] == i and p != prec and p not in chemin]
            if not suivants:
                break
            prec, cour = cour, suivants[0]
            chemin.append(cour)
        chemin.append(depart)
        chemins.append(chemin)
    return chemins


def douglas_peucker(pts, tol):
    if len(pts) < 3:
        return pts
    a, b = np.array(pts[0], float), np.array(pts[-1], float)
    ab = b - a
    n = np.hypot(*ab)
    p = np.array(pts, float)
    if n == 0:
        d = np.hypot(*(p - a).T)
    else:
        d = np.abs(ab[0] * (p[:, 1] - a[1]) - ab[1] * (p[:, 0] - a[0])) / n
    i = int(np.argmax(d))
    if d[i] > tol:
        return douglas_peucker(pts[: i + 1], tol)[:-1] + douglas_peucker(pts[i:], tol)
    return [pts[0], pts[-1]]


def routes(sq, classe, monde, min_px, tol):
    out = []
    for c in graphe_squelette(sq):
        if len(c) < min_px:
            continue
        s = douglas_peucker(c, tol)
        pts = [monde(y, x) for y, x in s]
        # Une route a au moins 2 points distincts, et pas de segment ridicule.
        propres = [pts[0]]
        for p in pts[1:]:
            if np.hypot(p[0] - propres[-1][0], p[1] - propres[-1][1]) > 5:
                propres.append(p)
        if len(propres) >= 2:
            out.append({"classe": classe, "points": propres})
    return out


def polygones(m, monde_xy):
    """Contours extérieurs des taches d'un masque, en mètres (trous ignorés)."""
    out = []
    lab, n = ndi.label(m, structure=np.ones((3, 3)))
    for i in range(1, n + 1):
        tache = np.pad(ndi.binary_fill_holes(lab == i), 1)
        cs = find_contours(tache.astype(float), 0.5)
        if not cs:
            continue
        c = max(cs, key=len)
        poly = Polygon([monde_xy(y - 1, x - 1) for y, x in c]).simplify(12)
        poly = make_valid(poly)
        out.append((i, poly))
    return out


def morceaux(g):
    if g.is_empty:
        return []
    if g.geom_type == "Polygon":
        return [g]
    return [p for p in getattr(g, "geoms", []) if p.geom_type == "Polygon"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("couleurs")
    ap.add_argument("sortie")
    ap.add_argument("--coin", nargs=2, type=float, default=[-16000.0, -16000.0])
    ap.add_argument("--m-par-px", type=float, default=32000.0 / 1254.0)
    ap.add_argument("--case", type=float, default=450.0, help="côté des cases de quartier, en mètres")
    ap.add_argument("--apercu", help="image de contrôle des masques")
    a = ap.parse_args()

    img = np.array(Image.open(a.couleurs).convert("RGB"))
    mpp, (x0, z0) = a.m_par_px, a.coin

    def monde(y, x):
        return [round(x0 + (x + 0.5) * mpp, 2), round(z0 + (y + 0.5) * mpp, 2)]

    eau, rouge, orange, blanc, saumon, violet = masques(img)

    # Neige : grandes taches blanches (une route blanche disparaît à l'ouverture).
    neige = nettoyer(blanc, ouverture=2, min_px=40)
    neige = ndi.binary_dilation(neige, iterations=3)
    # Ville : saumon refermé par-dessus ses rues blanches.
    ville = ndi.binary_closing(saumon | (blanc & ndi.binary_dilation(saumon, iterations=2)), iterations=2)
    ville = nettoyer(ville & ~eau, ouverture=1, min_px=12)
    activite = nettoyer(ndi.binary_closing(violet, iterations=2) & ~eau, ouverture=1, min_px=10)
    activite &= ~ville

    # Routes. Les lignes de 1 px se perdent à l'ouverture : on ferme seulement.
    rouge_m = nettoyer(ndi.binary_closing(rouge, iterations=1), min_px=30)
    orange_m = nettoyer(ndi.binary_closing(orange, iterations=1) & ~rouge_m, min_px=15)
    autour = ndi.binary_dilation(rouge_m | orange_m, iterations=1)
    blanc_m = nettoyer(blanc & ~neige & ~autour, min_px=6)
    ville_large = ndi.binary_dilation(ville, iterations=1)

    liste = []
    liste += routes(skeletonize(rouge_m), "autoroute", monde, 6, 1.2)
    liste += routes(skeletonize(orange_m), "route", monde, 5, 1.0)
    sq_blanc = skeletonize(blanc_m)
    liste += routes(sq_blanc & ville_large, "rue", monde, 3, 0.8)
    liste += routes(sq_blanc & ~ville_large, "chemin_terre", monde, 6, 1.0)

    # Quartiers.
    def monde_xy(y, x):
        return (x0 + (x + 0.5) * mpp, z0 + (y + 0.5) * mpp)

    part_ville = ndi.uniform_filter(ville.astype(float), size=25)  # ~640 m
    lab_v, _ = ndi.label(ville, structure=np.ones((3, 3)))
    taille_v = ndi.sum(ville, lab_v, range(lab_v.max() + 1))
    zones = []

    def decouper(poly, famille, densite_de):
        minx, minz, maxx, maxz = poly.bounds
        c = a.case
        gx = np.arange(np.floor(minx / c) * c, maxx, c)
        gz = np.arange(np.floor(minz / c) * c, maxz, c)
        for x in gx:
            for z in gz:
                for p in morceaux(poly.intersection(box(x, z, x + c, z + c))):
                    if p.area < 4000:
                        continue
                    p = Polygon(p.exterior).simplify(4)
                    if not p.is_valid or p.geom_type != "Polygon" or len(p.exterior.coords) < 4:
                        continue
                    cx, cz = p.centroid.x, p.centroid.y
                    zones.append(
                        {
                            "famille": famille,
                            "densite": densite_de(cx, cz),
                            "points": [[round(u, 2), round(v, 2)] for u, v in list(p.exterior.coords)[:-1]],
                        }
                    )

    def px(cx, cz):
        return (
            int(np.clip((cz - z0) / mpp, 0, img.shape[0] - 1)),
            int(np.clip((cx - x0) / mpp, 0, img.shape[1] - 1)),
        )

    # Cœur et rayon équivalent de chaque ville (en pixels).
    idx = range(lab_v.max() + 1)
    coeurs = ndi.center_of_mass(ville, lab_v, idx)
    rayons = np.sqrt(np.maximum(taille_v, 1) / np.pi)

    def densite_ville(cx, cz):
        """Grande ville : tours au cœur seulement, immeubles autour, pavillons en bordure. Bourg : petits
        immeubles au centre. Village : pavillons."""
        y, x = px(cx, cz)
        i = lab_v[y, x]
        if not i:
            return "faible"
        t = taille_v[i]
        cy, cxp = coeurs[i]
        d = np.hypot(y - cy, x - cxp) / rayons[i]
        if t >= 1500:
            if d < 0.12:
                return "centre"
            if d < 0.35:
                return "forte"
            if d < 0.75:
                return "moyenne"
            return "faible"
        if t >= 150 and d < 0.35:
            return "moyenne"
        return "faible"

    for _, poly in polygones(ville, monde_xy):
        decouper(poly, "residentiel", densite_ville)
    for _, poly in polygones(activite, monde_xy):
        decouper(poly, "activite", lambda cx, cz: "faible")

    # Quelques cases de centre deviennent des commerces (rez-de-chaussée commerçant dans le jeu actuel : ici des blocs).
    rng = np.random.default_rng(7)
    for z in zones:
        # Commerces de centre-ville : immeubles mitoyens de 3 à 7 niveaux.
        if z["famille"] == "residentiel" and z["densite"] in ("moyenne", "forte") and rng.random() < 0.25:
            z["famille"], z["densite"] = "commerce", "moyenne"

    # Lacs : eau intérieure, rivières retirées par ouverture, sans contact avec le bord (mer).
    eau_large = ndi.binary_opening(eau, iterations=3)
    lab_e, n_e = ndi.label(eau_large)
    bord = set(np.unique(np.concatenate([lab_e[0], lab_e[-1], lab_e[:, 0], lab_e[:, -1]])))
    lacs = []
    for i, poly in polygones(eau_large, monde_xy):
        if i in bord or poly.area < 30 * mpp * mpp:
            continue
        for p in morceaux(poly):
            lacs.append([[round(u, 2), round(v, 2)] for u, v in list(Polygon(p.exterior).exterior.coords)[:-1]])
    print("lacs   :", len(lacs))

    sortie = {"routes": liste, "zones": zones, "lacs": lacs, "niveau_eau_m": 0.0}
    with open(a.sortie, "w", encoding="utf-8") as f:
        json.dump(sortie, f)
    from collections import Counter

    print("routes :", dict(Counter(r["classe"] for r in liste)))
    print("zones  :", dict(Counter((z["famille"], z["densite"]) for z in zones)))
    if a.apercu:
        ap_img = np.zeros(img.shape, np.uint8) + 30
        ap_img[eau] = (40, 80, 160)
        ap_img[ville] = (230, 140, 120)
        ap_img[activite] = (170, 130, 200)
        ap_img[neige] = (250, 250, 250)
        ap_img[skeletonize(orange_m)] = (255, 170, 0)
        ap_img[sq_blanc] = (255, 255, 255)
        ap_img[skeletonize(rouge_m)] = (255, 0, 0)
        Image.fromarray(ap_img).resize((img.shape[1] * 2, img.shape[0] * 2), Image.NEAREST).save(a.apercu)


if __name__ == "__main__":
    main()
