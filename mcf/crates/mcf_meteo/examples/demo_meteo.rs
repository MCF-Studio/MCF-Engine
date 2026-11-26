//! Démo de la météo : un sol, quelques blocs, la pluie ou la neige autour de la caméra.
//!
//! `cargo run -p mcf_meteo --example demo_meteo`
//! Touches : 1 Clair, 2 Nuageux, 3 Pluie, 4 Tempête, 5 Neige (transition de 5 s).
//! `MCF_CAPTURE=fichier.png` : enregistre une capture après quelques secondes puis quitte (tests sans écran).

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use mcf_meteo::{AbriPrecipitations, ChangerMeteo, McfMeteoPlugin, McfPrecipitationsPlugin, PresetMeteo};

fn main() {
    let mut app = App::new();
    app.add_plugins((DefaultPlugins, McfMeteoPlugin, McfPrecipitationsPlugin))
        .add_systems(Startup, scene)
        .add_systems(Update, (touches, capture));
    app.run();
}

fn scene(
    mut commandes: Commands,
    mut maillages: ResMut<Assets<Mesh>>,
    mut materiaux: ResMut<Assets<StandardMaterial>>,
    mut demandes: MessageWriter<ChangerMeteo>,
) {
    commandes.spawn((
        Mesh3d(maillages.add(Plane3d::default().mesh().size(200.0, 200.0))),
        MeshMaterial3d(materiaux.add(Color::srgb(0.35, 0.33, 0.3))),
    ));
    let bloc = maillages.add(Cuboid::new(4.0, 6.0, 4.0));
    let gris = materiaux.add(Color::srgb(0.6, 0.6, 0.62));
    for (x, z) in [(-10.0, -20.0), (8.0, -30.0), (20.0, -12.0), (-25.0, -40.0)] {
        commandes.spawn((
            Mesh3d(bloc.clone()),
            MeshMaterial3d(gris.clone()),
            Transform::from_xyz(x, 3.0, z),
        ));
    }
    // Abri : un toit sur quatre poteaux. Il ne pleut pas dessous.
    let bois = materiaux.add(Color::srgb(0.45, 0.3, 0.2));
    commandes.spawn((
        Mesh3d(maillages.add(Cuboid::new(7.0, 0.3, 5.0))),
        MeshMaterial3d(bois.clone()),
        Transform::from_xyz(-3.0, 3.2, 0.0),
        AbriPrecipitations,
    ));
    let poteau = maillages.add(Cuboid::new(0.2, 3.2, 0.2));
    for (x, z) in [(-6.3, -2.3), (0.3, -2.3), (-6.3, 2.3), (0.3, 2.3)] {
        commandes.spawn((
            Mesh3d(poteau.clone()),
            MeshMaterial3d(bois.clone()),
            Transform::from_xyz(x, 1.6, z),
        ));
    }
    commandes.spawn((
        DirectionalLight {
            illuminance: 8000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(30.0, 50.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // `MCF_CAMERA=abri` : caméra sous le toit, regard vers l'extérieur.
    let camera = if std::env::var("MCF_CAMERA").as_deref() == Ok("abri") {
        Transform::from_xyz(-3.0, 1.6, -1.5).looking_at(Vec3::new(-3.0, 1.8, 10.0), Vec3::Y)
    } else {
        Transform::from_xyz(4.0, 1.7, 9.0).looking_at(Vec3::new(-3.0, 2.0, 0.0), Vec3::Y)
    };
    commandes.spawn((Camera3d::default(), camera));
    let preset = match std::env::var("MCF_PRESET").as_deref() {
        Ok("neige") => PresetMeteo::Neige,
        Ok("pluie") => PresetMeteo::Pluie,
        _ => PresetMeteo::Tempete,
    };
    demandes.write(ChangerMeteo {
        cible: preset.etat(),
        duree_s: 0.0,
    });
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
                duree_s: 5.0,
            });
        }
    }
}

fn capture(mut commandes: Commands, mut images: Local<u32>, mut sortie: MessageWriter<AppExit>) {
    let Ok(chemin) = std::env::var("MCF_CAPTURE") else {
        return;
    };
    *images += 1;
    if *images == 120 {
        commandes
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(chemin));
    }
    if *images == 140 {
        sortie.write(AppExit::Success);
    }
}
