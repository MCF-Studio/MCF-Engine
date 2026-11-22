//! Format d'échange `.mcfmap` : un calque de carte en JSON de **données**, jamais une scène ni du code.
//!
//! Portage fidèle de `McfMapFormat` (dépôt `MCF-Map-Editor`, `map_editor/model/mcfmap_format.gd`) : mêmes champs,
//! mêmes bornes, mêmes messages. Un fichier écrit par l'éditeur Godot s'ouvre ici et inversement.
//!
//! Règles (tout écart = refus, rien n'est « corrigé en silence ») :
//! - seuls les champs connus sont acceptés, aucun chemin (`res://`, `user://`, `..`, `\`) n'est toléré ;
//! - chaque élément référence une fiche du catalogue par son identifiant ;
//! - réglages en liste blanche (celle de la fiche), types et bornes vérifiés ;
//! - bornes : taille du fichier, imbrication, nombre d'éléments, positions, échelle.
//!
//! Repère : celui de Godot et de Bevy (Y vers le haut, main droite, avant = −Z, mètres), **sans conversion**.
//! `basis[i]` est l'axe `i` de la base (colonne `i` de la matrice), comme `Basis.x/y/z` de Godot et
//! `Mat3::from_cols` de Bevy.
//!
//! Utilisation type :
//! ```
//! use mcf_map::{load_str, McfMap};
//! let carte = McfMap::new("ville_mobilier", "Aroun", "2026-11-18T20:00:00", 7);
//! let texte = carte.to_json_pretty().expect("carte valide");
//! let relue = load_str(&texte, None).expect("relecture");
//! assert_eq!(relue.map, carte);
//! ```

mod catalog;
mod io;
mod model;
mod text;
mod validate;

pub use catalog::{Catalog, SimpleCatalog};
pub use io::{
    LoadError, Loaded, SaveError, load_file, load_str, parse_text, read_file, save_file, write_atomic,
};
pub use model::{
    BaseChange, BaseEdit, JunctionArm, MapItem, MapJunction, MapRoad, MapTransform, McfMap, PropValue,
};
pub use validate::{Report, is_base_path, validate};

/// Valeur du champ `format`.
pub const FORMAT: &str = "mcfmap";
/// Version du format gérée.
pub const VERSION: i64 = 1;
/// Taille maximale d'un fichier, vérifiée **avant** l'analyse du JSON.
pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
/// Nombre maximal d'éléments posés dans un calque.
pub const MAX_ITEMS: usize = 20_000;
/// Valeur absolue maximale d'une coordonnée, en mètres.
pub const MAX_COORD: f64 = 5_000.0;
/// Longueur maximale d'un nom (calque, auteur, date, partie de chemin).
pub const MAX_NAME: usize = 64;
/// Imbrication maximale du JSON (un calque en utilise 5).
pub const MAX_DEPTH: usize = 8;
/// Valeur absolue maximale de `yaw` / `island` d'un carrefour.
pub const MAX_YAW: f64 = 36_000.0;
/// Valeur maximale de `catalog_version`.
pub const MAX_CATALOG_VERSION: i64 = 1_000_000;
/// Échelle minimale d'un axe.
pub const SCALE_MIN: f64 = 0.1;
/// Échelle maximale d'un axe.
pub const SCALE_MAX: f64 = 10.0;
/// Champs autorisés à la racine.
pub const TOP_KEYS: &[&str] = &[
    "format",
    "version",
    "catalog_version",
    "layer",
    "author",
    "created",
    "items",
    "roads",
    "junctions",
    "base_edits",
];
/// Nom du calque des modifications de l'île de base.
pub const BASE_LAYER: &str = "Île de base";
/// Nom de fichier (sans extension) des modifications de l'île de base.
pub const BASE_FILE: &str = "ile_de_base";
/// Champs autorisés dans une modification de l'île de base.
pub const BASE_EDIT_KEYS: &[&str] = &["path", "catalog", "from", "transform", "removed"];
/// Seuls ces groupes de l'île sont modifiables.
pub const BASE_ROOTS: &[&str] = &["City/Places", "City/ATM", "Countryside", "StreetLamps3D"];
/// Nombre maximal de modifications de l'île de base.
pub const MAX_BASE_EDITS: usize = 2_000;
/// Longueur maximale d'un chemin de nœud de l'île.
pub const MAX_BASE_PATH: usize = 128;
/// Champs autorisés dans une route.
pub const ROAD_KEYS: &[&str] = &["uid", "template", "points"];
/// Nombre maximal de routes par calque.
pub const MAX_ROADS: usize = 500;
/// Modèles de route connus (`MapRoad.TEMPLATES`).
pub const ROAD_TEMPLATES: &[&str] = &[
    "sentier",
    "chemin_terre",
    "route",
    "ruelle",
    "rue",
    "avenue",
    "voie_rapide",
];
/// Nombre minimal de points d'une route.
pub const ROAD_MIN_POINTS: usize = 2;
/// Nombre maximal de points d'une route.
pub const ROAD_MAX_POINTS: usize = 200;
/// Distance maximale entre deux points d'une route, en mètres.
pub const ROAD_MAX_SEGMENT: f64 = 400.0;
/// Champs autorisés dans un carrefour.
pub const JUNCTION_KEYS: &[&str] = &["uid", "kind", "origin", "yaw", "island", "arms"];
/// Champs autorisés dans une branche de carrefour.
pub const ARM_KEYS: &[&str] = &["angle", "template"];
/// Nombre maximal de carrefours par calque.
pub const MAX_JUNCTIONS: usize = 300;
/// Natures de carrefour connues (`MapJunction.KINDS`).
pub const JUNCTION_KINDS: &[&str] = &["carrefour", "rond_point"];
/// Nombre minimal de branches d'un carrefour.
pub const JUNCTION_MIN_ARMS: usize = 2;
/// Nombre maximal de branches d'un carrefour.
pub const JUNCTION_MAX_ARMS: usize = 6;
/// Rayon minimal de l'îlot central d'un rond-point, en mètres.
pub const ISLAND_MIN: f64 = 3.0;
/// Rayon maximal de l'îlot central d'un rond-point, en mètres.
pub const ISLAND_MAX: f64 = 20.0;
/// Champs autorisés dans un élément posé.
pub const ITEM_KEYS: &[&str] = &["uid", "catalog", "transform", "props"];
/// Champs autorisés dans une transformation.
pub const TRANSFORM_KEYS: &[&str] = &["origin", "basis"];
/// Au-delà de ce nombre d'erreurs, la validation des éléments s'arrête.
pub const MAX_REPORTED_ERRORS: usize = 50;

/// Type d'un réglage autorisé (`PROP_SCHEMA`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropKind {
    /// Vrai / faux.
    Bool,
    /// Entier borné (inclus).
    Int {
        /// Minimum.
        min: i64,
        /// Maximum.
        max: i64,
    },
    /// Texte court, sans chemin ni caractère de contrôle.
    Text {
        /// Longueur maximale en caractères.
        max: usize,
    },
    /// Couleur HTML (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`).
    Color,
}

/// Réglages autorisés dans `props` (liste blanche globale ; chaque fiche du catalogue la restreint encore).
pub fn prop_kind(key: &str) -> Option<PropKind> {
    match key {
        "light_on_at_night" => Some(PropKind::Bool),
        "variant" => Some(PropKind::Int { min: 0, max: 16 }),
        "text" => Some(PropKind::Text { max: 40 }),
        "color" => Some(PropKind::Color),
        _ => None,
    }
}
