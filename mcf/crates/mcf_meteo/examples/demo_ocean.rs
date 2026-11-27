//! Démo de l'océan : la houle selon la météo, avec l'atmosphère de Bevy pour le ciel et les reflets.
//!
//! `cargo run -p mcf_meteo --example demo_ocean`
//! Touches : 1 Clair, 2 Nuageux, 3 Pluie, 4 Tempête, 5 Neige (transition de 10 s).
//! `MCF_CAPTURE=fichier.png` : capture puis quitte. `MCF_PRESET=tempete|clair` (nuageux par défaut).

use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::light_consts::lux;
use bevy::light::{Atmosphere, AtmosphereEnvironmentMapLight};
use bevy::pbr::AtmosphereSettings;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use mcf_meteo::{ChangerMeteo, McfMeteoPlugin, McfOceanPlugin, McfPrecipitationsPlugin, Ocean, PresetMeteo};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            McfMeteoPlugin,
            McfPrecipitationsPlugin,
            McfOceanPlugin,
        ))
        .add_systems(Startup, scene)
        .add_systems(Update, (touches, bouee, capture))
        .run();
}

#[derive(Component)]
struct Bouee;

fn scene(
    mut commandes: Commands,
    mut milieux: ResMut<Assets<ScatteringMedium>>,
    mut maillages: ResMut<Assets<Mesh>>,
    mut materiaux: ResMut<Assets<StandardMaterial>>,
    mut demandes: MessageWriter<ChangerMeteo>,
) {
    commandes.spawn(Atmosphere::earth(milieux.add(ScatteringMedium::earth(256, 256))));
    commandes.spawn((
        DirectionalLight {
            illuminance: lux::RAW_SUNLIGHT,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(1.0, 0.35, -0.6).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commandes.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 6.0, 20.0).looking_at(Vec3::new(0.0, 2.0, -60.0), Vec3::Y),
        AtmosphereSettings::default(),
        Exposure { ev100: 13.0 },
        Tonemapping::AcesFitted,
        AtmosphereEnvironmentMapLight::default(),
    ));
    // Une bouée qui flotte grâce à `Ocean::hauteur`.
    commandes.spawn((
        Bouee,
        Mesh3d(maillages.add(Sphere::new(0.8))),
        MeshMaterial3d(materiaux.add(Color::srgb(0.9, 0.25, 0.1))),
        Transform::from_xyz(-4.0, 0.0, -10.0),
    ));
    let preset = match std::env::var("MCF_PRESET").as_deref() {
        Ok("tempete") => PresetMeteo::Tempete,
        Ok("clair") => PresetMeteo::Clair,
        _ => PresetMeteo::Nuageux,
    };
    demandes.write(ChangerMeteo {
        cible: preset.etat(),
        duree_s: 0.0,
    });
}

fn bouee(ocean: Res<Ocean>, mut bouees: Query<&mut Transform, With<Bouee>>) {
    for mut t in &mut bouees {
        t.translation.y = ocean.hauteur(t.translation.x, t.translation.z);
    }
}

fn touches(clavier: Res<ButtonInput<KeyCode>>, mut demandes: MessageWriter<ChangerMeteo>) {
    let choix = [
        (KeyCode::Digit1, PresetMeteo::Clair),
        (KeyCode::Digit2, PresetMeteo::Nuageux),
        (KeyCode::Digit3, PresetMeteo::Pluie),
        (KeyCode::Digit4, PresetMeteo::Tempete),
        (KeyCode::Digit5, PresetMeteo::Neige),
    ];
    for (touche, preset) in choix {
        if clavier.just_pressed(touche) {
            demandes.write(ChangerMeteo {
                cible: preset.etat(),
                duree_s: 10.0,
            });
        }
    }
}

fn capture(mut commandes: Commands, mut images: Local<u32>, mut sortie: MessageWriter<AppExit>) {
    let Ok(chemin) = std::env::var("MCF_CAPTURE") else {
        return;
    };
    *images += 1;
    if *images == 90 {
        commandes
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(chemin));
    }
    if *images == 110 {
        sortie.write(AppExit::Success);
    }
}
