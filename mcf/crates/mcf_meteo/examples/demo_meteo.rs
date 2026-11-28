//! Démo de la météo : un sol, quelques blocs, un auvent, la pluie ou la neige autour de la caméra. Le sol, les
//! blocs et l'auvent utilisent le matériau météo (mouillé, flaques, neige).
//!
//! `cargo run -p mcf_meteo --example demo_meteo`
//! Touches : 1 Clair, 2 Nuageux, 3 Pluie, 4 Tempête, 5 Neige (transition de 5 s).
//! Variables d'environnement (captures sans écran) :
//! - `MCF_CAPTURE=fichier.png` : enregistre une capture après quelques secondes puis quitte ;
//! - `MCF_PRESET=pluie|neige` (tempête par défaut) ;
//! - `MCF_CAMERA=abri` : caméra sous l'auvent ; `MCF_CAMERA=sol` : caméra basse, tournée vers le sol et un bloc ;
//!   `MCF_CAMERA=mur` : devant un bloc (ruissellement) ;
//! - `MCF_ECLABOUSSURES=0` : sans les éclaboussures de la pluie (comparaison) ;
//! - `MCF_SOL=1` : sol déjà détrempé (ou enneigé) au démarrage.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use mcf_meteo::{
    AbriPrecipitations, CarteAbris, ChangerMeteo, EclaboussuresPluie, MateriauMeteo, McfMateriauMeteoPlugin,
    McfMeteoPlugin, McfPrecipitationsPlugin, PresetMeteo, SurfacesMeteo, materiau_meteo,
};

fn main() {
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins,
        McfMeteoPlugin,
        McfPrecipitationsPlugin,
        McfMateriauMeteoPlugin,
    ))
    .add_systems(Startup, scene)
    .add_systems(Update, (touches, capture));
    app.run();
}

fn materiau(couleur: Color, rugosite: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: couleur,
        perceptual_roughness: rugosite,
        ..default()
    }
}

fn scene(
    mut commandes: Commands,
    mut maillages: ResMut<Assets<Mesh>>,
    mut materiaux: ResMut<Assets<MateriauMeteo>>,
    carte: Res<CarteAbris>,
    mut surfaces: ResMut<SurfacesMeteo>,
    mut demandes: MessageWriter<ChangerMeteo>,
) {
    let sol = materiaux.add(materiau_meteo(
        materiau(Color::srgb(0.35, 0.33, 0.3), 0.9),
        0.7,
        1.0,
        &carte,
    ));
    commandes.spawn((
        Mesh3d(maillages.add(Plane3d::default().mesh().size(200.0, 200.0))),
        MeshMaterial3d(sol),
    ));
    let bloc = maillages.add(Cuboid::new(4.0, 6.0, 4.0));
    let gris = materiaux.add(materiau_meteo(
        materiau(Color::srgb(0.6, 0.6, 0.62), 0.8),
        0.4,
        0.0,
        &carte,
    ));
    for (x, z) in [(-10.0, -20.0), (8.0, -30.0), (20.0, -12.0), (-25.0, -40.0)] {
        commandes.spawn((
            Mesh3d(bloc.clone()),
            MeshMaterial3d(gris.clone()),
            Transform::from_xyz(x, 3.0, z),
        ));
    }
    // Abri : un toit sur quatre poteaux. Il ne pleut pas dessous, le sol y reste sec.
    let bois = materiaux.add(materiau_meteo(
        materiau(Color::srgb(0.45, 0.3, 0.2), 0.7),
        0.6,
        0.0,
        &carte,
    ));
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
    let camera = match std::env::var("MCF_CAMERA").as_deref() {
        Ok("abri") => Transform::from_xyz(-3.0, 1.6, -1.5).looking_at(Vec3::new(-3.0, 1.8, 10.0), Vec3::Y),
        // Près du sol, devant un poteau et le bord du toit : les éclaboussures se voient de près.
        // Devant un bloc : le ruissellement sur ses faces.
        Ok("mur") => Transform::from_xyz(-6.0, 1.7, -14.5).looking_at(Vec3::new(-9.0, 2.2, -18.0), Vec3::Y),
        Ok("sol") => Transform::from_xyz(2.5, 0.6, 5.5).looking_at(Vec3::new(0.3, 0.3, 2.3), Vec3::Y),
        _ => Transform::from_xyz(4.0, 1.7, 9.0).looking_at(Vec3::new(-3.0, 2.0, 0.0), Vec3::Y),
    };
    let vue = commandes.spawn((Camera3d::default(), camera)).id();
    if std::env::var("MCF_ECLABOUSSURES").as_deref() != Ok("0") {
        commandes.entity(vue).insert(EclaboussuresPluie);
    }
    let preset = match std::env::var("MCF_PRESET").as_deref() {
        Ok("neige") => PresetMeteo::Neige,
        Ok("pluie") => PresetMeteo::Pluie,
        _ => PresetMeteo::Tempete,
    };
    if std::env::var("MCF_SOL").is_ok() {
        if preset == PresetMeteo::Neige {
            surfaces.neige = 1.0;
        } else {
            surfaces.mouillage = 1.0;
            surfaces.flaques = 0.8;
        }
    }
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
