"""Corrige une carte de relief générée (mer noire, terre qui démarre à ~40 % de gris) pour l'éditeur MCF.

- Mer (noir relié au bord) : -10 m.
- Terre : gris recentré (le gris de la côte devient ~2 m), courbe pour aplatir les plaines et garder les massifs,
  rampe douce sur les premiers centaines de mètres depuis la côte, léger débruitage.
- Eaux intérieures (rivières, lac : plus sombres que la terre autour) : 3 m sous la terre voisine lissée.
- Encodage en racine carrée : l'éditeur relit avec « courbe » = 2, ce qui donne des marches de 1 à 3 m en plaine au
  lieu de ~10 m (8 bits seulement).

Usage : python corriger_relief.py entree.png sortie.png [sommet_m]
"""

import sys

import numpy as np
from PIL import Image

ENTREE, SORTIE = sys.argv[1], sys.argv[2]
SOMMET = float(sys.argv[3]) if len(sys.argv) > 3 else 2412.0
FOND_MER = -10.0
PLAINE_M = 60.0  # altitude visée pour le gris médian de la terre
RAMPE_PX = 24  # ~600 m à 25 m/px : la côte remonte en douceur
GRIS_COTE = 0.30  # gris de terre le plus bas (quantile 5 %) : devient la côte

a = np.asarray(Image.open(ENTREE).convert("L")).astype(np.float64) / 255.0


def voisins(m):
    d = m.copy()
    d[1:, :] |= m[:-1, :]
    d[:-1, :] |= m[1:, :]
    d[:, 1:] |= m[:, :-1]
    d[:, :-1] |= m[:, 1:]
    return d


def depuis_bord(masque):
    r = np.zeros_like(masque)
    r[0, :], r[-1, :], r[:, 0], r[:, -1] = masque[0, :], masque[-1, :], masque[:, 0], masque[:, -1]
    while True:
        d = voisins(r) & masque
        if (d == r).all():
            return r
        r = d


def flou(x, sigma):
    r = int(3 * sigma)
    k = np.exp(-0.5 * (np.arange(-r, r + 1) / sigma) ** 2)
    k /= k.sum()
    p = np.pad(x, r, mode="edge")
    # Séparable : lignes puis colonnes.
    t = np.zeros((p.shape[0], x.shape[1]))
    for i in range(2 * r + 1):
        t += k[i] * p[:, i : i + x.shape[1]]
    y = np.zeros(x.shape)
    for i in range(2 * r + 1):
        y += k[i] * t[i : i + x.shape[0], :]
    return y


def flou_masque(x, m, sigma):
    """Moyenne floue de x sur les seuls pixels de m."""
    n = flou(x * m, sigma)
    d = flou(m.astype(np.float64), sigma)
    return np.where(d > 1e-6, n / np.maximum(d, 1e-6), 0.0)


mer = depuis_bord(a < 0.06)
terre = ~mer

# Eaux intérieures : nettement plus sombres que la terre autour.
local = flou_masque(a, terre, 6.0)
eau = terre & (a < local - 0.05)
sol = terre & ~eau

# Distance à la mer (pixels, plafonnée) pour la rampe côtière.
dist = np.full(a.shape, RAMPE_PX, dtype=np.float64)
front = mer.copy()
for k in range(RAMPE_PX):
    nouveau = voisins(front) & ~front
    dist[nouveau] = k + 1
    front |= nouveau
dist[mer] = 0

# Terre : gris recentré, débruité, courbe.
g = flou_masque(a, sol, 1.5)
x = np.clip((g - GRIS_COTE) / (1.0 - GRIS_COTE), 0.0, 1.0)
mediane = np.clip((np.median(a[sol]) - GRIS_COTE) / (1.0 - GRIS_COTE), 1e-3, 0.999)
p = np.log((PLAINE_M - 2.0) / (SOMMET - 2.0)) / np.log(mediane)
h = 2.0 + (SOMMET - 2.0) * x**p
t = np.clip(dist / RAMPE_PX, 0.0, 1.0)
h = 2.0 + (h - 2.0) * (t * t * (3 - 2 * t))

# Eaux intérieures : 3 m sous la terre voisine lissée.
h_sol = flou_masque(h, sol, 4.0)
h = np.where(eau, np.maximum(h_sol - 3.0, 0.5), h)
h = np.where(mer, FOND_MER, h)

# Encodage racine carrée sur [FOND_MER, SOMMET].
n = np.clip((h - FOND_MER) / (SOMMET - FOND_MER), 0.0, 1.0)
s = np.sqrt(n)
Image.fromarray(np.round(s * 255).astype(np.uint8), "L").save(SORTIE)

q = np.quantile(h[sol], [0.05, 0.25, 0.5, 0.75, 0.95, 0.99, 1.0]).round(0)
print(f"courbe interne p = {p:.2f} ; terre (m) q05..max : {q}")
print(f"eaux intérieures : {eau.sum()} px, altitude médiane {np.median(h[eau]):.0f} m")
print(f"Réglages éditeur : Noir {FOND_MER:.0f} m, Blanc {SOMMET:.0f} m, gris du noir 0, gris du blanc 1, courbe 2.0")
