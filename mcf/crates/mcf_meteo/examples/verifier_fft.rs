//! Vérifie que la réplique CPU de la mer du vent ([`mcf_meteo::mer_du_vent`]) donne le même champ que les cascades
//! FFT du GPU : relit la texture de déplacement (3 couches) et la compare, texel par texel, à la somme des modes
//! calculée sur le CPU au même instant.
//!
//! `cargo run -p mcf_meteo --example verifier_fft` (`MCF_PRESET=clair|nuageux|tempete`, tempête par défaut).
//! Code de sortie non nul si l'écart dépasse 2 % de l'amplitude d'une cascade.

use bevy::prelude::*;
use bevy::render::gpu_readback::{ReadbackComplete, ReadbackOnce};
use bevy::window::WindowResolution;
use mcf_meteo::mer_du_vent::{N_FFT, ParamsSpectre, Spectre};
use mcf_meteo::ocean_fft::TexturesOcean;
use mcf_meteo::{
    ChangerMeteo, EtatMeteo, McfMeteoPlugin, McfOceanPlugin, MerDuVent, Ocean, PresetMeteo, ReglagesFft,
};

/// Image à laquelle le temps est figé, puis image de la relecture.
const IMAGE_PAUSE: u32 = 40;
const IMAGE_LECTURE: u32 = 50;
/// Un texel sur `PAS` (dans chaque direction) est comparé.
const PAS: usize = 16;

fn main() -> AppExit {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: WindowResolution::new(320, 180),
                    ..default()
                }),
                ..default()
            }),
            McfMeteoPlugin,
            McfOceanPlugin,
        ))
        .add_systems(Startup, scene)
        .add_systems(Update, piloter)
        .run()
}

fn scene(mut commandes: Commands, mut demandes: MessageWriter<ChangerMeteo>) {
    commandes.spawn((Camera3d::default(), Transform::from_xyz(0.0, 10.0, 0.0)));
    let preset = match std::env::var("MCF_PRESET").as_deref() {
        Ok("clair") => PresetMeteo::Clair,
        Ok("nuageux") => PresetMeteo::Nuageux,
        _ => PresetMeteo::Tempete,
    };
    demandes.write(ChangerMeteo {
        cible: preset.etat(),
        duree_s: 0.0,
    });
}

#[derive(Resource, Clone)]
struct Reference {
    params: ParamsSpectre,
    temps: f32,
    choppiness: f32,
}

fn piloter(
    mut commandes: Commands,
    mut image: Local<u32>,
    mut virtuel: ResMut<Time<Virtual>>,
    textures: Res<TexturesOcean>,
    etat: Res<EtatMeteo>,
    ocean: Res<Ocean>,
    reglages: Res<ReglagesFft>,
    mer: Res<MerDuVent>,
) {
    *image += 1;
    if *image == IMAGE_PAUSE {
        // Temps figé : la texture relue correspond à cet instant, quelle que soit la latence de la relecture.
        virtuel.pause();
    }
    if *image != IMAGE_LECTURE {
        return;
    }
    let reference = Reference {
        params: ParamsSpectre::depuis(&etat, &reglages),
        temps: ocean.temps(),
        choppiness: reglages.choppiness,
    };
    info!(
        "instant {:.3} s, vent {:.1} m/s, génération du spectre {}",
        reference.temps,
        reference.params.u10,
        mer.generation()
    );
    commandes
        .spawn(ReadbackOnce::texture(textures.deplacement.clone()))
        .observe(
            move |lecture: On<ReadbackComplete>, mut sortie: MessageWriter<AppExit>| {
                let ok = comparer(&lecture.data, &reference);
                sortie.write(if ok { AppExit::Success } else { AppExit::error() });
            },
        );
}

/// 2^n exact, pour n dans [-126, 127].
fn puissance_de_2(n: i32) -> f32 {
    f32::from_bits(((n + 127) as u32) << 23)
}

/// Demi-flottant IEEE 754 → f32.
fn f16_vers_f32(bits: u16) -> f32 {
    let signe = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exposant = i32::from((bits >> 10) & 0x1F);
    let mantisse = f32::from(bits & 0x3FF);
    match exposant {
        0 => signe * mantisse * puissance_de_2(-24),
        31 => f32::NAN,
        e => signe * (1.0 + mantisse / 1024.0) * puissance_de_2(e - 15),
    }
}

fn comparer(donnees: &[u8], r: &Reference) -> bool {
    let n = N_FFT as usize;
    let attendu = n * n * 3 * 8;
    if donnees.len() < attendu {
        error!(
            "relecture trop courte : {} octets, {attendu} attendus",
            donnees.len()
        );
        return false;
    }
    let mut ok = true;
    for c in 0..3 {
        let spectre = Spectre::cascade_complete(&r.params, c);
        let taille = r.params.tailles[c];
        let (mut err2, mut val2, mut err_max) = (0.0f64, 0.0f64, 0.0f32);
        let mut points = 0;
        for y in (0..n).step_by(PAS) {
            for x in (0..n).step_by(PAS) {
                let o = ((c * n + y) * n + x) * 8;
                let lire =
                    |i: usize| f16_vers_f32(u16::from_le_bytes([donnees[o + 2 * i], donnees[o + 2 * i + 1]]));
                let gpu = Vec3::new(lire(0), lire(1), lire(2));
                let xz = Vec2::new(x as f32, y as f32) * taille / n as f32;
                let cpu = spectre.deplacement(usize::MAX, r.temps, xz, r.choppiness);
                let e = (gpu - cpu).length();
                err2 += f64::from(e * e);
                val2 += f64::from(gpu.length_squared());
                err_max = err_max.max(e);
                points += 1;
            }
        }
        let rms = (val2 / f64::from(points)).sqrt();
        let rms_err = (err2 / f64::from(points)).sqrt();
        let relatif = if rms > 0.0 { rms_err / rms } else { 0.0 };
        info!(
            "cascade {c} ({taille} m, {} modes) : amplitude RMS {rms:.4} m, écart RMS {rms_err:.5} m ({:.2} %), \
             écart max {err_max:.5} m",
            spectre.modes.len(),
            relatif * 100.0
        );
        if relatif > 0.02 {
            ok = false;
        }
    }
    if ok {
        info!("réplique CPU conforme au GPU");
    } else {
        error!("réplique CPU différente du GPU");
    }
    ok
}
