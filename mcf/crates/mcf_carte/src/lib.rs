//! Projet de carte MCF (ADR-0001, `docs/SPEC_CONCEPTION_CARTE_2D_3D.md`) : sources (manifeste, relief, réseau,
//! quartiers, bâti, calques d'objets), dérivés (géométrie, îlots, terrain terrassé), historique, fichiers validés
//! et exports. Sans Bevy : l'éditeur, le serveur et les outils en ligne de commande partagent ce crate.

pub mod batiments;
pub mod carte;
pub mod carte_de_test;
pub mod catalogue;
pub mod contours;
pub mod conversion;
pub mod export;
pub mod gabarits;
pub mod geometrie;
pub mod graphe;
pub mod ids;
pub mod ilots;
pub mod io;
pub mod lotissement;
pub mod manifeste;
pub mod quartiers;
pub mod references;
pub mod releve;
pub mod relief;
pub mod relief_image;
pub mod reseau;

pub use batiments::{Batiment, ModelePose, Origine, Transformation};
pub use carte::{Carte, Derive, Revisions, Sale, Transaction};
pub use catalogue::{CatalogueBatiments, Fiche};
pub use contours::{Contour, Contours, UsageContour};
pub use gabarits::{Densite, Famille, Gabarit, TableGabarits};
pub use geometrie::GeoReseau;
pub use graphe::{AreteGraphe, GrapheRoutier, Itineraire, NoeudGraphe};
pub use ids::Id;
pub use manifeste::Manifeste;
pub use references::{ImageRgba, Reference, References};
pub use relief::{Pinceau, Relief};
pub use reseau::Reseau;
