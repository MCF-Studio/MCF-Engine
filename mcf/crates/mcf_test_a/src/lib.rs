//! **Test A — grosse carte** (`mcf/docs/bancs-essai.md`) : affichage dans Bevy de la carte générée par
//! `mcf_big_map`.
//!
//! - [`catalog`] : identifiant `bench_*` → modèle (emplacement dense, sans chaîne à l'exécution) ;
//! - [`models`] : modèles générés en code, un maillage fusionné par modèle, couleurs par sommet ;
//! - [`load`] : lecture stricte et parallèle du dossier (`manifest.json` + calques `.mcfmap`) ;
//! - [`MapViewPlugin`] : pose chaque élément de chaque calque, avec maillages et matériaux partagés.
//!
//! ```no_run
//! use bevy::prelude::*;
//! use mcf_test_a::{MapViewOptions, MapViewPlugin};
//!
//! App::new()
//!     .add_plugins(DefaultPlugins)
//!     .add_plugins(MapViewPlugin {
//!         dir: "bench/maps/depart_4d43465f4249475f".into(),
//!         options: MapViewOptions::default(),
//!     })
//!     .run();
//! ```

pub mod catalog;
pub mod load;
pub mod models;
mod plugin;

pub use plugin::{
    KindStats, LampLight, MapLayer, MapReport, MapViewOptions, MapViewPlugin, MapViewSet, set_lamps_lit,
};
