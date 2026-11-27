//! Branchement de l'ambiance sonore sur `bevy_audio` (feature `son`) : une source audio « infinie » dont le
//! décodeur appelle [`SynthAmbiance`] par blocs de 10 ms, sur le fil audio. Le monde lui transmet chaque image
//! les entrées ([`EntreesSon`]) et les coups de tonnerre via un état partagé.
//!
//! Utilisation : ajouter [`McfSonPlugin`] et poser [`AuditeurMeteo`] sur la caméra (ou le joueur).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::audio::{AddAudioSource, ChannelCount, Decodable, SampleRate, Source};
use bevy::prelude::*;
use bevy::reflect::TypePath;

use crate::MiseAJourMeteo;
use crate::etat::EtatMeteo;
use crate::houle::Ocean;
use crate::orage::Eclair;
use crate::rivage::{CarteRivage, ParametresRivage, params_bord};
use crate::son::{EntreesSon, FREQUENCE, ReglagesSon, SynthAmbiance, calculer_entrees};

/// Images par bloc de synthèse (10 ms).
const BLOC: usize = 480;

/// Marque l'entité qui « écoute » l'ambiance (caméra ou joueur). Sans elle, l'auditeur est à l'origine.
#[derive(Component, Reflect, Debug, Clone, Copy, Default)]
#[reflect(Component, Default)]
pub struct AuditeurMeteo;

#[derive(Debug, Default)]
struct Partage {
    entrees: EntreesSon,
    reglages: ReglagesSon,
    tonnerres: Vec<(f32, f32)>,
}

/// Source audio de l'ambiance (un seul exemplaire, créé par le plugin).
#[derive(Asset, TypePath, Clone)]
pub struct AmbianceMeteo {
    partage: Arc<Mutex<Partage>>,
}

/// Décodeur : produit des échantillons stéréo entrelacés à la demande du fil audio.
pub struct DecodeurAmbiance {
    synth: SynthAmbiance,
    partage: Arc<Mutex<Partage>>,
    tampon: Vec<f32>,
    position: usize,
}

impl Iterator for DecodeurAmbiance {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.position >= self.tampon.len() {
            let (entrees, reglages) = match self.partage.lock() {
                Ok(mut p) => {
                    for (delai, proximite) in p.tonnerres.drain(..) {
                        self.synth.tonnerre(delai, proximite);
                    }
                    (p.entrees, p.reglages)
                }
                // Un fil a paniqué en tenant le verrou : on continue avec des valeurs neutres.
                Err(_) => (EntreesSon::default(), ReglagesSon::default()),
            };
            self.synth.rendre(&entrees, &reglages, &mut self.tampon);
            self.position = 0;
        }
        let v = self.tampon[self.position];
        self.position += 1;
        Some(v)
    }
}

impl Source for DecodeurAmbiance {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::MIN.saturating_add(1)
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(FREQUENCE).unwrap_or(SampleRate::MIN)
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Decodable for AmbianceMeteo {
    type Decoder = DecodeurAmbiance;

    fn decoder(&self) -> Self::Decoder {
        DecodeurAmbiance {
            synth: SynthAmbiance::default(),
            partage: self.partage.clone(),
            tampon: vec![0.0; BLOC * 2],
            position: BLOC * 2,
        }
    }
}

/// Plugin de l'ambiance sonore. Nécessite `McfMeteoPlugin` et l'`AudioPlugin` de Bevy.
pub struct McfSonPlugin;

#[derive(Resource)]
struct SourceAmbiance(Arc<Mutex<Partage>>);

impl Plugin for McfSonPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<AmbianceMeteo>()
            .register_type::<ReglagesSon>()
            .register_type::<AuditeurMeteo>()
            .init_resource::<ReglagesSon>()
            .add_systems(Startup, lancer_ambiance)
            .add_systems(Update, transmettre.after(MiseAJourMeteo));
    }
}

fn lancer_ambiance(mut commandes: Commands, mut sources: ResMut<Assets<AmbianceMeteo>>) {
    let partage = Arc::new(Mutex::new(Partage::default()));
    let source = sources.add(AmbianceMeteo {
        partage: partage.clone(),
    });
    commandes.insert_resource(SourceAmbiance(partage));
    commandes.spawn((Name::new("Ambiance météo"), AudioPlayer(source)));
}

#[allow(clippy::too_many_arguments)]
fn transmettre(
    source: Option<Res<SourceAmbiance>>,
    etat: Res<EtatMeteo>,
    ocean: Res<Ocean>,
    carte: Res<CarteRivage>,
    rivage: Res<ParametresRivage>,
    reglages: Res<ReglagesSon>,
    temps: Res<Time<Virtual>>,
    auditeurs: Query<&GlobalTransform, With<AuditeurMeteo>>,
    #[cfg(feature = "rendu")] abris: Option<Res<crate::CarteAbris>>,
    mut eclairs: MessageReader<Eclair>,
) {
    let Some(source) = source else {
        return;
    };
    let (position, droite) = auditeurs
        .iter()
        .next()
        .map(|t| (t.translation(), t.right().as_vec3()))
        .unwrap_or((Vec3::ZERO, Vec3::X));
    #[cfg(feature = "rendu")]
    let abrite = abris.is_some_and(|a| a.a_l_abri(position));
    #[cfg(not(feature = "rendu"))]
    let abrite = false;
    let entrees = calculer_entrees(
        &etat,
        &ocean,
        &carte,
        &params_bord(&etat, &rivage),
        position,
        droite,
        abrite,
        temps.is_paused(),
    );
    if let Ok(mut p) = source.0.lock() {
        p.entrees = entrees;
        p.reglages = *reglages;
        for e in eclairs.read() {
            p.tonnerres.push((e.delai_tonnerre_s, e.proximite));
        }
    }
}
