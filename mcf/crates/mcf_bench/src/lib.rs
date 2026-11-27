//! Outils de mesure des bancs d'essai (`mcf/docs/bancs-essai.md`).
//!
//! - [`FrameStats`] : statistiques sur les temps par image (moyenne, médiane, p99, **1 % low**, saccades) ;
//! - [`CameraPath`] : parcours caméra rejouable à l'identique d'une exécution à l'autre ;
//! - [`BenchPlugin`] : rejoue le parcours, enregistre chaque image, écrit les résultats (dont le temps GPU de
//!   chaque passe, `passes.csv`) puis quitte.

mod metrics;
mod path;
mod plugin;
mod stats;

pub use metrics::{
    FRAMES_CSV_HEADER, FrameSample, PASSES_CSV_HEADER, RenderTotals, aggregate_render, frames_csv,
    gpu_par_passe, passes_csv,
};
pub use path::{CameraKey, CameraPath};
pub use plugin::{BenchCamera, BenchPlugin, BenchSettings};
pub use stats::{FrameStats, SeriesStats};
