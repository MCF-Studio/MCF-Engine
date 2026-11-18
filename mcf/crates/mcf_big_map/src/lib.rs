//! Générateur déterministe de la **grosse carte du test A** (`mcf/docs/bancs-essai.md`).
//!
//! Produit des calques `.mcfmap` ordinaires (aucun format nouveau pour les éléments posés), découpés par cellule
//! carrée (500 m par défaut) puis en fichiers de 5 000 éléments au plus. Ce découpage respecte les bornes du
//! format (20 000 éléments, 8 Mio par fichier) et prépare le prototype de streaming par cellules.
//!
//! Garanties, vérifiées par les tests :
//! - **même graine + mêmes paramètres ⇒ mêmes fichiers, octet pour octet**, sur toutes les machines ;
//! - chaque fichier passe la validation stricte de `mcf_map` avec le catalogue du banc ([`catalog`]) ;
//! - les `uid` sont uniques sur **toute** la carte, pas seulement dans un calque ;
//! - les bâtiments occupent des parcelles distinctes (aucun chevauchement).
//!
//! Disposition : une ville carrée au centre (30 % du côté de l'île), en îlots de 100 m séparés par des rues de
//! 20 m, chaque îlot coupé en 4 × 4 parcelles de 20 m. Lampes, véhicules et personnages suivent les rues ; les
//! objets sont à 70 % en ville ; les arbres sont hors de la ville, à 70 % groupés en forêts. Toutes les hauteurs
//! valent 0 : le terrain (heightmap) est une étape séparée, les éléments seront posés dessus au chargement.
//!
//! Chaque type d'élément a son propre flux pseudo-aléatoire : changer le nombre d'arbres ne déplace pas les
//! bâtiments, ce qui garde les mesures comparables d'un réglage à l'autre.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use mcf_map::{
    MAX_COORD, MAX_FILE_BYTES, MAX_ITEMS, MapItem, MapTransform, McfMap, PropValue, SimpleCatalog,
    write_atomic,
};
use serde_json::{Value, json};

/// Version du catalogue du banc.
pub const CATALOG_VERSION: i64 = 1;
/// Auteur écrit dans chaque calque.
pub const AUTHOR: &str = "mcf_big_map";
/// Date écrite dans chaque calque : fixe, pour que deux générations donnent les mêmes octets.
pub const CREATED: &str = "2026-11-18T00:00:00";
/// Version du manifeste (`manifest.json`).
pub const MANIFEST_VERSION: i64 = 1;

/// Pas de la grille de la ville (îlot + rue), en mètres.
pub const BLOCK: f64 = 100.0;
/// Largeur d'une rue, en mètres.
pub const STREET: f64 = 20.0;
/// Côté d'une parcelle de bâtiment, en mètres.
pub const LOT: f64 = 20.0;
/// Parcelles par côté d'îlot.
const LOTS_PER_SIDE: usize = ((BLOCK - STREET) / LOT) as usize;
/// Espacement des lampes le long d'une rue, en mètres.
const LAMP_SPACING: f64 = 25.0;

const MODELS_BUILDING: usize = 30;
const MODELS_INTERIOR: usize = 10;
const MODELS_OBJECT: usize = 200;
const MODELS_TREE: usize = 8;
const MODELS_CHARACTER: usize = 10;
const MODELS_VEHICLE: usize = 12;

/// Couleurs des véhicules (réglage `color`).
const VEHICLE_COLORS: &[&str] = &["#b01e23", "#1d3f8f", "#e6e6e6", "#202020", "#7a7f85", "#2f6b3a"];

/// Nature d'un élément généré.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// Bâtiment (extérieur seul).
    Building,
    /// Bâtiment avec intérieur visitable.
    Interior,
    /// Objet posé (mobilier, props).
    Object,
    /// Arbre.
    Tree,
    /// Lampe sans ombre.
    Lamp,
    /// Lampe avec ombre (exception budgétée).
    ShadowLamp,
    /// Point d'apparition d'un personnage animé.
    Character,
    /// Véhicule.
    Vehicle,
}

impl Kind {
    /// Toutes les natures, dans l'ordre de génération.
    pub const ALL: [Self; 8] = [
        Self::Building,
        Self::Interior,
        Self::Object,
        Self::Tree,
        Self::Lamp,
        Self::ShadowLamp,
        Self::Character,
        Self::Vehicle,
    ];

    /// Nom court (manifeste).
    pub fn name(self) -> &'static str {
        match self {
            Self::Building => "batiments_sans_interieur",
            Self::Interior => "interieurs",
            Self::Object => "objets",
            Self::Tree => "arbres",
            Self::Lamp => "lampes_sans_ombre",
            Self::ShadowLamp => "lampes_ombre",
            Self::Character => "personnages",
            Self::Vehicle => "vehicules",
        }
    }

    /// Sel du flux pseudo-aléatoire propre à cette nature.
    fn salt(self) -> u64 {
        0x4D43_465F_0000_0000 | self as u64
    }
}

/// Quantités à générer (palier).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quantities {
    /// Bâtiments au total (intérieurs visitables compris).
    pub buildings: usize,
    /// Dont bâtiments avec intérieur visitable.
    pub interiors: usize,
    /// Objets posés.
    pub objects: usize,
    /// Arbres.
    pub trees: usize,
    /// Lampes au total (avec ombre comprises).
    pub lamps: usize,
    /// Dont lampes avec ombre.
    pub shadow_lamps: usize,
    /// Personnages animés.
    pub characters: usize,
    /// Véhicules.
    pub vehicles: usize,
}

impl Quantities {
    /// Palier de départ du test A.
    pub const START: Self = Self {
        buildings: 800,
        interiors: 100,
        objects: 80_000,
        trees: 30_000,
        lamps: 2_000,
        shadow_lamps: 50,
        characters: 100,
        vehicles: 200,
    };

    /// Palier « stress » du test A. Le protocole ne fixe pas les lampes avec ombre à ce palier : on garde 50
    /// (l'ombre reste une exception budgétée, son coût ne doit pas grandir avec la carte).
    pub const STRESS: Self = Self {
        buildings: 2_000,
        interiors: 300,
        objects: 200_000,
        trees: 100_000,
        lamps: 5_000,
        shadow_lamps: 50,
        characters: 300,
        vehicles: 500,
    };

    /// Quantité pour une nature (bâtiments et lampes : hors intérieurs / hors lampes avec ombre).
    pub fn of(&self, kind: Kind) -> usize {
        match kind {
            Kind::Building => self.buildings - self.interiors.min(self.buildings),
            Kind::Interior => self.interiors,
            Kind::Object => self.objects,
            Kind::Tree => self.trees,
            Kind::Lamp => self.lamps - self.shadow_lamps.min(self.lamps),
            Kind::ShadowLamp => self.shadow_lamps,
            Kind::Character => self.characters,
            Kind::Vehicle => self.vehicles,
        }
    }

    /// Nombre total d'éléments.
    pub fn total(&self) -> usize {
        Kind::ALL.iter().map(|k| self.of(*k)).sum()
    }
}

/// Paramètres d'une génération.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Nom du palier (recopié dans le manifeste).
    pub tier: String,
    /// Graine.
    pub seed: u64,
    /// Côté de l'île, en mètres.
    pub size: f64,
    /// Côté d'une cellule, en mètres.
    pub cell: f64,
    /// Nombre maximal d'éléments par fichier.
    pub max_per_file: usize,
    /// Quantités.
    pub quantities: Quantities,
}

/// Graine par défaut (« MCF_BIG_ »).
pub const DEFAULT_SEED: u64 = 0x4D43_465F_4249_475F;

impl Params {
    /// Palier de départ : île de 4 × 4 km.
    pub fn start() -> Self {
        Self {
            tier: "depart".into(),
            seed: DEFAULT_SEED,
            size: 4_000.0,
            cell: 500.0,
            max_per_file: 5_000,
            quantities: Quantities::START,
        }
    }

    /// Palier « stress » : île de 8 × 8 km.
    pub fn stress() -> Self {
        Self {
            tier: "stress".into(),
            size: 8_000.0,
            quantities: Quantities::STRESS,
            ..Self::start()
        }
    }

    /// Côté de la ville, en mètres (multiple de [`BLOCK`]).
    pub fn city_side(&self) -> f64 {
        ((self.size * 0.3) / BLOCK).round().max(1.0) * BLOCK
    }

    /// Nombre de cellules par côté.
    pub fn cells_per_side(&self) -> usize {
        (self.size / self.cell).ceil() as usize
    }

    /// Vérifie les paramètres (bornes du format, capacité de la ville).
    pub fn check(&self) -> Result<(), String> {
        let q = &self.quantities;
        if !(1_000.0..=2.0 * MAX_COORD).contains(&self.size) {
            return Err(format!(
                "taille hors limites : {} m (1000 à {} m)",
                self.size,
                2.0 * MAX_COORD
            ));
        }
        if !(100.0..=2_000.0).contains(&self.cell) {
            return Err(format!("cellule hors limites : {} m (100 à 2000 m)", self.cell));
        }
        if !(1..=MAX_ITEMS).contains(&self.max_per_file) {
            return Err(format!(
                "éléments par fichier hors limites : {} (1 à {MAX_ITEMS})",
                self.max_per_file
            ));
        }
        if q.interiors > q.buildings || q.shadow_lamps > q.lamps {
            return Err("plus d'intérieurs que de bâtiments, ou de lampes à ombre que de lampes".into());
        }
        let blocks = (self.city_side() / BLOCK) as usize;
        let lots = blocks * blocks * LOTS_PER_SIDE * LOTS_PER_SIDE;
        if q.buildings > lots {
            return Err(format!(
                "{} bâtiments pour {lots} parcelles en ville",
                q.buildings
            ));
        }
        if q.total() > 2_000_000 {
            return Err(format!("{} éléments : au-delà de 2 000 000, refusé", q.total()));
        }
        if self.tier.is_empty()
            || self.tier.len() > 32
            || !self.tier.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err("nom de palier invalide (lettres, chiffres, _ ; 32 caractères au plus)".into());
        }
        Ok(())
    }
}

/// Un fichier de calque généré.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Nom du fichier, sans extension (= nom du calque).
    pub stem: String,
    /// Cellule (colonne selon X, ligne selon Z), depuis le coin −X −Z de l'île.
    pub cell: (usize, usize),
    /// Contenu.
    pub map: McfMap,
}

/// Résultat d'une génération.
#[derive(Debug, Clone, PartialEq)]
pub struct Generated {
    /// Paramètres utilisés.
    pub params: Params,
    /// Calques, triés par cellule puis par partie.
    pub layers: Vec<Layer>,
}

/// Catalogue du banc : identifiants `bench_*` et réglages modifiables.
pub fn catalog() -> SimpleCatalog {
    let mut c = SimpleCatalog::new(CATALOG_VERSION);
    for (prefix, n, props) in catalog_families() {
        for i in 0..n {
            c = c.with(&model_id(prefix, i, n), props);
        }
    }
    c.with("bench_lampe", &["light_on_at_night"])
        .with("bench_lampe_ombre", &["light_on_at_night"])
}

fn catalog_families() -> [(&'static str, usize, &'static [&'static str]); 6] {
    [
        ("bench_batiment", MODELS_BUILDING, &["variant"]),
        ("bench_batiment_int", MODELS_INTERIOR, &["variant"]),
        ("bench_objet", MODELS_OBJECT, &["variant"]),
        ("bench_arbre", MODELS_TREE, &["variant"]),
        ("bench_personnage", MODELS_CHARACTER, &[]),
        ("bench_vehicule", MODELS_VEHICLE, &["color"]),
    ]
}

/// Identifiant du modèle `i` d'une famille de `n` modèles (`bench_objet_007`).
fn model_id(prefix: &str, i: usize, n: usize) -> String {
    let width = n.saturating_sub(1).to_string().len().max(2);
    format!("{prefix}_{i:0width$}")
}

/// Mélange SplitMix64 : bijectif sur `u64`.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

/// Générateur pseudo-aléatoire SplitMix64 (aucune dépendance, identique partout).
struct Rng(u64);

impl Rng {
    fn new(seed: u64, salt: u64) -> Self {
        Self(mix(seed ^ mix(salt)))
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(GOLDEN);
        mix(self.0)
    }

    /// Uniforme dans [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    /// Entier dans [0, n) ; `n` > 0.
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            v.swap(i, self.below(i + 1));
        }
    }
}

/// Élément placé, avant découpage en calques.
struct Placed {
    catalog: String,
    x: f64,
    z: f64,
    yaw: f64,
    scale: f64,
    props: BTreeMap<String, PropValue>,
}

/// Géométrie de la ville et de l'île.
struct World {
    half: f64,
    city_half: f64,
    blocks: usize,
}

impl World {
    fn new(p: &Params) -> Self {
        let city = p.city_side();
        Self {
            half: p.size / 2.0,
            city_half: city / 2.0,
            blocks: (city / BLOCK) as usize,
        }
    }

    /// Coordonnée de l'axe de la rue `k` (0 ..= blocks).
    fn street(&self, k: usize) -> f64 {
        -self.city_half + k as f64 * BLOCK
    }

    fn in_city(&self, x: f64, z: f64, margin: f64) -> bool {
        x.abs() < self.city_half + margin && z.abs() < self.city_half + margin
    }

    fn in_island(&self, x: f64, z: f64) -> bool {
        x.abs() <= self.half - 1.0 && z.abs() <= self.half - 1.0
    }

    /// Point uniforme sur l'île, hors de la ville (marge 10 m).
    fn countryside(&self, rng: &mut Rng) -> (f64, f64) {
        let lim = self.half - 1.0;
        loop {
            let (x, z) = (rng.range(-lim, lim), rng.range(-lim, lim));
            if !self.in_city(x, z, 10.0) {
                return (x, z);
            }
        }
    }

    /// Point uniforme à l'intérieur d'un îlot (hors rue).
    fn in_block(&self, rng: &mut Rng) -> (f64, f64) {
        let (bi, bj) = (rng.below(self.blocks), rng.below(self.blocks));
        let m = STREET / 2.0 + 0.5;
        let x = rng.range(self.street(bi) + m, self.street(bi + 1) - m);
        let z = rng.range(self.street(bj) + m, self.street(bj + 1) - m);
        (x, z)
    }

    /// Point au hasard le long d'une rue, décalé de `offset` mètres de l'axe ; avec le cap de la rue.
    fn on_street(&self, rng: &mut Rng, offset: f64) -> (f64, f64, f64) {
        let k = rng.below(self.blocks + 1);
        let t = rng.range(-self.city_half, self.city_half);
        let side = if rng.below(2) == 0 { -1.0 } else { 1.0 };
        let line = self.street(k) + side * offset;
        if rng.below(2) == 0 {
            // Rue orientée selon Z : on roule vers −Z ou +Z selon le côté.
            (line, t, if side > 0.0 { 0.0 } else { std::f64::consts::PI })
        } else {
            (t, line, side * std::f64::consts::FRAC_PI_2)
        }
    }
}

fn simple(catalog: String, x: f64, z: f64, yaw: f64) -> Placed {
    Placed {
        catalog,
        x,
        z,
        yaw,
        scale: 1.0,
        props: BTreeMap::new(),
    }
}

fn with_prop(mut p: Placed, key: &str, v: PropValue) -> Placed {
    p.props.insert(key.to_owned(), v);
    p
}

/// Génère la carte. Aucune écriture sur le disque.
pub fn generate(params: &Params) -> Result<Generated, String> {
    params.check()?;
    let w = World::new(params);
    let q = &params.quantities;
    let seed = params.seed;
    let tau = std::f64::consts::TAU;
    let mut all: Vec<Placed> = Vec::with_capacity(q.total());

    // Bâtiments : parcelles distinctes tirées dans la grille de la ville.
    {
        let mut rng = Rng::new(seed, Kind::Building.salt());
        let mut lots = Vec::with_capacity(w.blocks * w.blocks * LOTS_PER_SIDE * LOTS_PER_SIDE);
        for bi in 0..w.blocks {
            for bj in 0..w.blocks {
                for li in 0..LOTS_PER_SIDE {
                    for lj in 0..LOTS_PER_SIDE {
                        let x = w.street(bi) + STREET / 2.0 + (li as f64 + 0.5) * LOT;
                        let z = w.street(bj) + STREET / 2.0 + (lj as f64 + 0.5) * LOT;
                        lots.push((x, z));
                    }
                }
            }
        }
        rng.shuffle(&mut lots);
        for (n, &(x, z)) in lots.iter().take(q.buildings).enumerate() {
            let yaw = rng.below(4) as f64 * std::f64::consts::FRAC_PI_2;
            let (id, variant) = if n < q.interiors {
                (
                    model_id("bench_batiment_int", rng.below(MODELS_INTERIOR), MODELS_INTERIOR),
                    rng.below(4),
                )
            } else {
                (
                    model_id("bench_batiment", rng.below(MODELS_BUILDING), MODELS_BUILDING),
                    rng.below(4),
                )
            };
            all.push(with_prop(
                simple(id, x, z, yaw),
                "variant",
                PropValue::Int(variant as i64),
            ));
        }
    }

    // Objets : 70 % dans les îlots de la ville, 30 % à la campagne.
    {
        let mut rng = Rng::new(seed, Kind::Object.salt());
        let in_town = q.objects * 7 / 10;
        for n in 0..q.objects {
            let (x, z) = if n < in_town {
                w.in_block(&mut rng)
            } else {
                w.countryside(&mut rng)
            };
            let id = model_id("bench_objet", rng.below(MODELS_OBJECT), MODELS_OBJECT);
            let p = simple(id, x, z, rng.range(0.0, tau));
            all.push(with_prop(p, "variant", PropValue::Int(rng.below(4) as i64)));
        }
    }

    // Arbres : 70 % en forêts (environ 500 arbres par forêt), 30 % isolés ; jamais en ville.
    {
        let mut rng = Rng::new(seed, Kind::Tree.salt());
        let forests: Vec<(f64, f64)> = (0..(q.trees / 500).max(1))
            .map(|_| w.countryside(&mut rng))
            .collect();
        let grouped = q.trees * 7 / 10;
        for n in 0..q.trees {
            let (x, z) = if n < grouped {
                let (cx, cz) = forests[rng.below(forests.len())];
                let mut found = None;
                for _ in 0..16 {
                    // Somme de 3 uniformes : cloche approchée, rayon typique ~120 m.
                    let dx = (rng.unit() + rng.unit() + rng.unit() - 1.5) * 240.0;
                    let dz = (rng.unit() + rng.unit() + rng.unit() - 1.5) * 240.0;
                    let (x, z) = (cx + dx, cz + dz);
                    if w.in_island(x, z) && !w.in_city(x, z, 10.0) {
                        found = Some((x, z));
                        break;
                    }
                }
                found.unwrap_or_else(|| w.countryside(&mut rng))
            } else {
                w.countryside(&mut rng)
            };
            let id = model_id("bench_arbre", rng.below(MODELS_TREE), MODELS_TREE);
            let mut p = simple(id, x, z, rng.range(0.0, tau));
            p.scale = rng.range(0.8, 1.3);
            all.push(with_prop(p, "variant", PropValue::Int(rng.below(4) as i64)));
        }
    }

    // Lampes : sur les trottoirs tous les 25 m, puis à la campagne s'il en reste.
    {
        let mut rng = Rng::new(seed, Kind::Lamp.salt());
        let mut spots = Vec::new();
        let steps = (2.0 * w.city_half / LAMP_SPACING) as usize;
        for k in 0..=w.blocks {
            for side in [-1.0, 1.0] {
                let line = w.street(k) + side * (STREET / 2.0 - 1.0);
                for s in 0..steps {
                    let t = -w.city_half + (s as f64 + 0.5) * LAMP_SPACING;
                    spots.push((line, t));
                    spots.push((t, line));
                }
            }
        }
        rng.shuffle(&mut spots);
        for n in 0..q.lamps {
            let (x, z) = spots.get(n).copied().unwrap_or_else(|| w.countryside(&mut rng));
            let id = if n < q.shadow_lamps {
                "bench_lampe_ombre"
            } else {
                "bench_lampe"
            };
            let p = simple(id.into(), x, z, 0.0);
            all.push(with_prop(p, "light_on_at_night", PropValue::Bool(true)));
        }
    }

    // Personnages : sur les trottoirs ; véhicules : sur les voies, dans le sens de la rue.
    {
        let mut rng = Rng::new(seed, Kind::Character.salt());
        for _ in 0..q.characters {
            let (x, z, _) = w.on_street(&mut rng, STREET / 2.0 - 2.0);
            let id = model_id("bench_personnage", rng.below(MODELS_CHARACTER), MODELS_CHARACTER);
            all.push(simple(id, x, z, rng.range(0.0, tau)));
        }
        let mut rng = Rng::new(seed, Kind::Vehicle.salt());
        for _ in 0..q.vehicles {
            let (x, z, yaw) = w.on_street(&mut rng, 4.0);
            let id = model_id("bench_vehicule", rng.below(MODELS_VEHICLE), MODELS_VEHICLE);
            let color = VEHICLE_COLORS[rng.below(VEHICLE_COLORS.len())];
            all.push(with_prop(
                simple(id, x, z, yaw),
                "color",
                PropValue::Color(color.into()),
            ));
        }
    }

    Ok(Generated {
        layers: split(params, &w, all),
        params: params.clone(),
    })
}

/// Arrondi à `10^-decimals`. On **divise** par une puissance de 10 exacte : le résultat est le flottant le plus
/// proche du décimal voulu, qui s'écrit court (`-1806.61` et non `-1806.6100000000001`).
fn round(v: f64, decimals: i32) -> f64 {
    let scale = 10f64.powi(decimals);
    let r = (v * scale).round() / scale;
    if r == 0.0 { 0.0 } else { r } // pas de « -0 » dans le JSON
}

fn transform(p: &Placed) -> MapTransform {
    let (s, c) = p.yaw.sin_cos();
    let k = p.scale;
    let b = |v: f64| round(v * k, 6);
    MapTransform {
        origin: [round(p.x, 2), 0.0, round(p.z, 2)],
        // Rotation autour de Y (comme `Basis(Vector3.UP, yaw)` de Godot) × échelle uniforme.
        basis: [[b(c), 0.0, b(-s)], [0.0, b(1.0), 0.0], [b(s), 0.0, b(c)]],
    }
}

/// Découpe par cellule puis par paquets de `max_per_file`, et attribue les `uid`.
fn split(params: &Params, w: &World, all: Vec<Placed>) -> Vec<Layer> {
    let n = params.cells_per_side();
    let cell_of = |v: f64| (((v + w.half) / params.cell).floor().max(0.0) as usize).min(n - 1);
    let mut cells: BTreeMap<(usize, usize), Vec<Placed>> = BTreeMap::new();
    for p in all {
        cells.entry((cell_of(p.x), cell_of(p.z))).or_default().push(p);
    }
    let uid_base = mix(params.seed ^ 0x5549_445F);
    let mut counter: u64 = 0;
    let mut layers = Vec::new();
    for ((cx, cz), items) in cells {
        for (part, chunk) in items.chunks(params.max_per_file).enumerate() {
            let stem = format!("bench_c{cx:03}_{cz:03}_{part}");
            let mut map = McfMap::new(&stem, AUTHOR, CREATED, CATALOG_VERSION);
            map.items = chunk
                .iter()
                .map(|p| {
                    // i ↦ mix(base + i·φ) est bijectif : uid uniques sur toute la carte.
                    let uid = format!(
                        "{:016x}",
                        mix(uid_base.wrapping_add(counter.wrapping_mul(GOLDEN)))
                    );
                    counter += 1;
                    MapItem {
                        uid,
                        catalog: p.catalog.clone(),
                        transform: transform(p),
                        props: p.props.clone(),
                    }
                })
                .collect();
            layers.push(Layer {
                stem,
                cell: (cx, cz),
                map,
            });
        }
    }
    layers
}

/// Manifeste : paramètres, quantités, liste des fichiers (pour le chargeur et le rapport de mesure).
pub fn manifest(g: &Generated) -> Value {
    let p = &g.params;
    let quantities: serde_json::Map<String, Value> = Kind::ALL
        .iter()
        .map(|k| (k.name().to_owned(), json!(p.quantities.of(*k))))
        .collect();
    let files: Vec<Value> = g
        .layers
        .iter()
        .map(|l| json!({ "fichier": format!("{}.mcfmap", l.stem), "cellule": [l.cell.0, l.cell.1], "elements": l.map.items.len() }))
        .collect();
    json!({
        "format": "mcf_big_map",
        "version": MANIFEST_VERSION,
        "palier": p.tier,
        // En texte : un entier 64 bits ne survit pas à un nombre JSON lu en flottant (Godot, JavaScript).
        "graine": format!("{:016x}", p.seed),
        "taille_m": p.size,
        "cellule_m": p.cell,
        "cellules_par_cote": p.cells_per_side(),
        "ville_m": p.city_side(),
        "catalog_version": CATALOG_VERSION,
        "quantites": quantities,
        "elements": g.layers.iter().map(|l| l.map.items.len()).sum::<usize>(),
        "fichiers": files,
    })
}

/// Catalogue du banc en JSON : `{ "version": 1, "fiches": { "<id>": ["<réglage>", …] } }`.
pub fn catalog_json() -> Value {
    let c = catalog();
    json!({ "version": c.version, "fiches": c.entries })
}

/// Bilan d'une écriture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Written {
    /// Fichiers `.mcfmap` écrits.
    pub files: usize,
    /// Octets écrits (calques seulement).
    pub bytes: usize,
    /// Plus gros fichier, en octets.
    pub largest: usize,
}

/// Écrit les calques, `catalogue.json` et `manifest.json` dans `dir`, qui doit être absent ou vide (on ne
/// mélange jamais deux générations, et on n'efface rien).
pub fn write_dir(g: &Generated, dir: &Path) -> Result<Written, String> {
    let err = |what: &str, e: std::io::Error| format!("{what} ({}) : {e}", dir.display());
    if dir.exists() {
        let mut entries = fs::read_dir(dir).map_err(|e| err("lecture du dossier impossible", e))?;
        if entries.next().is_some() {
            return Err(format!(
                "le dossier {} n'est pas vide : choisir un autre dossier",
                dir.display()
            ));
        }
    }
    fs::create_dir_all(dir).map_err(|e| err("création du dossier impossible", e))?;
    let mut w = Written {
        files: 0,
        bytes: 0,
        largest: 0,
    };
    for l in &g.layers {
        let text = l.map.to_json_pretty().map_err(|e| format!("{} : {e}", l.stem))?;
        if text.len() > MAX_FILE_BYTES {
            return Err(format!(
                "{} : {} octets, au-delà de la limite du format",
                l.stem,
                text.len()
            ));
        }
        write_atomic(&dir.join(format!("{}.mcfmap", l.stem)), text.as_bytes())
            .map_err(|e| err("écriture impossible", e))?;
        w.files += 1;
        w.bytes += text.len();
        w.largest = w.largest.max(text.len());
    }
    for (name, v) in [("catalogue.json", catalog_json()), ("manifest.json", manifest(g))] {
        let mut text = serde_json::to_string_pretty(&v).map_err(|e| format!("{name} : {e}"))?;
        let _ = writeln!(text);
        write_atomic(&dir.join(name), text.as_bytes()).map_err(|e| err("écriture impossible", e))?;
    }
    Ok(w)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use mcf_map::load_str;

    use super::*;

    fn small() -> Params {
        Params {
            tier: "test".into(),
            quantities: Quantities {
                buildings: 120,
                interiors: 10,
                objects: 3_000,
                trees: 2_000,
                lamps: 300,
                shadow_lamps: 5,
                characters: 20,
                vehicles: 30,
            },
            max_per_file: 400,
            ..Params::start()
        }
    }

    fn count(g: &Generated) -> BTreeMap<&'static str, usize> {
        let mut c = BTreeMap::new();
        for it in g.layers.iter().flat_map(|l| &l.map.items) {
            let kind = match it.catalog.as_str() {
                "bench_lampe" => "lampes_sans_ombre",
                "bench_lampe_ombre" => "lampes_ombre",
                s if s.starts_with("bench_batiment_int") => "interieurs",
                s if s.starts_with("bench_batiment") => "batiments_sans_interieur",
                s if s.starts_with("bench_objet") => "objets",
                s if s.starts_with("bench_arbre") => "arbres",
                s if s.starts_with("bench_personnage") => "personnages",
                s if s.starts_with("bench_vehicule") => "vehicules",
                other => panic!("identifiant inattendu {other}"),
            };
            *c.entry(kind).or_default() += 1;
        }
        c
    }

    #[test]
    fn deterministe() {
        let a = generate(&small()).expect("génération");
        let b = generate(&small()).expect("génération");
        assert_eq!(a, b);
        let mut other = small();
        other.seed ^= 1;
        assert_ne!(generate(&other).expect("génération").layers, a.layers);
    }

    #[test]
    fn quantites_exactes() {
        let p = small();
        let g = generate(&p).expect("génération");
        let c = count(&g);
        for k in Kind::ALL {
            assert_eq!(
                c.get(k.name()).copied().unwrap_or(0),
                p.quantities.of(k),
                "{}",
                k.name()
            );
        }
    }

    #[test]
    fn calques_valides_et_uid_uniques() {
        let p = small();
        let g = generate(&p).expect("génération");
        let cat = catalog();
        let mut uids = HashSet::new();
        let half = p.size / 2.0;
        for l in &g.layers {
            assert!(l.map.items.len() <= p.max_per_file);
            let text = l.map.to_json_pretty().expect("écriture");
            assert!(
                !text.contains("00000000") && !text.contains("99999999"),
                "nombre mal arrondi dans {}",
                l.stem
            );
            let relu = load_str(&text, Some(&cat)).expect("relecture validée avec le catalogue");
            assert_eq!(relu.map, l.map);
            for it in &l.map.items {
                assert!(uids.insert(it.uid.clone()), "uid en double {}", it.uid);
                let [x, _, z] = it.transform.origin;
                assert!(x.abs() < half && z.abs() < half);
                let cell = |v: f64| ((v + half) / p.cell).floor() as usize;
                assert_eq!((cell(x), cell(z)), l.cell, "{} hors de sa cellule", it.uid);
            }
        }
    }

    #[test]
    fn batiments_sans_chevauchement_et_en_ville() {
        let p = small();
        let g = generate(&p).expect("génération");
        let city_half = p.city_side() / 2.0;
        let mut seen = HashSet::new();
        for it in g.layers.iter().flat_map(|l| &l.map.items) {
            if it.catalog.starts_with("bench_batiment") {
                let [x, _, z] = it.transform.origin;
                assert!(x.abs() < city_half && z.abs() < city_half);
                assert!(
                    seen.insert(((x * 100.0) as i64, (z * 100.0) as i64)),
                    "parcelle occupée deux fois"
                );
            }
        }
        assert_eq!(seen.len(), p.quantities.buildings);
    }

    #[test]
    fn arbres_hors_de_la_ville() {
        let p = small();
        let g = generate(&p).expect("génération");
        let city_half = p.city_side() / 2.0;
        for it in g.layers.iter().flat_map(|l| &l.map.items) {
            if it.catalog.starts_with("bench_arbre") {
                let [x, _, z] = it.transform.origin;
                assert!(x.abs() >= city_half || z.abs() >= city_half);
            }
        }
    }

    #[test]
    fn changer_les_arbres_ne_deplace_pas_les_batiments() {
        let a = generate(&small()).expect("génération");
        let mut p = small();
        p.quantities.trees = 50;
        let b = generate(&p).expect("génération");
        let pos = |g: &Generated| -> Vec<(String, [f64; 3])> {
            let mut v: Vec<_> = g
                .layers
                .iter()
                .flat_map(|l| &l.map.items)
                .filter(|it| it.catalog.starts_with("bench_batiment"))
                .map(|it| (it.catalog.clone(), it.transform.origin))
                .collect();
            v.sort_by(|x, y| x.partial_cmp(y).expect("pas de NaN"));
            v
        };
        assert_eq!(pos(&a), pos(&b));
    }

    #[test]
    fn paliers_du_protocole() {
        for p in [Params::start(), Params::stress()] {
            p.check().expect("palier valide");
        }
        assert_eq!(
            Quantities::START.total(),
            800 + 80_000 + 30_000 + 2_000 + 100 + 200
        );
    }

    #[test]
    fn parametres_refuses() {
        let mut p = small();
        p.size = 20_000.0;
        assert!(p.check().is_err());
        let mut p = small();
        p.quantities.buildings = 1_000_000;
        assert!(p.check().is_err());
        let mut p = small();
        p.tier = "../x".into();
        assert!(p.check().is_err());
        let mut p = small();
        p.max_per_file = MAX_ITEMS + 1;
        assert!(p.check().is_err());
    }

    #[test]
    fn ecriture_refuse_un_dossier_non_vide() {
        let dir = std::env::temp_dir().join(format!("mcf_big_map_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut p = small();
        p.quantities = Quantities {
            buildings: 4,
            interiors: 1,
            objects: 20,
            trees: 20,
            lamps: 4,
            shadow_lamps: 1,
            characters: 2,
            vehicles: 2,
        };
        let g = generate(&p).expect("génération");
        let w = write_dir(&g, &dir).expect("écriture");
        assert_eq!(w.files, g.layers.len());
        let m: Value =
            serde_json::from_str(&fs::read_to_string(dir.join("manifest.json")).expect("manifeste"))
                .expect("JSON");
        assert_eq!(m["elements"], json!(p.quantities.total()));
        assert!(write_dir(&g, &dir).is_err(), "un second passage doit être refusé");
        let _ = fs::remove_dir_all(&dir);
    }
}
