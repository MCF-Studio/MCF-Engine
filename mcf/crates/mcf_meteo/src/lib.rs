//! Météo et eau de MCF Engine, porté de SkyForge (C++ / DirectX 12) vers Bevy.
//!
//! Étape 1 (ce crate pour l'instant) : l'état météo, ses préréglages et les transitions. Aucune dépendance au
//! rendu, pour que le serveur dédié puisse piloter la météo et la diffuser. Les étapes suivantes (vent, pluie et
//! neige, humidité, océan, nuages, son) sont décrites dans `mcf/docs/meteo.md`.
//!
//! Utilisation :
//! ```no_run
//! use bevy::prelude::*;
//! use mcf_meteo::{ChangerMeteo, McfMeteoPlugin, PresetMeteo};
//!
//! fn passer_a_la_pluie(mut demandes: MessageWriter<ChangerMeteo>) {
//!     demandes.write(ChangerMeteo { cible: PresetMeteo::Pluie.etat(), duree_s: 30.0 });
//! }
//!
//! App::new()
//!     .add_plugins((MinimalPlugins, McfMeteoPlugin))
//!     .add_systems(Startup, passer_a_la_pluie)
//!     .run();
//! ```

use bevy::prelude::*;

pub mod etat;
pub mod transition;

pub use etat::{EtatMeteo, PresetMeteo};
pub use transition::{ChangerMeteo, TransitionMeteo};

/// Ensemble de systèmes de la météo, pour ordonner les autres systèmes (rendu, son, gameplay) après lui.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MiseAJourMeteo;

/// Plugin météo : ressources [`EtatMeteo`] et [`TransitionMeteo`], message [`ChangerMeteo`].
pub struct McfMeteoPlugin;

impl Plugin for McfMeteoPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<EtatMeteo>()
            .register_type::<PresetMeteo>()
            .register_type::<TransitionMeteo>()
            .init_resource::<EtatMeteo>()
            .init_resource::<TransitionMeteo>()
            .add_message::<ChangerMeteo>()
            .add_systems(Update, transition::mettre_a_jour_meteo.in_set(MiseAJourMeteo));
    }
}
