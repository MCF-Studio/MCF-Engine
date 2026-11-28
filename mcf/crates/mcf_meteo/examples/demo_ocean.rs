//! Démo de l'océan et du ciel : la houle selon la météo, les nuages volumétriques, avec l'atmosphère de Bevy pour
//! le ciel et les reflets.
//!
//! `cargo run -p mcf_meteo --example demo_ocean`
//! Touches : 1 Clair, 2 Nuageux, 3 Pluie, 4 Tempête, 5 Neige (transition de 10 s) ; flèches haut / bas : monter
//! ou descendre (400 m/s, pour traverser la couche de nuages).
//! `MCF_CAPTURE=fichier.png` : capture puis quitte. `MCF_PRESET=tempete|clair|pluie` (nuageux par défaut).
//! `MCF_CAMERA=ciel` (regard vers le haut), `MCF_CAMERA=eau` (près de la surface, impacts de la pluie), `MCF_CAMERA=nuages` (dans la couche, à 2 500 m),
//! `MCF_CAMERA=dessus` (au-dessus, à 5 000 m), `MCF_CAMERA=survol` (à 1 200 m, regard plongeant vers la mer).
//! `MCF_REPROJECTION=0` : nuages marchés en pleine résolution chaque image (sans reprojection temporelle).
//! `MCF_MOUVEMENT=1` : la caméra tourne de 0,5° par image (30°/s à 60 i/s), pour juger les traînées.
//! `MCF_SOLEIL=haut` : soleil haut (~55°) au lieu du soleil bas de fin d'après-midi.
//! `MCF_CIEL=0` : reflets et lumière ambiante sans les nuages (comparaison).
//! `MCF_MSAA=0` : sans anticrénelage MSAA (×4 par défaut).
//! `MCF_OMBRE=0` : sans l'ombre des nuages au sol (comparaison).

use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::light_consts::lux;
use bevy::light::{Atmosphere, AtmosphereEnvironmentMapLight};
use bevy::pbr::AtmosphereSettings;
use bevy::prelude::*;
use bevy::render::gpu_readback::{ReadbackComplete, ReadbackOnce};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use mcf_meteo::ombre_nuages::TAILLE_OMBRE;
use mcf_meteo::rivage::{hauteur_eau, params_bord};
use mcf_meteo::{
    CarteRivage, ChangerMeteo, EclaboussuresPluie, EtatMeteo, McfMeteoPlugin, McfNuagesPlugin,
    McfOceanPlugin, McfPrecipitationsPlugin, MerDuVent, NuagesVolumetriques, Ocean, OmbreNuages,
    ParametresRivage, PresetMeteo, ReglagesOmbreNuages,
};

/// Exposition selon la couverture nuageuse (voir `EtatMeteo::exposition_ev100`).
fn exposition(etat: Res<EtatMeteo>, mut cameras: Query<&mut Exposure>) {
    for mut e in &mut cameras {
        e.ev100 = etat.exposition_ev100();
    }
}

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            McfMeteoPlugin,
            McfPrecipitationsPlugin,
            McfOceanPlugin,
            McfNuagesPlugin,
        ))
        // `MCF_OMBRE=0` : sans l'ombre des nuages (comparaison).
        .insert_resource(ReglagesOmbreNuages {
            active: std::env::var("MCF_OMBRE").as_deref() != Ok("0"),
            ..default()
        })
        .add_systems(Startup, scene)
        .add_systems(Update, (touches, voler, bouee, exposition, capture))
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
        // `MCF_SOLEIL=haut` : soleil à ~55° (16° par défaut, fin d'après-midi).
        if std::env::var("MCF_SOLEIL").as_deref() == Ok("haut") {
            Transform::from_xyz(1.0, 1.65, -0.6).looking_at(Vec3::ZERO, Vec3::Y)
        } else {
            Transform::from_xyz(1.0, 0.35, -0.6).looking_at(Vec3::ZERO, Vec3::Y)
        },
    ));
    let transformation = match std::env::var("MCF_CAMERA").as_deref() {
        Ok("ciel") => Transform::from_xyz(0.0, 6.0, 20.0).looking_at(Vec3::new(0.0, 40.0, -60.0), Vec3::Y),
        // Près de l'eau, regard vers la surface : les impacts de la pluie sur la mer.
        Ok("eau") => Transform::from_xyz(0.0, 2.2, 20.0).looking_at(Vec3::new(0.0, 0.0, 12.0), Vec3::Y),
        Ok("nuages") => {
            Transform::from_xyz(0.0, 2500.0, 0.0).looking_at(Vec3::new(200.0, 2450.0, -1000.0), Vec3::Y)
        }
        // Sous la base des nuages, regard plongeant : l'ombre des nuages sur la mer.
        Ok("survol") => {
            Transform::from_xyz(0.0, 1200.0, 0.0).looking_at(Vec3::new(0.0, 0.0, -2500.0), Vec3::Y)
        }
        Ok("dessus") => {
            Transform::from_xyz(0.0, 5000.0, 0.0).looking_at(Vec3::new(500.0, 4300.0, -3000.0), Vec3::Y)
        }
        _ => Transform::from_xyz(0.0, 6.0, 20.0).looking_at(Vec3::new(0.0, 2.0, -60.0), Vec3::Y),
    };
    commandes.spawn((
        Camera3d::default(),
        transformation,
        NuagesVolumetriques {
            // `MCF_REPROJECTION=0` : chaque pixel marché à chaque image (comparaison).
            reprojection: std::env::var("MCF_REPROJECTION").as_deref() != Ok("0"),
            // `MCF_CIEL=0` : reflets et lumière ambiante sans les nuages (comparaison).
            ciel: std::env::var("MCF_CIEL").as_deref() != Ok("0"),
            ..default()
        },
        AtmosphereSettings::default(),
        Exposure { ev100: 13.0 },
        // `MCF_MSAA=0` : sans anticrénelage (MSAA ×4 par défaut, comme toute caméra Bevy).
        if std::env::var("MCF_MSAA").as_deref() == Ok("0") {
            Msaa::Off
        } else {
            Msaa::Sample4
        },
        Tonemapping::AcesFitted,
        AtmosphereEnvironmentMapLight::default(),
        EclaboussuresPluie,
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
        Ok("pluie") => PresetMeteo::Pluie,
        _ => PresetMeteo::Nuageux,
    };
    demandes.write(ChangerMeteo {
        cible: preset.etat(),
        duree_s: 0.0,
    });
}

/// La bouée flotte sur la houle et la mer du vent (réplique CPU des cascades FFT).
fn bouee(
    ocean: Res<Ocean>,
    mer: Res<MerDuVent>,
    carte: Res<CarteRivage>,
    etat: Res<EtatMeteo>,
    parametres: Res<ParametresRivage>,
    mut bouees: Query<&mut Transform, With<Bouee>>,
) {
    let b = params_bord(&etat, &parametres);
    for mut t in &mut bouees {
        t.translation.y = hauteur_eau(&ocean, Some(&mer), &carte, &b, t.translation.x, t.translation.z);
    }
}

fn voler(
    clavier: Res<ButtonInput<KeyCode>>,
    temps: Res<Time>,
    mut cameras: Query<&mut Transform, With<Camera3d>>,
) {
    let sens = clavier.pressed(KeyCode::ArrowUp) as i32 - clavier.pressed(KeyCode::ArrowDown) as i32;
    // Pas fixe par image (et non par seconde) : le rendu logiciel est lent, on veut le même mouvement d'une
    // image à l'autre que sur un vrai GPU à 60 i/s.
    let mouvement = std::env::var("MCF_MOUVEMENT").is_ok_and(|v| v == "1");
    for mut t in &mut cameras {
        if mouvement {
            t.rotate_y(0.5_f32.to_radians());
        }
        t.translation.y = (t.translation.y + sens as f32 * 400.0 * temps.delta_secs()).max(2.0);
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

fn capture(
    mut commandes: Commands,
    mut images: Local<u32>,
    mut sortie: MessageWriter<AppExit>,
    ombre: Res<OmbreNuages>,
) {
    let Ok(chemin) = std::env::var("MCF_CAPTURE") else {
        return;
    };
    *images += 1;
    // `MCF_OMBRE_PGM=fichier.pgm` : relit aussi la carte d'ombre des nuages (niveaux de gris, `TAILLE_OMBRE` de côté).
    if *images == 85
        && let Ok(fichier) = std::env::var("MCF_OMBRE_PGM")
    {
        commandes
            .spawn(ReadbackOnce::texture(ombre.image.clone()))
            .observe(move |lecture: On<ReadbackComplete>| {
                let n = TAILLE_OMBRE as usize;
                let mut pgm = format!("P5 {n} {n} 255\n").into_bytes();
                pgm.extend(lecture.data.chunks(4).take(n * n).map(|p| p[0]));
                let _ = std::fs::write(&fichier, pgm);
            });
    }
    if *images == 90 {
        commandes
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(chemin));
    }
    if *images == 110 {
        sortie.write(AppExit::Success);
    }
}
