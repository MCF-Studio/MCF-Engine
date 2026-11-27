//! Démo des vagues de bord : une plage (relief de SkyForge), la houle qui grandit, déferle et vient mourir sur le
//! sable, l'écume du rivage. Le fond est donné au moteur par une `SourceFond` (ici une fonction ; dans le jeu, le
//! terrain de la carte). Avec le son d'ambiance (mer, ressac calé sur les vagues, vent, pluie, tonnerre) et les
//! éclairs en tempête (flash par l'exposition de la caméra).
//!
//! `cargo run -p mcf_meteo --example demo_plage`
//! Touches : 1 Clair, 2 Nuageux, 3 Pluie, 4 Tempête, 5 Neige (transition de 10 s).
//! `MCF_CAPTURE=fichier.png` : capture puis quitte. `MCF_PRESET=tempete|clair|pluie|neige` (nuageux par défaut).
//! `MCF_CAMERA=haut` : vue plongeante depuis 35 m.

use bevy::asset::RenderAssetUsages;
use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::light_consts::lux;
use bevy::light::{Atmosphere, AtmosphereEnvironmentMapLight};
use bevy::math::ops;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::AtmosphereSettings;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use mcf_meteo::bruit::perlin3;
use mcf_meteo::rivage::{hauteur_eau, params_bord};
use mcf_meteo::{
    AuditeurMeteo, CarteRivage, ChangerMeteo, EtatMeteo, McfMeteoPlugin, McfNuagesPlugin, McfOceanPlugin,
    McfPrecipitationsPlugin, McfSonPlugin, MerDuVent, NuagesVolumetriques, Ocean, Orage, ParametresRivage,
    PresetMeteo, SourceFond,
};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            McfMeteoPlugin,
            McfPrecipitationsPlugin,
            McfOceanPlugin,
            McfNuagesPlugin,
            McfSonPlugin,
        ))
        .insert_resource(SourceFond::new(hauteur_sol))
        .add_systems(Startup, scene)
        .add_systems(Update, (touches, bouee, flash, capture))
        .run();
}

/// Ligne de rivage (z en fonction de x), comme SkyForge : sinuosités + bruit. La terre est vers +Z.
fn ligne_rivage(x: f32) -> f32 {
    12.0 * ops::sin(x * 0.018)
        + 7.0 * ops::sin(x * 0.047 + 1.3)
        + perlin3(Vec3::new(x * 0.01, 3.7, 0.0), 0) * 14.0
}

/// Hauteur du sol (le niveau de la mer est 0) : fond marin en pente douce, plage, arrière-plage.
fn hauteur_sol(x: f32, z: f32) -> f32 {
    let d = z - ligne_rivage(x);
    let mut h = if d < 0.0 {
        (d * 0.05).max(-12.0)
    } else if d < 70.0 {
        d * 0.045
    } else {
        3.15 + (d - 70.0) * 0.015
    };
    h += perlin3(Vec3::new(x * 0.08, 0.5, z * 0.08), 0) * 0.12;
    h
}

fn maillage_terrain(demi_largeur: f32, z0: f32, z1: f32, pas: f32) -> Mesh {
    let nx = (2.0 * demi_largeur / pas) as u32;
    let nz = ((z1 - z0) / pas) as u32;
    let mut positions = Vec::new();
    let mut normales = Vec::new();
    for j in 0..=nz {
        for i in 0..=nx {
            let x = -demi_largeur + i as f32 * pas;
            let z = z0 + j as f32 * pas;
            positions.push([x, hauteur_sol(x, z), z]);
            let e = 0.5;
            let dx = hauteur_sol(x + e, z) - hauteur_sol(x - e, z);
            let dz = hauteur_sol(x, z + e) - hauteur_sol(x, z - e);
            normales.push(Vec3::new(-dx, 2.0 * e, -dz).normalize().to_array());
        }
    }
    let mut indices = Vec::new();
    let ligne = nx + 1;
    for j in 0..nz {
        for i in 0..nx {
            let a = j * ligne + i;
            indices.extend_from_slice(&[a, a + ligne, a + 1, a + 1, a + ligne, a + ligne + 1]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normales)
        .with_inserted_indices(Indices::U32(indices))
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
        Transform::from_xyz(0.6, 0.45, -1.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    let x0 = 0.0;
    let rivage = ligne_rivage(x0);
    let transformation = match std::env::var("MCF_CAMERA").as_deref() {
        Ok("haut") => Transform::from_xyz(x0 - 20.0, 35.0, rivage + 40.0)
            .looking_at(Vec3::new(x0, 0.0, rivage - 40.0), Vec3::Y),
        _ => Transform::from_xyz(x0, hauteur_sol(x0, rivage + 14.0) + 1.7, rivage + 14.0)
            .looking_at(Vec3::new(x0 - 10.0, 0.0, rivage - 60.0), Vec3::Y),
    };
    commandes.spawn((
        Camera3d::default(),
        transformation,
        NuagesVolumetriques::default(),
        AuditeurMeteo,
        AtmosphereSettings::default(),
        Exposure { ev100: 13.0 },
        Tonemapping::AcesFitted,
        AtmosphereEnvironmentMapLight::default(),
    ));
    commandes.spawn((
        Name::new("Plage"),
        Mesh3d(maillages.add(maillage_terrain(300.0, -400.0, 200.0, 1.0))),
        MeshMaterial3d(materiaux.add(StandardMaterial {
            base_color: Color::srgb(0.78, 0.70, 0.52),
            perceptual_roughness: 0.95,
            ..default()
        })),
    ));
    commandes.spawn((
        Bouee,
        Mesh3d(maillages.add(Sphere::new(0.4))),
        MeshMaterial3d(materiaux.add(Color::srgb(0.9, 0.25, 0.1))),
        Transform::from_xyz(x0 - 6.0, 0.0, rivage - 35.0),
    ));
    let preset = match std::env::var("MCF_PRESET").as_deref() {
        Ok("tempete") => PresetMeteo::Tempete,
        Ok("clair") => PresetMeteo::Clair,
        Ok("pluie") => PresetMeteo::Pluie,
        Ok("neige") => PresetMeteo::Neige,
        _ => PresetMeteo::Nuageux,
    };
    demandes.write(ChangerMeteo {
        cible: preset.etat(),
        duree_s: 0.0,
    });
}

/// La bouée flotte avec la houle (Gerstner et mer du vent) atténuée et les vagues de bord.
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

/// Flash des éclairs : l'exposition s'ouvre brièvement (jusqu'à ×3,5 comme SkyForge).
fn flash(orage: Res<Orage>, mut cameras: Query<&mut Exposure>) {
    for mut e in &mut cameras {
        e.ev100 = 13.0 - ops::log2(1.0 + orage.eclat() * 2.5);
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
