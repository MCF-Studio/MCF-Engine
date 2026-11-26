"""Importe la bibliothèque d'assets 3D du jeu (projet Godot mcf-2d-rp-) dans l'éditeur MCF, sans les véhicules.

- Copie les modèles (.glb, .gltf + .bin) et leurs textures dans `mcf/crates/mcf_editor/assets/bibliotheque/`
  (non versionnés, comme les bâtiments modélisés), en gardant l'arborescence : les liens relatifs des modèles vers
  leurs textures partagées (`props/_textures/`) restent valides. Les fichiers `.import` de Godot sont ignorés.
- Écrit le catalogue versionné `bibliotheque.json` : sections → familles → objets, noms en français (sections et
  familles traduites à la main, variantes traduites mot à mot), mots-clés pour la recherche (français + nom anglais
  du fichier).
- Refuse un modèle dont un lien (`uri`) sortirait de la bibliothèque, ou qui pointe vers une donnée intégrée en
  `data:` trop grosse : les modèles restent des données non fiables, l'éditeur revalide de toute façon.

Usage : python mcf/tools/bibliotheque/importer.py [--src <resources/models>] [--dest <dossier>] [--dry-run]
Relançable : recopie les fichiers modifiés, réécrit le catalogue. Mots non traduits : listés à la fin.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import struct
import sys
from collections import Counter
from pathlib import Path, PurePosixPath

DEPOT = Path(__file__).resolve().parents[3]
SRC = Path("I:/game_dev/Godot/mcf-2d-rp-/resources/models")
DEST = DEPOT / "mcf" / "crates" / "mcf_editor" / "assets" / "bibliotheque"
CATALOGUE = DEST.parent / "bibliotheque.json"
NOMS = {k: v for k, v in json.loads((Path(__file__).with_name("noms.json")).read_text(encoding="utf-8")).items() if not k.startswith("_")}

# Véhicules : jamais importés (demande d'Aroun), y compris ceux rangés dans le décor.
EXCLUS_DOSSIERS = {"vehicles", "towers"}
EXCLUS_FICHIERS = {
    "BicycleParked", "ScooterParked", "CarWreckBurnt", "Kayak", "Pedalo", "MobileCrane",
}
MODELES = {".glb", ".gltf"}
ANNEXES = {".bin", ".png", ".jpg", ".jpeg", ".webp", ".ktx2"}

SECTIONS = {
    "01_Barrieres_Clotures_Murs": ("barrieres", "Clôtures, murs et barrières", "▦"),
    "02_Signalisation_Voirie": ("signalisation", "Signalisation et voirie", "⚠"),
    "03_Mobilier_Urbain": ("mobilier", "Mobilier urbain", "🏪"),
    "04_Terrasses_Commerces_Habitation": ("terrasses", "Terrasses, commerces et habitat", "☂"),
    "05_Industrie_Chantier_Port": ("industrie", "Industrie, chantier et port", "⚙"),
    "06_Ordre_Secours_Zones": ("secours", "Ordre public, secours et zones", "🚨"),
    "07_Campagne_Nature": ("nature", "Campagne et nature", "🌲"),
    "08_Plage_Loisirs": ("loisirs", "Plage et loisirs", "☀"),
    "09_Clutter": ("divers", "Objets divers et encombrants", "📦"),
    "10_Elements_Structurels": ("structure", "Éléments structurels", "▣"),
    "11_Batiments": ("batiments", "Bâtiments et lieux", "🏠"),
    "13_Aeroport": ("aeroport", "Aéroport", "✈"),
    "14_Cite": ("cite", "Cité", "🏢"),
    "15_Garage": ("batiments", "Bâtiments et lieux", "🏠"),
    # Hors du dossier props.
    "buildings": ("batiments", "Bâtiments et lieux", "🏠"),
    "elevator": ("batiments", "Bâtiments et lieux", "🏠"),
    "city_kit": ("kit_ville", "Kit de façades et de rues", "🔧"),
    "characters": ("personnages", "Personnages, animaux et équipement", "👤"),
    "animals": ("personnages", "Personnages, animaux et équipement", "👤"),
    "weapons": ("personnages", "Personnages, animaux et équipement", "👤"),
    "handheld": ("personnages", "Personnages, animaux et équipement", "👤"),
    "worn": ("personnages", "Personnages, animaux et équipement", "👤"),
}
ORDRE_SECTIONS = [
    "batiments", "mobilier", "signalisation", "barrieres", "nature", "terrasses", "industrie", "secours",
    "loisirs", "aeroport", "cite", "structure", "divers", "kit_ville", "personnages",
]

FAMILLES = {
    # 01
    "FenceBamboo": "Clôture en bambou", "FenceBarbedWire": "Clôture barbelée", "FenceChainlink": "Clôture grillagée",
    "FenceFarm": "Clôture de ferme", "FenceGardenWood": "Clôture de jardin en bois", "FenceHeras": "Clôture de chantier (Heras)",
    "FenceResidence": "Grille de résidence", "FenceRigidPanel": "Panneau grillagé rigide", "FenceSection_Kit": "Clôture (kit)",
    "FenceSecurityHigh": "Clôture haute sécurité", "PalisadeSection_Kit": "Palissade (kit)",
    "Gabion": "Gabions", "WallBlockPlaster": "Mur parpaing crépi", "WallBrick": "Mur de briques",
    "WallDryStoneCoastal": "Muret en pierres sèches", "WallEnclosureConcrete": "Mur d'enceinte en béton",
    "WallGardenLow": "Muret de jardin", "WallNoiseBarrier": "Mur antibruit", "WallRetainingConcrete": "Mur de soutènement",
    "WallStoneLow": "Muret de pierre", "BarrierBoom": "Barrière levante", "BarrierK2": "Barrière de chantier K2",
    "BarrierTape": "Rubalise", "BarrierVauban": "Barrière Vauban", "BarrierWaterFilled": "Plot remplissable",
    "Bollard": "Bornes et potelets", "ChevronBeacon": "Balise de virage", "ConcreteBarrierJersey": "Séparateur béton (GBA)",
    "ConcreteBlock": "Bloc béton", "CzechHedgehog": "Hérisson tchèque", "GuardBooth": "Guérite", "Guardrail": "Glissière de sécurité",
    "HescoBastion": "Bastion Hesco", "LevelCrossing": "Passage à niveau", "Sandbags": "Sacs de sable",
    "SpikeStrip": "Herse", "TrafficCone": "Cônes de chantier",
    # 02
    "Billboards": "Panneaux publicitaires", "BusShelter": "Abribus", "Delineators": "Délinéateurs",
    "DirectionSigns": "Panneaux directionnels", "FireHydrant": "Bouches incendie", "Gantry": "Portique",
    "Manholes": "Plaques d'égout", "ParkingMeter": "Horodateurs", "RoadMarkings": "Marquages au sol",
    "RoadSigns": "Panneaux routiers", "SpeedBumps": "Ralentisseurs", "SpeedCameras": "Radars",
    "StreetLamps": "Lampadaires", "StreetMirror": "Miroir de rue", "TollBooth": "Péage", "TrafficLights": "Feux tricolores",
    "VariableMessageSign": "Panneau à message variable",
    # 03
    "Benches": "Bancs", "BikeRacks": "Arceaux à vélos", "EVChargers": "Bornes de recharge", "FlagPoles": "Mâts de drapeau",
    "Fountains": "Fontaines", "Kiosks": "Kiosques", "Mailboxes": "Boîtes aux lettres", "Monuments": "Monuments",
    "Pergolas": "Pergolas", "PhoneBooths": "Cabines téléphoniques", "Planters": "Jardinières", "PublicToilets": "Toilettes publiques",
    "SecurityCameras": "Caméras de surveillance", "SidewalkBarriers": "Barrières de trottoir", "StreetBins": "Poubelles de rue",
    "StreetClock": "Horloge de rue", "VendingMachines": "Distributeurs", "WasteContainers": "Conteneurs à déchets",
    # 04
    "AirConditioners": "Climatiseurs", "Antennas": "Antennes", "Awnings": "Stores", "Barbecues": "Barbecues",
    "Barber": "Salon de coiffure", "Firewood": "Bois de chauffage", "Garden": "Jardin", "GardenFurniture": "Salon de jardin",
    "GardenSheds": "Abris de jardin", "Neons": "Néons", "Parasols": "Parasols", "Playground": "Aire de jeux",
    "Pools": "Piscines", "PowerBoxes": "Coffrets électriques", "Railings": "Garde-corps", "Ramps": "Rampes",
    "ShopSigns": "Enseignes", "StairsExterior": "Escaliers extérieurs", "TerraceFurniture": "Mobilier de terrasse",
    # 05
    "CableReels": "Tourets de câble", "ConstructionEquipment": "Matériel de chantier", "Cranes": "Grues",
    "Crates": "Caisses", "Drums": "Fûts et cuves", "Electrical": "Électricité", "MaterialPiles": "Tas de matériaux",
    "Mine": "Mine", "Pallets": "Palettes", "Pipes": "Buses et tuyaux", "Pontoon": "Ponton", "Port": "Port",
    "Scaffolding": "Échafaudage", "ShippingContainers": "Conteneurs maritimes", "SiteCabins": "Bungalows de chantier",
    "StorageRacks": "Racks de stockage", "Tanks": "Citernes", "Tires": "Pneus",
    # 06
    "Medical": "Secours", "PortableLights": "Éclairages mobiles", "Safety": "Sécurité incendie", "Security": "Sécurité",
    "ShootingRange": "Stand de tir", "Tents": "Tentes", "WarningSigns": "Panneaux d'avertissement", "Watchtowers": "Miradors",
    # 07
    "Beach": "Plage", "Bushes": "Buissons", "Farm": "Ferme", "Flowers": "Fleurs", "Greenhouses": "Serres",
    "GroundCover": "Herbes et couvre-sol", "HayBales": "Bottes de foin", "Hedges": "Haies", "Rocks": "Rochers",
    "StumpsLogs": "Souches et troncs", "Succulents": "Plantes grasses", "Trees": "Arbres",
    # 08
    "Gazebos": "Barnums", "InfoPanels": "Panneaux d'information", "LakeBar": "Bar du lac", "Market": "Marché",
    "PicnicTables": "Tables de pique-nique", "Sports": "Sports", "Stage": "Scène", "Watercraft": "Plage (flotteurs)",
    # 09
    "ArtFrames": "Cadres", "Cardboard": "Cartons", "Chests": "Coffres", "Decals": "Décals", "Dumped": "Encombrants",
    "Litter": "Déchets", "Props": "Petits objets", "Tarps": "Bâches", "TrashBags": "Sacs poubelle",
    "Vehicles_Decor": "Chariots", "Workstations": "Postes de travail", "Wrecks": "Épaves et gravats",
    # 10
    "Carports": "Carports", "ConcreteSlabs": "Dalles béton", "Curbs": "Bordures", "Footbridges": "Passerelles",
    "Hangars": "Hangars", "Paving": "Pavés", "Pillars": "Piliers", "PropertyGates": "Portails",
    "Shelters": "Préaux", "Steps": "Marches", "Tunnels": "Tunnels",
    # 11, 13, 14, 15
    "ATM": "Distributeur de billets", "Bank": "Banque", "Factory": "Usine", "Farms": "Fermes", "FuelPumps": "Pompes à essence",
    "GasStation": "Station-service", "Houses": "Maisons", "Superette": "Supérette",
    "Aeroclub": "Aéroclub", "AirfieldTower": "Tour d'aérodrome", "Apron": "Aire de stationnement", "ControlTower": "Tour de contrôle",
    "Hangar": "Hangar", "Runway": "Pistes", "Taxiway": "Voies de circulation", "Terminal": "Aérogare", "Windsock": "Manche à air",
    "Barre": "Barre d'immeubles", "CityStade": "City-stade", "Commerces": "Commerces", "Signs": "Panneaux", "Tour": "Tour",
    "Garage": "Garage",
    # hors props
    "buildings": "Bâtiments publics et immeubles", "elevator": "Ascenseur",
    "characters": "Personnages", "clothes": "Vêtements", "robes": "Robes", "chiens": "Chiens",
    "weapons": "Armes", "attachments": "Accessoires d'armes", "handheld": "Objets en main", "worn": "Objets portés",
}

# Kit de façades : familles par préfixe de fichier.
KIT = [
    ("Brick", "Façade en briques"), ("Cornice", "Corniches"), ("Decal", "Marquages au sol"), ("Door", "Portes"),
    ("Entrance", "Entrées"), ("Floor", "Sols"), ("Metal", "Structure métallique"), ("Window", "Fenêtres"),
    ("Building", "Immeubles assemblés"), ("Sidewalk", "Trottoirs"), ("Road", "Chaussée"), ("Roof", "Toits"),
    ("Street", "Rue"), ("Concrete", "Béton"), ("Plaster", "Façade enduite"), ("Wall", "Murs"),
]

MOTS = {
    # états, variantes
    "Debris": "débris", "Broken": "cassé", "Fallen": "tombé", "Overturned": "renversé", "Damaged": "abîmé",
    "Collapsed": "effondré", "Bent": "tordu", "Burnt": "brûlé", "Crushed": "écrasé", "Rusty": "rouillé", "Tagged": "tagué",
    "Old": "ancien", "Modern": "moderne", "Traditional": "traditionnel", "Closed": "fermé", "Open": "ouvert",
    "Opening": "ouverture", "Retracted": "rentré", "Extended": "déployé", "Deployed": "déployé", "Folded": "plié",
    "Lowered": "abaissé", "Rolled": "enroulé", "Stacked": "empilé", "Stack": "pile", "Group": "groupe", "Row": "rangée",
    "Set": "ensemble", "Full": "plein", "Empty": "vide", "Covered": "couvert", "Dead": "mort", "Flowering": "en fleurs",
    "noWear": "sans usure", "Wear": "usure", "Fixed": "fixe", "Removable": "amovible", "Retractable": "escamotable",
    "Automatic": "automatique", "Rotating": "rotatif", "Mobile": "mobile", "Portable": "portable", "Folding": "pliant",
    "Standalone": "isolé", "Freestanding": "sur pied", "Laid": "posé", "Standing": "debout", "Moving": "mobile",
    # tailles, formes, positions
    "Small": "petit", "Medium": "moyen", "Large": "grand", "Tall": "haut", "Low": "bas", "High": "haut", "Short": "court",
    "Long": "long", "Wide": "large", "Half": "demi", "Double": "double", "Single": "simple", "Triple": "triple",
    "Corner": "angle", "Angle": "angle", "Center": "centre", "Central": "central", "End": "fin", "Top": "haut",
    "Bottom": "bas", "Left": "gauche", "Right": "droite", "L": "gauche", "R": "droite", "Straight": "droit",
    "Curve": "courbe", "Curved": "courbe", "Bend": "courbe", "Round": "rond", "Square": "carré", "Rect": "rectangulaire",
    "Octagon": "octogonal", "Triangle": "triangle", "Hexagon": "hexagonal", "Herringbone": "chevrons",
    "Horizontal": "horizontal", "Vertical": "vertical", "Inner": "intérieur", "Interior": "intérieur", "Inset": "renfoncement",
    "Above": "au-dessus", "Under": "dessous", "Up": "haut", "Down": "bas", "Sloped": "en pente", "Flat": "plat",
    "Section": "section", "Post": "poteau", "Pole": "poteau", "Gate": "portail", "Panel": "panneau", "Piece": "pièce",
    "Module": "module", "Modular": "modulaire", "Kit": "kit", "Cap": "chapeau", "Plinth": "socle", "Base": "base",
    "First": "premier", "One": "un", "Way": "sens", "Only": "seulement", "No": "interdit", "With": "avec", "On": "allumé",
    "Off": "éteint", "Blank": "vierge", "Plain": "uni", "Patch": "raccord", "Trim": "moulure", "Ornament": "ornement",
    # matériaux, couleurs
    "Concrete": "béton", "Metal": "métal", "Steel": "acier", "Iron": "fer", "Wood": "bois", "Wooden": "bois", "Stone": "pierre",
    "Brick": "briques", "Bricks": "briques", "Plaster": "enduit", "Glass": "verre", "Plastic": "plastique", "Slate": "ardoise",
    "Asphalt": "asphalte", "Gravel": "gravier", "Sand": "sable", "Dirt": "terre", "Soil": "terre", "Mesh": "grillage",
    "Red": "rouge", "White": "blanc", "Green": "vert", "Blue": "bleu", "Yellow": "jaune", "Orange": "orange",
    "Grey": "gris", "Black": "noir", "Brown": "marron", "Beige": "beige", "Amber": "orange", "RedBricks": "briques rouges",
    "RedWhite": "rouge et blanc", "TrimBricks": "briques de moulure",
    # objets
    "Wall": "mur", "Fence": "clôture", "Barrier": "barrière", "Sign": "panneau", "Signs": "panneaux", "Road": "route",
    "Street": "rue", "Lane": "voie", "Sidewalk": "trottoir", "Curb": "bordure", "Column": "colonne", "Pillar": "pilier",
    "Cornice": "corniche", "Window": "fenêtre", "Windows": "fenêtres", "Door": "porte", "DoorFrame": "encadrement de porte",
    "Frame": "cadre", "Roof": "toit", "Floor": "sol", "Stairs": "escalier", "Stair": "escalier", "Steps": "marches",
    "Railing": "garde-corps", "Rail": "rail", "Rails": "rails", "Ramp": "rampe", "Entrance": "entrée", "Building": "immeuble",
    "Tree": "arbre", "Bush": "buisson", "Shrub": "arbuste", "Hedge": "haie", "Rock": "rocher", "Cliff": "falaise",
    "Grass": "herbe", "Tuft": "touffe", "Fern": "fougère", "Reeds": "roseaux", "Flower": "fleur", "Palm": "palmier",
    "Pine": "pin", "Oak": "chêne", "Olive": "olivier", "Apple": "pommier", "Fruit": "fruitier", "Cactus": "cactus",
    "Agave": "agave", "Stump": "souche", "Log": "tronc", "Branches": "branches", "Driftwood": "bois flotté",
    "Seaweed": "algues", "Pebbles": "galets", "Shells": "coquillages", "Hay": "foin", "Bale": "botte",
    "Light": "lumière", "Lamp": "lampe", "Traffic": "circulation", "Speed": "vitesse", "Stop": "stop", "Yield": "cédez",
    "Arrow": "flèche", "Forward": "tout droit", "Turn": "tourner", "Crosswalk": "passage piéton", "Bikelane": "piste cyclable",
    "Line": "ligne", "Stripe": "bande", "BrokenLine": "ligne discontinue", "DoubleYellow": "double ligne jaune", "Slow": "ralentir",
    "Parking": "parking", "Bus": "bus", "Shelter": "abri", "Booth": "cabine", "Camera": "caméra", "Bench": "banc",
    "Bin": "poubelle", "Container": "conteneur", "Crate": "caisse", "Box": "boîte", "Boxes": "boîtes", "Pallet": "palette",
    "Pallets": "palettes", "Drum": "fût", "Barrel": "tonneau", "Tank": "citerne", "Tire": "pneu", "Tires": "pneus",
    "Pipe": "tuyau", "Cable": "câble", "Cables": "câbles", "Reel": "touret", "Scaffold": "échafaudage", "Cabin": "bungalow",
    "Rack": "rack", "Shipping": "maritime", "Pile": "tas", "Bag": "sac", "Sacks": "sacs", "Sandbag": "sac de sable",
    "Tarp": "bâche", "Cardboard": "carton", "Litter": "déchets", "Trash": "poubelle", "Rubble": "gravats",
    "Table": "table", "Chair": "chaise", "Chairs": "chaises", "Stool": "tabouret", "Sofa": "canapé", "Cushion": "coussin",
    "Lounger": "transat", "Deckchair": "chaise longue", "Hammock": "hamac", "Parasol": "parasol", "Umbrella": "parasol",
    "Awning": "store", "Neon": "néon", "Lightbox": "caisson lumineux", "Letters": "lettres", "Shop": "boutique",
    "Terrace": "terrasse", "Garden": "jardin", "Shed": "abri", "Pool": "piscine", "Playground": "aire de jeux",
    "Swing": "balançoire", "Slide": "toboggan", "Sandbox": "bac à sable", "Spring": "jeu à ressort", "Rider": "",
    "Basketball": "basket", "Football": "football", "Volleyball": "volley", "Goal": "but", "Net": "filet", "Court": "terrain",
    "Stage": "scène", "Market": "marché", "Stall": "étal", "Gazebo": "barnum", "Picnic": "pique-nique",
    "Beach": "plage", "Towel": "serviette", "Surf": "surf", "Board": "planche", "Lifeguard": "poste de secours",
    "Tent": "tente", "Watchtower": "mirador", "Tower": "tour", "Warning": "avertissement", "Restricted": "accès restreint",
    "Area": "zone", "Zone": "zone", "Police": "police", "Military": "militaire", "Medical": "médical", "Aid": "secours",
    "Stretcher": "brancard", "Defibrillator": "défibrillateur", "Extinguisher": "extincteur", "Hose": "lance",
    "Fire": "incendie", "Hydrant": "bouche", "Shooting": "tir", "Range": "stand", "Target": "cible", "Detector": "détecteur",
    "Floodlight": "projecteur", "Tripod": "trépied", "Generator": "groupe électrogène", "Compressor": "compresseur",
    "Mixer": "bétonnière", "Crane": "grue", "Transformer": "transformateur", "Substation": "poste électrique",
    "Pylon": "pylône", "Power": "électrique", "Electric": "électrique", "Mast": "mât", "Antenna": "antenne",
    "TVAntenna": "antenne TV", "Satellite": "satellite", "Dish": "parabole", "ACUnit": "climatiseur", "Unit": "groupe",
    "Fan": "ventilateur", "Vending": "distributeur", "Machine": "machine", "Drinks": "boissons", "Snacks": "en-cas",
    "Phone": "téléphone", "Mailbox": "boîte aux lettres", "Kiosk": "kiosque", "News": "journaux", "Clock": "horloge",
    "Fountain": "fontaine", "Drinking": "à boire", "Statue": "statue", "Bust": "buste", "Memorial": "monument aux morts",
    "Commemorative": "commémoratif", "Plaque": "plaque", "War": "guerre", "Flag": "drapeau", "Planter": "jardinière",
    "Pot": "pot", "Toilet": "toilettes", "Public": "public", "Bollard": "borne", "Bike": "vélo", "Hoop": "arceau",
    "EVCharger": "borne de recharge", "Charger": "chargeur", "Billboard": "panneau publicitaire", "Poster": "affiche",
    "Banner": "banderole", "Mupi": "MUPI", "Morris": "colonne Morris", "Info": "information", "Map": "plan",
    "Delineator": "délinéateur", "Direction": "direction", "Gantry": "portique", "Manhole": "plaque d'égout",
    "Grate": "grille", "Drain": "avaloir", "Meter": "horodateur", "Bump": "dos-d'âne", "Hump": "dos-d'âne",
    "Toll": "péage", "Mirror": "miroir", "Variable": "variable", "Message": "message", "Milestone": "borne kilométrique",
    "Highway": "autoroute", "Pedestrian": "piétons", "Disabled": "handicapés", "PMR": "PMR", "Cross": "croix",
    "Chevron": "chevron", "Beacon": "balise", "Cone": "cône", "Tape": "ruban", "Spike": "herse", "Strip": "bande",
    "Hesco": "Hesco", "Bastion": "bastion", "Jersey": "GBA", "Vauban": "Vauban", "Water": "eau", "Filled": "lestée",
    "Boom": "barrière levante", "Arm": "bras", "Guardrail": "glissière", "Guard": "garde", "Level": "niveau",
    "Crossing": "passage", "Barbed": "barbelé", "Wire": "fil", "Concertina": "concertina", "Chainlink": "grillage",
    "Heras": "Heras", "Residence": "résidence", "Rigid": "rigide", "Bamboo": "bambou", "Palisade": "palissade",
    "Gabion": "gabion", "Retaining": "soutènement", "Enclosure": "enceinte", "Noise": "antibruit", "Coastal": "côtier",
    "Dry": "sèches", "Block": "bloc", "Blocks": "blocs", "Cube": "cube", "Lego": "type Lego", "Czech": "tchèque",
    "Hedgehog": "hérisson", "Farm": "ferme", "Barn": "grange", "Silo": "silo", "Trough": "abreuvoir", "Plough": "charrue",
    "Scarecrow": "épouvantail", "Windpump": "éolienne de pompage", "Mill": "moulin", "Well": "puits", "Beehive": "ruche",
    "Chicken": "poulailler", "Coop": "", "Doghouse": "niche", "Dog": "chien", "Compost": "composteur", "Crop": "culture",
    "Wheat": "blé", "Beet": "betterave", "Cotton": "coton", "Vegetable": "potager", "Greenhouse": "serre",
    "Churn": "bidon de lait", "Grain": "grain", "Sugar": "sucre", "Clothesline": "étendoir", "Firewood": "bois",
    "BBQ": "barbecue", "Charcoal": "charbon", "Kitchen": "cuisine", "Stove": "réchaud", "Oven": "four", "Fridge": "réfrigérateur",
    "Counter": "comptoir", "Desk": "bureau", "Office": "bureau", "PC": "ordinateur", "Workbench": "établi", "Sewing": "couture",
    "Loom": "métier à tisser", "Lab": "laboratoire", "Still": "alambic", "Reception": "accueil", "Shower": "douche",
    "Shampoo": "bac à shampoing", "Barber": "coiffeur", "Laundry": "laverie", "Mattress": "matelas", "Bed": "lit",
    "Chest": "coffre", "Lid": "couvercle", "Ladder": "échelle", "Broom": "balai", "Bucket": "seau", "Wheelbarrow": "brouette",
    "Beer": "bière", "Shopping": "courses", "Cart": "chariot", "Bottle": "bouteille", "Gas": "gaz", "Can": "bidon",
    "Jerry": "jerrican", "Oil": "huile", "Art": "art", "Easel": "chevalet", "Portrait": "portrait", "Landscape": "paysage",
    "Graffiti": "graffiti", "Tag": "tag", "Stain": "tache", "Urine": "urine", "Droppings": "fientes", "Pigeon": "pigeon",
    "Cracks": "fissures", "Dumped": "abandonné", "Wreck": "épave", "Car": "voiture", "Truck": "camion", "Trailer": "remorque",
    "Pontoon": "ponton", "Mooring": "amarrage", "Buoy": "bouée", "Quay": "quai", "Fender": "défense", "Port": "port",
    "Mine": "mine", "Adit": "galerie", "Minecart": "wagonnet", "Ore": "minerai", "Sulfur": "soufre", "Foundry": "fonderie",
    "Refinery": "raffinerie", "Powder": "poudre", "Factory": "usine", "Workshop": "atelier", "Garage": "garage",
    "Carport": "carport", "Slab": "dalle", "Paving": "pavés", "Footbridge": "passerelle", "Tunnel": "tunnel",
    "Culvert": "buse", "Hangar": "hangar", "Truss": "treillis", "Beams": "poutres", "Planks": "planches", "Plank": "planche",
    "Slat": "latte", "Sheet": "tôle", "Property": "propriété", "Sliding": "coulissant", "Cantilever": "autoportant",
    "Airport": "aéroport", "Airfield": "aérodrome", "Runway": "piste", "Taxiway": "voie de circulation", "Apron": "aire",
    "Terminal": "aérogare", "Control": "contrôle", "Windsock": "manche à air", "Aeroclub": "aéroclub",
    "House": "maison", "Bank": "banque", "Superette": "supérette", "Fuel": "carburant", "Pump": "pompe", "Station": "station",
    "ATM": "distributeur", "Cite": "cité", "Barre": "barre", "Tour": "tour", "City": "city", "Stade": "stade",
    "Stadium": "stade", "Commerces": "commerces", "Epicerie": "épicerie", "Boulangerie": "boulangerie", "Pharmacie": "pharmacie",
    "Pharmacy": "pharmacie", "Immobilier": "agence immobilière", "Soldes": "soldes", "Food": "restauration", "Hot": "chaud",
    "Motel": "motel", "Hotel": "hôtel", "Hospital": "hôpital", "Prison": "prison", "Concert": "concert", "Festival": "festival",
    "Bandstand": "kiosque à musique", "Band": "", "Speaker": "enceinte", "Wanted": "avis de recherche", "Name": "nom",
    "Logo": "logo", "Decor": "décor", "Decal": "marquage", "Euro": "euro", "Wheelie": "à roulettes", "Recycling": "recyclage",
    "Sorting": "tri", "Skip": "benne", "Waste": "déchets", "Glass2m": "verre 2 m", "Dome": "dôme", "Exterior": "extérieur",
    "Escape": "de secours", "Service": "de service", "Entry": "entrée", "Space": "place", "Overhead": "suspendu",
    "Roadworks": "travaux", "Works": "travaux", "Roadside": "bord de route", "Seaside": "bord de mer", "Lake": "lac",
    "Park": "parc", "Mediterranean": "méditerranéen", "Berlin": "berlinois", "Security": "sécurité", "Safety": "sécurité",
    "Private": "privé", "Storm": "orage", "Sun": "soleil", "Mech": "mécanique", "Moyen": "moyen", "Petit": "petit",
    "Grand": "grand", "Test": "test", "Banque": "banque", "Centrale": "centrale", "Caserne": "caserne", "Remise": "remise",
    "Commissariat": "commissariat", "Hopital": "hôpital", "Hotel_Affaires": "hôtel d'affaires", "Affaires": "d'affaires",
    "Immeuble": "immeuble", "Bureaux": "bureaux", "Appartements": "appartements", "Plateau": "plateau", "Gun": "pistolet",
    "Intersection": "carrefour", "TIntersection": "carrefour en T", "Hatching": "hachures", "Channel": "caniveau",
    "Marker": "repère", "Clamp": "collier", "Butter": "beurre", "Black": "noir",
    "H": "h.", "Prop": "accessoire", "T": "T", "AC": "clim", "Bar": "bar", "Tube": "tube", "Head": "tête",
    "Backrest": "avec dossier", "Ground": "sol", "Cover": "couverture", "Foot": "pied", "Stand": "support",
    "Basket": "panier", "Cabinet": "armoire", "Columns": "colonnes", "Roll": "rouleau", "Rows": "rangs", "TV": "TV",
    "Air": "air", "IBC": "cuve IBC", "Shack": "cabane", "Over": "", "A": "A", "B": "B", "C": "C",
    "TI": "", "ntersection": "carrefour en T",
    # personnages, armes
    "perso": "personnage", "homme": "homme", "femme": "femme", "apparence": "apparence", "vetements": "vêtements",
    "chien": "chien", "chiot": "chiot", "beagle": "beagle", "berger": "berger", "allemand": "allemand",
    "labrador": "labrador", "malinois": "malinois", "lowpoly": "simplifié", "human": "humain", "bandana": "bandana",
    "sacs": "sacs", "phone": "téléphone", "pickaxe": "pioche", "fuel": "pistolet", "nozzle": "à carburant",
    "head": "frontale", "lamp": "lampe", "ascenseur": "ascenseur", "rifle": "fusil", "assault": "d'assaut",
    "sniper": "de précision", "carbine": "carabine", "bullpup": "bullpup", "battle": "de combat", "pistol": "pistolet",
    "launcher": "lance", "rocket": "roquettes", "grenade": "grenade", "magazine": "chargeur", "optic": "viseur",
    "scope": "lunette", "muzzle": "bouche", "brake": "frein", "suppressor": "silencieux", "foregrip": "poignée",
    "bipod": "bipied", "flash": "cache-flamme", "hider": "", "long": "long", "short": "court", "small": "petit", "no": "sans",
}


def separer(nom: str) -> list[str]:
    morceaux = []
    for partie in nom.split("_"):
        morceaux += re.findall(r"[A-Z]{2,}(?=[A-Z][a-z]|\d|$)|[A-Z]?[a-z]+|[A-Z]+|\d+(?:[.,]\d+)?(?:m|ft|x\d+)?", partie)
    return [m for m in morceaux if m]


def mesure(m: str) -> str | None:
    if re.fullmatch(r"\d+m", m):
        return m[:-1] + " m"
    if re.fullmatch(r"\d+x\d+", m):
        return m.replace("x", " × ") + " m"
    if re.fullmatch(r"\d+ft", m):
        return m[:-2] + " pieds"
    if re.fullmatch(r"\d+", m):
        return str(int(m)) if len(m) <= 2 else m
    return None


def traduire(nom: str, famille_en: str, inconnus: Counter) -> str:
    """Variante en français à partir du nom de fichier (les mots de la famille sont retirés)."""
    mots_famille = {m.lower() for m in separer(famille_en)} | {famille_en.lower().rstrip("s")}
    out = []
    for m in separer(nom):
        if m.lower() in mots_famille or m.lower().rstrip("s") in mots_famille:
            continue
        num = re.fullmatch(r"([A-Za-z]+)(\d+(?:m|x\d+)?)", m)
        if m in MOTS:
            t = MOTS[m]
        elif (x := mesure(m)) is not None:
            t = x
        elif num and num.group(1) in MOTS:
            t = f"{MOTS[num.group(1)]} {mesure(num.group(2)) or num.group(2)}"
        elif re.fullmatch(r"H\d+", m):
            v = int(m[1:])
            t = f"h. {v / 100:g} m".replace(".", ",", 1).replace("h,", "h.") if v >= 20 else f"h. {v} m"
        else:
            inconnus[m] += 1
            t = m
        if t and (not out or out[-1] != t):
            out.append(t)
    return " ".join(out)


def nom_affiche(famille_fr: str, variante: str) -> str:
    if not variante:
        return famille_fr
    return f"{famille_fr} — {variante}"


def uris(chemin: Path) -> list[str]:
    if chemin.suffix == ".gltf":
        doc = json.loads(chemin.read_text(encoding="utf-8"))
    else:
        data = chemin.read_bytes()
        magic, version, _ = struct.unpack_from("<III", data, 0)
        if magic != 0x46546C67 or version != 2:
            raise ValueError("pas un glTF binaire 2.0")
        jlen, _ = struct.unpack_from("<II", data, 12)
        doc = json.loads(data[20:20 + jlen].decode("utf-8"))
    return [x["uri"] for k in ("images", "buffers") for x in doc.get(k, []) if "uri" in x]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", type=Path, default=SRC)
    ap.add_argument("--dest", type=Path, default=DEST)
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    src, dest = a.src.resolve(), a.dest.resolve()

    sections: dict[str, dict] = {}
    inconnus: Counter = Counter()
    a_copier: set[Path] = set()
    refus = []
    ids = set()

    for f in sorted(src.rglob("*")):
        if f.suffix.lower() not in MODELES or not f.is_file():
            continue
        rel = f.relative_to(src)
        parts = rel.parts
        if parts[0] in EXCLUS_DOSSIERS or f.stem in EXCLUS_FICHIERS:
            continue
        # Section et famille.
        if parts[0] == "props":
            cle_section = parts[1]
            if cle_section in ("handheld", "worn"):
                famille_en = cle_section
            else:
                reste = [p for p in parts[2:-1] if p not in ("Debris", "Sizes") and not re.match(r"^\d\.\d_", p)]
                famille_en = reste[0] if reste else parts[1]
        else:
            cle_section = parts[0]
            if parts[0] == "city_kit":
                famille_en = next((k for k, _ in KIT if f.stem.startswith(k)), "Divers")
            elif parts[0] in ("characters", "animals", "weapons"):
                famille_en = parts[-2] if len(parts) > 2 else parts[0]
            else:
                famille_en = parts[0]
        if cle_section not in SECTIONS:
            print(f"section inconnue ignorée : {rel}", file=sys.stderr)
            continue
        sid, snom, sicone = SECTIONS[cle_section]
        if parts[0] == "city_kit":
            ffr = dict(KIT).get(famille_en, "Divers")
        else:
            ffr = FAMILLES.get(famille_en)
            if ffr is None:
                inconnus[f"famille:{famille_en}"] += 1
                ffr = famille_en
        # Liens vers les textures et buffers : confinés à la bibliothèque.
        try:
            liens = uris(f)
        except (ValueError, json.JSONDecodeError, struct.error) as e:
            refus.append(f"{rel} : {e}")
            continue
        ok = True
        annexes = []
        for u in liens:
            if u.startswith("data:"):
                continue
            cible = (f.parent / PurePosixPath(u)).resolve()
            if src not in cible.parents or not cible.is_file():
                refus.append(f"{rel} : lien hors de la bibliothèque ou absent ({u})")
                ok = False
                break
            annexes.append(cible)
        if not ok:
            continue
        a_copier.add(f)
        a_copier.update(annexes)

        variante = traduire(f.stem, famille_en, inconnus)
        if "Debris" in parts[:-1]:
            variante = "débris " + variante.replace("débris", "").strip()
            nom = nom_affiche(ffr, variante.strip())
        elif f.stem in NOMS:
            nom = NOMS[f.stem]
        else:
            nom = nom_affiche(ffr, variante)
        # Identifiant sûr (minuscules, chiffres, « _ », 64 au plus) : le nom du fichier, la section en cas de doublon.
        base = re.sub(r"[^a-z0-9_]+", "_", re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", f.stem).lower()).strip("_")
        if "Debris" in parts[:-1]:
            base = re.sub(r"[^a-z0-9_]+", "_", f"{famille_en}_{base}".lower())
        oid = base[:64]
        if oid in ids:
            oid = f"{sid}_{base}"[:64]
        n = 2
        while oid in ids:
            oid = f"{base[:58]}_{n}"
            n += 1
        ids.add(oid)
        mots = sorted({m.lower() for m in separer(f.stem)} | set(ffr.lower().split()) | set(nom.lower().split()))
        sec = sections.setdefault(sid, {"id": sid, "nom": snom, "icone": sicone, "familles": {}})
        fam = sec["familles"].setdefault(ffr, {"nom": ffr, "objets": []})
        fam["objets"].append(
            {
                "id": oid,
                "nom": nom[:80],
                "fichier": rel.as_posix(),
                "mots": [m for m in mots if 1 < len(m) <= 32][:24],
            }
        )

    # Mise en forme : sections dans l'ordre voulu, familles et objets triés par nom.
    sortie_sections = []
    for sid in ORDRE_SECTIONS:
        if sid not in sections:
            continue
        s = sections[sid]
        familles = []
        for i, (ffr, fam) in enumerate(sorted(s["familles"].items(), key=lambda kv: kv[0].lower())):
            fam["objets"].sort(key=lambda o: o["nom"].lower())
            familles.append({"id": f"{sid}_{i:02}", "nom": fam["nom"], "objets": fam["objets"]})
        sortie_sections.append({"id": sid, "nom": s["nom"], "icone": s["icone"], "familles": familles})
    catalogue = {"format": "mcfbibliotheque", "version": 1, "sections": sortie_sections}

    n_obj = sum(len(f["objets"]) for s in sortie_sections for f in s["familles"])
    n_fam = sum(len(s["familles"]) for s in sortie_sections)
    print(f"{n_obj} objets, {n_fam} familles, {len(sortie_sections)} sections ; {len(a_copier)} fichiers à copier")
    for s in sortie_sections:
        print(f"  {s['nom']:<40} {sum(len(f['objets']) for f in s['familles']):4} objets")
    for r in refus:
        print("refusé :", r)
    if inconnus:
        print("mots non traduits :", ", ".join(f"{k} ({v})" for k, v in inconnus.most_common(80)))
    if a.dry_run:
        return 0

    octets = 0
    for f in sorted(a_copier):
        cible = dest / f.relative_to(src)
        cible.parent.mkdir(parents=True, exist_ok=True)
        if not cible.exists() or cible.stat().st_size != f.stat().st_size or cible.stat().st_mtime < f.stat().st_mtime:
            shutil.copy2(f, cible)
            octets += f.stat().st_size
    CATALOGUE.write_text(json.dumps(catalogue, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"copié {octets / 1e6:.0f} Mo ; catalogue : {CATALOGUE}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
