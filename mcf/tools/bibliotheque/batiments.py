"""Bâtiments de la bibliothèque → catalogue des bâtiments modélisés de l'éditeur (`assets/batiments/catalogue.json`).

Pour chaque bâtiment choisi (liste `CHOIX`) : copie le modèle de `assets/bibliotheque/` vers `assets/batiments/` (un
`.gltf` est empaqueté en `.glb`, ses textures restent à côté avec le même chemin relatif), **mesure** son encombrement
(boîte des maillages visibles, maillages de collision `-col` exclus, transformations des nœuds appliquées) et écrit sa
fiche. Les fiches existantes gardent leur identifiant (des bâtiments posés y renvoient).

Usage : python mcf/tools/bibliotheque/batiments.py
"""

from __future__ import annotations

import json
import math
import shutil
import struct
import sys
from pathlib import Path, PurePosixPath

ASSETS = Path(__file__).resolve().parents[2] / "crates" / "mcf_editor" / "assets"
BIBLIO = ASSETS / "bibliotheque"
DEST = ASSETS / "batiments"
CATALOGUE = DEST / "catalogue.json"

# (identifiant, nom, famille, fichier dans la bibliothèque, fichier dans batiments/)
CHOIX = [
    ("maison_a", "Maison A", "residentiel", "props/11_Batiments/Houses/House_A.glb", "11_Batiments/Houses/House_A.glb"),
    ("maison_b", "Maison B", "residentiel", "props/11_Batiments/Houses/House_B.glb", "11_Batiments/Houses/House_B.glb"),
    ("maison_c", "Maison C", "residentiel", "props/11_Batiments/Houses/House_C.glb", "11_Batiments/Houses/House_C.glb"),
    ("superette", "Supérette", "commerce", "props/11_Batiments/Superette/Superette.glb", "11_Batiments/Superette/Superette.glb"),
    ("station_service", "Station-service", "commerce", "props/11_Batiments/GasStation/GasStation.glb", "11_Batiments/GasStation/GasStation.glb"),
    ("banque", "Banque", "services", "props/11_Batiments/Bank/BankCentral.glb", "11_Batiments/Bank/BankCentral.glb"),
    ("usine", "Usine", "activite", "props/11_Batiments/Factory/Factory.glb", "11_Batiments/Factory/Factory.glb"),
    ("ferme_moderne", "Ferme moderne", "activite", "props/11_Batiments/Farms/Farm_Modern.glb", "11_Batiments/Farms/Farm_Modern.glb"),
    ("ferme_traditionnelle", "Ferme traditionnelle", "activite", "props/11_Batiments/Farms/Farm_Traditional.glb", "11_Batiments/Farms/Farm_Traditional.glb"),
    ("garage", "Garage mécanique", "activite", "props/15_Garage/Garage/Workshop_Garage.glb", "15_Garage/Garage/Workshop_Garage.glb"),
    ("cite_barre", "Barre d'immeubles", "residentiel", "props/14_Cite/Barre/Cite_Barre.glb", "14_Cite/Barre/Cite_Barre.glb"),
    ("cite_tour", "Tour de la cité", "residentiel", "props/14_Cite/Tour/Cite_Tour.glb", "14_Cite/Tour/Cite_Tour.glb"),
    ("cite_commerces", "Commerces de la cité", "commerce", "props/14_Cite/Commerces/Cite_Commerces.glb", "14_Cite/Commerces/Cite_Commerces.glb"),
    ("hangar_ouvert", "Hangar ouvert", "activite", "props/10_Elements_Structurels/Hangars/HangarOpen8x12.glb", "10_Elements_Structurels/Hangars/HangarOpen8x12.glb"),
    ("immeuble_petit", "Petit immeuble", "residentiel", "buildings/Immeuble_Petit.gltf", "buildings/Immeuble_Petit.glb"),
    ("immeuble_moyen", "Immeuble moyen", "residentiel", "buildings/Immeuble_Moyen.gltf", "buildings/Immeuble_Moyen.glb"),
    ("immeuble_grand", "Grand immeuble", "residentiel", "buildings/Immeuble_Grand.gltf", "buildings/Immeuble_Grand.glb"),
    ("tour_appartements", "Tour d'appartements", "residentiel", "buildings/Tour_Appartements.gltf", "buildings/Tour_Appartements.glb"),
    ("tour_bureaux", "Tour de bureaux", "commerce", "buildings/Tour_Bureaux.gltf", "buildings/Tour_Bureaux.glb"),
    ("hotel_affaires", "Hôtel d'affaires", "commerce", "buildings/Hotel_Affaires.gltf", "buildings/Hotel_Affaires.glb"),
    ("caserne", "Caserne de pompiers", "services", "buildings/Caserne.gltf", "buildings/Caserne.glb"),
    ("commissariat", "Commissariat", "services", "buildings/Commissariat.gltf", "buildings/Commissariat.glb"),
    ("hopital", "Hôpital", "services", "buildings/Hopital.gltf", "buildings/Hopital.glb"),
]


def lire(chemin: Path) -> tuple[dict, bytes | None]:
    if chemin.suffix == ".gltf":
        doc = json.loads(chemin.read_text(encoding="utf-8"))
        buf = doc["buffers"][0]
        return doc, (chemin.parent / buf["uri"]).read_bytes() if "uri" in buf else None
    data = chemin.read_bytes()
    jlen, _ = struct.unpack_from("<II", data, 12)
    doc = json.loads(data[20:20 + jlen].decode("utf-8"))
    off = 20 + jlen
    blen = struct.unpack_from("<I", data, off)[0] if off < len(data) else 0
    return doc, data[off + 8:off + 8 + blen] if blen else None


def ecrire_glb(doc: dict, binaire: bytes, chemin: Path) -> None:
    doc = json.loads(json.dumps(doc))
    doc["buffers"] = [{"byteLength": len(binaire)}]
    j = json.dumps(doc, separators=(",", ":")).encode("utf-8")
    j += b" " * (-len(j) % 4)
    b = binaire + b"\0" * (-len(binaire) % 4)
    total = 12 + 8 + len(j) + 8 + len(b)
    chemin.write_bytes(
        struct.pack("<III", 0x46546C67, 2, total) + struct.pack("<II", len(j), 0x4E4F534A) + j
        + struct.pack("<II", len(b), 0x004E4942) + b
    )


def mat_noeud(n: dict) -> list[list[float]]:
    if "matrix" in n:
        m = n["matrix"]
        return [[m[c * 4 + r] for c in range(4)] for r in range(4)]
    t = n.get("translation", [0, 0, 0])
    x, y, z, w = n.get("rotation", [0, 0, 0, 1])
    s = n.get("scale", [1, 1, 1])
    r = [
        [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
        [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
    ]
    return [[r[i][0] * s[0], r[i][1] * s[1], r[i][2] * s[2], t[i]] for i in range(3)] + [[0, 0, 0, 1]]


def mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def mesurer(doc: dict) -> tuple[list[float], list[float]]:
    mn, mx = [math.inf] * 3, [-math.inf] * 3
    scene = doc["scenes"][doc.get("scene", 0)]

    def visiter(i: int, parent):
        n = doc["nodes"][i]
        if "-col" in n.get("name", ""):
            return
        m = mul(parent, mat_noeud(n))
        if "mesh" in n:
            for p in doc["meshes"][n["mesh"]]["primitives"]:
                a = doc["accessors"][p["attributes"]["POSITION"]]
                lo, hi = a["min"], a["max"]
                for cx in (lo[0], hi[0]):
                    for cy in (lo[1], hi[1]):
                        for cz in (lo[2], hi[2]):
                            v = [sum(m[r][k] * c for k, c in enumerate((cx, cy, cz, 1))) for r in range(3)]
                            for k in range(3):
                                mn[k] = min(mn[k], v[k])
                                mx[k] = max(mx[k], v[k])
        for c in n.get("children", []):
            visiter(c, m)

    ident = [[1 if i == j else 0 for j in range(4)] for i in range(4)]
    for r in scene["nodes"]:
        visiter(r, ident)
    return mn, mx


def main() -> int:
    ancien = json.loads(CATALOGUE.read_text(encoding="utf-8")) if CATALOGUE.exists() else {"fiches": []}
    fiches = []
    for fid, nom, famille, src_rel, dst_rel in CHOIX:
        src, dst = BIBLIO / src_rel, DEST / dst_rel
        if not src.is_file():
            print(f"absent : {src_rel}", file=sys.stderr)
            continue
        doc, binaire = lire(src)
        dst.parent.mkdir(parents=True, exist_ok=True)
        if src.suffix == ".gltf":
            ecrire_glb(doc, binaire or b"", dst)
        else:
            shutil.copy2(src, dst)
        # Textures externes : même chemin relatif depuis le nouveau dossier.
        for img in doc.get("images", []):
            u = img.get("uri")
            if not u or u.startswith("data:"):
                continue
            a = (src.parent / PurePosixPath(u)).resolve()
            b = (dst.parent / PurePosixPath(u)).resolve()
            if DEST.resolve() not in b.parents:
                print(f"{fid} : texture hors du dossier ({u})", file=sys.stderr)
                continue
            b.parent.mkdir(parents=True, exist_ok=True)
            if not b.exists():
                shutil.copy2(a, b)
        mn, mx = mesurer(doc)
        fiches.append(
            {
                "id": fid,
                "nom": nom,
                "famille": famille,
                "fichier": dst_rel,
                "largeur_m": round(mx[0] - mn[0], 2),
                "profondeur_m": round(mx[2] - mn[2], 2),
                "hauteur_m": round(mx[1] - max(mn[1], 0.0), 2),
                "centre_m": [round((mn[0] + mx[0]) / 2, 2), round((mn[2] + mx[2]) / 2, 2)],
            }
        )
        f = fiches[-1]
        print(f"{fid:<22} {f['largeur_m']:6.1f} × {f['profondeur_m']:6.1f} × {f['hauteur_m']:5.1f} m")
    garder = {f["id"] for f in fiches}
    for f in ancien.get("fiches", []):
        if f["id"] not in garder:
            fiches.append(f)
    sortie = {"format": "mcfcatalogue_batiments", "version": 1, "fiches": fiches}
    CATALOGUE.write_text(json.dumps(sortie, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(fiches)} fiche(s) : {CATALOGUE}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
