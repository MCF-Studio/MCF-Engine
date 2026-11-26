//! Météo et eau de MCF Engine, porté de SkyForge (C++ / DirectX 12) vers Bevy.
//!
//! Contenu actuel : l'état météo, ses préréglages et les transitions (étape 1), le champ de vent 3D évalué sur le
//! CPU (étape 2), et la pluie et la neige sur le GPU (étape 3, feature `rendu`, [`McfPrecipitationsPlugin`]).
//! Sans la feature `rendu`, aucune dépendance au rendu : le serveur dédié peut piloter la météo et la diffuser.
//! Les étapes suivantes (humidité, océan, nuages, son) sont décrites dans `mcf/docs/meteo.md`.
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

#[cfg(feature = "rendu")]
pub mod abris;
pub mod bruit;
pub mod etat;
#[cfg(feature = "rendu")]
pub mod precipitations;
pub mod transition;
pub mod vent;

#[cfg(feature = "rendu")]
pub use abris::{AbriPrecipitations, CarteAbris};
pub use etat::{EtatMeteo, PresetMeteo};
#[cfg(feature = "rendu")]
pub use precipitations::{McfPrecipitationsPlugin, ReglagesPrecipitations};
pub use transition::{ChangerMeteo, TransitionMeteo};
pub use vent::{ChampVent, EchantillonVent, ParametresVent};

/// Ensemble de systèmes de la météo, pour ordonner les autres systèmes (rendu, son, gameplay) après lui.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MiseAJourMeteo;

/// Plugin météo : ressources [`EtatMeteo`], [`TransitionMeteo`] et [`ChampVent`], message [`ChangerMeteo`].
pub struct McfMeteoPlugin;

impl Plugin for McfMeteoPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<EtatMeteo>()
            .register_type::<PresetMeteo>()
            .register_type::<TransitionMeteo>()
            .register_type::<ChampVent>()
            .register_type::<ParametresVent>()
            .init_resource::<EtatMeteo>()
            .init_resource::<TransitionMeteo>()
            .init_resource::<ChampVent>()
            .add_message::<ChangerMeteo>()
            .add_systems(
                Update,
                (transition::mettre_a_jour_meteo, vent::avancer_vent)
                    .chain()
                    .in_set(MiseAJourMeteo),
            );
    }
}
