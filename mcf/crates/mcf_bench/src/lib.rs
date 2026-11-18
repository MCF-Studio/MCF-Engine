//! Outils de mesure des bancs d'essai (`mcf/docs/bancs-essai.md`).
//!
//! - [`FrameStats`] : statistiques sur les temps par image (moyenne, médiane, p99, **1 % low**, saccades) ;
//! - [`CameraPath`] : parcours caméra rejouable à l'identique d'une exécution à l'autre ;
//! - [`BenchPlugin`] : rejoue le parcours, enregistre chaque image, écrit les résultats puis quitte.

mod path;
mod plugin;
mod stats;

pub use path::{CameraKey, CameraPath};
pub use plugin::{BenchCamera, BenchPlugin, BenchSettings};
pub use stats::FrameStats;
