"""Copie dans l'éditeur les textures Poly Haven (CC0) réellement utilisées, réduites et converties en PNG.

Source : bibliothèque locale (`manifest.json`, 2k JPG). Bevy n'est compilé qu'avec le PNG (pas le JPEG) : convertir
évite de rebâtir tout le moteur. Couleur en 1024 px, normales et ARM (AO / rugosité / métal) en 512 px.

Usage : python -I tools/textures_polyhaven.py [dossier_bibliotheque]
"""

import json
import sys
from pathlib import Path

from PIL import Image

BIBLIO = Path(sys.argv[1] if len(sys.argv) > 1 else "I:/game_dev/assets/MCF-Rp/materials/polyhaven")
CIBLE = Path(__file__).resolve().parent.parent / "crates" / "mcf_editor" / "assets" / "textures"

# Rôle dans l'éditeur → slug Poly Haven. Garder en accord avec `conception/textures.rs`.
CHOIX = {
    "herbe": "leafy_grass",
    "terre": "brown_mud_dry",
    "asphalte": "clean_asphalt",
    "trottoir": "concrete_pavement",
    "facade": "plastered_wall",
    "toit": "bitumen",
}
TAILLES = {"diff": 1024, "nor_gl": 512, "arm": 512}


def main() -> None:
    manifeste = json.loads((BIBLIO / "manifest.json").read_text(encoding="utf-8"))["materials"]
    par_slug = {m["slug"]: m for m in manifeste} if isinstance(manifeste, list) else manifeste
    credits = []
    for role, slug in CHOIX.items():
        m = par_slug[slug]
        dossier = CIBLE / role
        dossier.mkdir(parents=True, exist_ok=True)
        for carte, taille in TAILLES.items():
            image = Image.open(BIBLIO / m["files"][carte]).convert("RGB")
            image = image.resize((taille, taille), Image.Resampling.LANCZOS)
            image.save(dossier / f"{carte}.png", optimize=True)
        credits.append(f"| {role} | {m.get('name', slug)} | {m['page']} |")
    (CIBLE / "SOURCES.md").write_text(
        "# Textures de l'éditeur\n\nPoly Haven, licence CC0 (domaine public). Générées par "
        "`tools/textures_polyhaven.py` : ne pas modifier à la main.\n\n| Rôle | Matériau | Page |\n|---|---|---|\n"
        + "\n".join(credits)
        + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
