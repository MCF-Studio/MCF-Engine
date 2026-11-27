//! Météo et eau de MCF Engine, porté de SkyForge (C++ / DirectX 12) vers Bevy.
//!
//! Contenu actuel : l'état météo, ses préréglages et les transitions (étape 1), le champ de vent 3D évalué sur le
//! CPU (étape 2), la pluie et la neige sur le GPU (étape 3, feature `rendu`, [`McfPrecipitationsPlugin`]), les
//! abris (étape 4a) et les surfaces mouillées, flaques et neige au sol (étape 4b, [`SurfacesMeteo`],
//! `McfMateriauMeteoPlugin`), puis l'océan : houle de Gerstner calée sur JONSWAP ([`Ocean`], aussi pour la
//! flottaison) et son rendu (étape 5a, `McfOceanPlugin`), avec les cascades FFT de la mer du vent (étape 5b) et
//! les vagues de bord ([`rivage`], étape 6), et enfin les nuages volumétriques traversables (étape 7,
//! `McfNuagesPlugin`). Ambiance sonore synthétisée (mer, ressac, vent, pluie, tonnerre) et éclairs (étape 8,
//! [`son`], [`Orage`], `McfSonPlugin` avec la feature `son`).
//! Sans la feature `rendu`, aucune dépendance au rendu : le serveur dédié peut piloter la météo et la diffuser.
//! L'étape suivante (banc d'essai) est décrite dans `mcf/docs/meteo.md`.
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
#[cfg(feature = "son")]
pub mod audio;
pub mod bruit;
pub mod etat;
pub mod houle;
#[cfg(feature = "rendu")]
pub mod materiau_meteo;
#[cfg(feature = "rendu")]
pub mod nuages;
#[cfg(feature = "rendu")]
pub mod ocean;
#[cfg(feature = "rendu")]
pub mod ocean_fft;
pub mod orage;
#[cfg(feature = "rendu")]
pub mod precipitations;
pub mod rivage;
pub mod son;
pub mod surfaces;
pub mod transition;
pub mod vent;

#[cfg(feature = "rendu")]
pub use abris::{AbriPrecipitations, CarteAbris};
#[cfg(feature = "son")]
pub use audio::{AuditeurMeteo, McfSonPlugin};
pub use etat::{EtatMeteo, PresetMeteo};
pub use houle::Ocean;
#[cfg(feature = "rendu")]
pub use materiau_meteo::{MateriauMeteo, McfMateriauMeteoPlugin, materiau_meteo};
#[cfg(feature = "rendu")]
pub use nuages::{McfNuagesPlugin, NuagesVolumetriques, ReglagesNuages};
#[cfg(feature = "rendu")]
pub use ocean::{McfOceanPlugin, ReglagesOcean};
#[cfg(feature = "rendu")]
pub use ocean_fft::ReglagesFft;
pub use orage::{Eclair, Orage};
#[cfg(feature = "rendu")]
pub use precipitations::{McfPrecipitationsPlugin, ReglagesPrecipitations};
pub use rivage::{CarteRivage, ParametresRivage, SourceFond};
pub use son::ReglagesSon;
pub use surfaces::{ParametresSurfaces, SurfacesMeteo};
pub use transition::{ChangerMeteo, TransitionMeteo};
pub use vent::{ChampVent, EchantillonVent, ParametresVent};

/// Charge une seule fois la bibliothèque WGSL de bruit (`nuages_commun.wesl`), partagée par les nuages et l'océan.
#[cfg(feature = "rendu")]
pub(crate) fn charger_bruit_wesl(app: &mut App) {
    #[derive(Resource)]
    struct BruitWeslCharge;
    if app.world().contains_resource::<BruitWeslCharge>() {
        return;
    }
    bevy::shader::load_shader_library!(app, "nuages_commun.wesl");
    app.insert_resource(BruitWeslCharge);
}

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
            .register_type::<SurfacesMeteo>()
            .register_type::<Ocean>()
            .register_type::<ParametresSurfaces>()
            .register_type::<ParametresRivage>()
            .register_type::<Orage>()
            .init_resource::<Orage>()
            .add_message::<Eclair>()
            .init_resource::<ParametresRivage>()
            .init_resource::<CarteRivage>()
            .init_resource::<EtatMeteo>()
            .init_resource::<TransitionMeteo>()
            .init_resource::<ChampVent>()
            .init_resource::<SurfacesMeteo>()
            .init_resource::<Ocean>()
            .add_message::<ChangerMeteo>()
            .add_systems(
                Update,
                (
                    transition::mettre_a_jour_meteo,
                    vent::avancer_vent,
                    surfaces::avancer_surfaces,
                    houle::avancer_ocean,
                    orage::avancer_orage,
                )
                    .chain()
                    .in_set(MiseAJourMeteo),
            );
    }
}
