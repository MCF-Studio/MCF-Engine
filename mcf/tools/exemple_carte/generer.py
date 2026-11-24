"""Génère l'exemple de la convention des cartes sources (docs/CONVENTION_CARTES_SOURCE.md).

Deux images au MÊME cadrage, 1024 × 1024 pixels pour 4 096 × 4 096 m (4 m par pixel), nord en haut :
- ile_exemple_couleurs.png : aplats de couleurs de la palette de la convention (eau, zones, routes) ;
- ile_exemple_relief.png   : niveaux de gris, noir = -40 m, blanc = 600 m, transparent hors île au large.

Reproductible (graine fixe). Usage, depuis la racine du dépôt :
    python mcf/tools/exemple_carte/generer.py
"""

from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

COTE_PX = 1024
M_PAR_PX = 4.0
NOIR_M, BLANC_M = -40.0, 600.0
GRAINE = 20261124

# Palette de la convention (sRGB) : aplats purs, sans dégradé ni anticrénelage.
EAU = (64, 150, 220)
CAMPAGNE = (184, 217, 155)
URBAIN = (232, 131, 107)
INDUSTRIE = (186, 160, 210)
AGRICULTURE = (240, 222, 110)
FORET = (90, 150, 90)
AUTOROUTE = (215, 38, 61)
ROUTE = (242, 165, 65)
RUE = (255, 255, 255)

SORTIE = Path(__file__).resolve().parents[3] / "docs" / "exemples"


def bruit(rng, taille, octaves=5):
    """Bruit fractal lisse (somme d'octaves interpolées), valeurs ~[-1, 1]."""
    total = np.zeros((taille, taille))
    amplitude, somme = 1.0, 0.0
    for o in range(octaves):
        n = 4 * 2**o
        grille = rng.uniform(-1, 1, (n + 1, n + 1))
        img = Image.fromarray(((grille + 1) * 127.5).astype(np.uint8)).resize((taille, taille), Image.Resampling.BICUBIC)
        total += amplitude * (np.asarray(img, dtype=float) / 127.5 - 1)
        somme += amplitude
        amplitude *= 0.5
    return total / somme


def main():
    rng = np.random.default_rng(GRAINE)
    y, x = np.mgrid[0:COTE_PX, 0:COTE_PX] / (COTE_PX - 1) * 2 - 1  # -1..1, y vers le sud

    # Île : distance au centre déformée par le bruit ; > 0 = terre.
    terre = 0.72 - np.hypot(x * 1.05, y) + 0.18 * bruit(rng, COTE_PX)
    # Massif au nord, collines douces ailleurs, plaine côtière au sud.
    massif = np.clip(1 - np.hypot(x + 0.1, y + 0.45) / 0.45, 0, 1) ** 1.6
    # Plaine côtière douce (montée progressive vers l'intérieur), collines modérées, massif plus raide.
    interieur = np.clip(terre / 0.5, 0, 1)
    interieur = interieur * interieur * (3 - 2 * interieur)
    collines = 12 * bruit(rng, COTE_PX, 4) * interieur
    altitude = np.where(
        terre > 0,
        2 + 140 * interieur + 430 * massif + collines,
        -40 * np.clip(-terre * 6, 0, 1),
    )
    # Lac à 140 m dans une cuvette à l'ouest du massif.
    lac = np.hypot((x + 0.35) / 0.11, (y + 0.05) / 0.07) < 1
    altitude = np.where(lac, 140.0, altitude)
    altitude = np.clip(altitude, NOIR_M, BLANC_M)

    # --- Carte de relief : gris = (alt - noir) / (blanc - noir) ; transparent au large (pas de donnée).
    gris = np.round((altitude - NOIR_M) / (BLANC_M - NOIR_M) * 255).astype(np.uint8)
    alpha = np.where(terre > -0.12, 255, 0).astype(np.uint8)
    Image.fromarray(np.dstack([gris, gris, gris, alpha]), "RGBA").save(SORTIE / "ile_exemple_relief.png")

    # --- Carte couleurs.
    img = np.zeros((COTE_PX, COTE_PX, 3), dtype=np.uint8)
    img[:] = EAU
    img[terre > 0] = CAMPAGNE
    img[(terre > 0) & (massif > 0.35)] = FORET
    img[lac] = EAU
    ville = (np.hypot((x - 0.18) / 0.26, (y - 0.42) / 0.18) < 1) & (terre > 0.02)
    zone_industrielle = (np.abs(x - 0.52) < 0.09) & (np.abs(y - 0.2) < 0.07) & (terre > 0)
    champs = (np.abs(x + 0.35) < 0.18) & (np.abs(y - 0.38) < 0.12) & (terre > 0)
    img[champs] = AGRICULTURE
    img[zone_industrielle] = INDUSTRIE
    img[ville] = URBAIN

    def px(xn, yn):
        return ((xn + 1) / 2 * (COTE_PX - 1), (yn + 1) / 2 * (COTE_PX - 1))

    # Rues de la ville : quadrillage blanc de 2 px (8 m) tous les ~25 px (100 m), seulement dans la ville.
    masque = Image.new("1", (COTE_PX, COTE_PX), 0)
    m = ImageDraw.Draw(masque)
    for i in range(-14, 15):
        xs = 0.18 + i * 0.05
        m.line([px(xs, 0.24), px(xs, 0.60)], fill=1, width=2)
        ys = 0.42 + i * 0.05
        m.line([px(-0.08, ys), px(0.44, ys)], fill=1, width=2)
    img[np.asarray(masque, dtype=bool) & ville] = RUE
    couleurs = Image.fromarray(img, "RGB")
    d = ImageDraw.Draw(couleurs)
    # Route (orange, 3 px = 12 m) : ville → lac, en suivant la côte ouest.
    d.line([px(0.0, 0.42), px(-0.30, 0.30), px(-0.45, 0.05), px(-0.35, -0.10)], fill=ROUTE, width=3, joint="curve")
    # Autoroute (rouge, 6 px = 24 m) : ville → zone industrielle → nord-est.
    d.line([px(0.30, 0.40), px(0.52, 0.28), px(0.58, 0.0), px(0.45, -0.35)], fill=AUTOROUTE, width=6, joint="curve")
    couleurs.save(SORTIE / "ile_exemple_couleurs.png")

    print(f"Écrit dans {SORTIE} : {COTE_PX} px pour {COTE_PX * M_PAR_PX:.0f} m ({M_PAR_PX} m/px), "
          f"relief noir {NOIR_M} m / blanc {BLANC_M} m.")


if __name__ == "__main__":
    SORTIE.mkdir(parents=True, exist_ok=True)
    main()
