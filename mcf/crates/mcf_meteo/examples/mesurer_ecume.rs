//! Mesure la part de la mer couverte d'écume, à comparer à la loi de Monahan (couverture des moutons selon le
//! vent à 10 m : W = 3,84·10⁻⁶ U^3,41). Laisse tourner la simulation (l'écume s'accumule puis décroît), relit
//! l'écume des cascades FFT sur le GPU, puis évalue sur le CPU la formule d'écume du matériau (`ocean.wesl`,
//! hors rivage) sur une grille de 250 m × 250 m.
//!
//! `MCF_PRESET=clair|nuageux|pluie|tempete cargo run -p mcf_meteo --example mesurer_ecume`

use bevy::math::ops;
use bevy::prelude::*;
use bevy::render::gpu_readback::{ReadbackComplete, ReadbackOnce};
use bevy::window::WindowResolution;
use mcf_meteo::mer_du_vent::N_FFT;
use mcf_meteo::ocean_fft::TexturesOcean;
use mcf_meteo::{
    ChangerMeteo, EtatMeteo, McfMeteoPlugin, McfOceanPlugin, Ocean, PresetMeteo, ReglagesFft, ReglagesOcean,
};

/// Durée de simulation avant la relecture (s) : l'écume atteint son régime établi.
const DUREE_S: f32 = 25.0;
/// Seuil d'intensité au-delà duquel un point compte comme écume.
const SEUIL: f32 = 0.5;

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

fn preset() -> (PresetMeteo, &'static str) {
    match std::env::var("MCF_PRESET").as_deref() {
        Ok("clair") => (PresetMeteo::Clair, "clair"),
        Ok("nuageux") => (PresetMeteo::Nuageux, "nuageux"),
        Ok("pluie") => (PresetMeteo::Pluie, "pluie"),
        _ => (PresetMeteo::Tempete, "tempete"),
    }
}

fn scene(mut commandes: Commands, mut demandes: MessageWriter<ChangerMeteo>) {
    commandes.spawn((Camera3d::default(), Transform::from_xyz(0.0, 10.0, 0.0)));
    demandes.write(ChangerMeteo {
        cible: preset().0.etat(),
        duree_s: 0.0,
    });
}

/// Monahan & O'Muircheartaigh (1980) : part de la surface couverte de moutons, vent à 10 m en m/s.
fn monahan(u10: f32) -> f32 {
    3.84e-6 * ops::powf(u10.max(0.0), 3.41)
}

#[derive(Clone)]
struct Instant {
    ocean: Ocean,
    tailles: [f32; 3],
    vent: f32,
    intensite: f32,
}

fn piloter(
    mut commandes: Commands,
    mut lance: Local<bool>,
    mut virtuel: ResMut<Time<Virtual>>,
    textures: Res<TexturesOcean>,
    etat: Res<EtatMeteo>,
    ocean: Res<Ocean>,
    reglages: Res<ReglagesFft>,
    reglages_ocean: Option<Res<ReglagesOcean>>,
) {
    if *lance || virtuel.elapsed_secs() < DUREE_S {
        return;
    }
    *lance = true;
    virtuel.pause();
    let instant = Instant {
        ocean: ocean.clone(),
        tailles: reglages.tailles,
        vent: etat.vent_vitesse,
        intensite: reglages_ocean.map_or(1.0, |r| r.ecume),
    };
    commandes
        .spawn(ReadbackOnce::texture(textures.deplacement.clone()))
        .observe(
            move |lecture: On<ReadbackComplete>, mut sortie: MessageWriter<AppExit>| {
                mesurer(&lecture.data, &instant);
                sortie.write(AppExit::Success);
            },
        );
}

fn puissance_de_2(n: i32) -> f32 {
    f32::from_bits(((n + 127) as u32) << 23)
}

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

/// Écume d'une cascade (canal w), lecture bilinéaire répétée comme l'échantillonneur du matériau.
fn ecume_cascade(donnees: &[u8], c: usize, uv: Vec2) -> f32 {
    let n = N_FFT as i32;
    let p = uv * n as f32 - 0.5;
    let i = p.floor();
    let f = p - i;
    let lire = |x: i32, y: i32| {
        let (x, y) = (x.rem_euclid(n) as usize, y.rem_euclid(n) as usize);
        let o = ((c * n as usize + y) * n as usize + x) * 8 + 6;
        f16_vers_f32(u16::from_le_bytes([donnees[o], donnees[o + 1]]))
    };
    let (x, y) = (i.x as i32, i.y as i32);
    let haut = lire(x, y) * (1.0 - f.x) + lire(x + 1, y) * f.x;
    let bas = lire(x, y + 1) * (1.0 - f.x) + lire(x + 1, y + 1) * f.x;
    haut * (1.0 - f.y) + bas * f.y
}

fn hachage(x: u32) -> u32 {
    let mut x = x;
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

fn valeur(c: IVec2) -> f32 {
    (hachage(c.x as u32 ^ hachage(c.y as u32 ^ 0x5bd1_e995)) & 0xFFFF) as f32 / 65535.0
}

fn bruit_valeur(p: Vec2) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    let i = i.as_ivec2();
    let a = valeur(i) + (valeur(i + IVec2::X) - valeur(i)) * u.x;
    let b = valeur(i + IVec2::Y) + (valeur(i + IVec2::ONE) - valeur(i + IVec2::Y)) * u.x;
    a + (b - a) * u.y
}

fn lisse(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Compression horizontale de la houle de Gerstner (comme `gerstner(xz).crete` dans `ocean.wesl`).
fn crete(ocean: &Ocean, xz: Vec2) -> f32 {
    ocean
        .vagues()
        .iter()
        .map(|v| {
            let th = v.k * v.direction.dot(xz) - v.omega * ocean.temps() + v.phase;
            v.raideur * v.k * v.amplitude * ops::sin(th)
        })
        .sum()
}

fn mesurer(donnees: &[u8], i: &Instant) {
    let n = N_FFT as usize;
    if donnees.len() < n * n * 3 * 8 {
        error!("relecture trop courte : {} octets", donnees.len());
        return;
    }
    let cote = 400;
    let (mut fft_seule, mut houle_seule, mut total) = (0usize, 0usize, 0usize);
    let mut somme = 0.0f64;
    for y in 0..cote {
        for x in 0..cote {
            let xz = Vec2::new(x as f32, y as f32) * (250.0 / cote as f32);
            let ecume_fft = (0..3)
                .map(|c| ecume_cascade(donnees, c, xz / i.tailles[c]))
                .sum::<f32>()
                .clamp(0.0, 1.0);
            let t = i.ocean.temps();
            let motif = bruit_valeur(xz * 0.8 + Vec2::splat(t * 0.05)) * 0.6 + bruit_valeur(xz * 3.1) * 0.4;
            let ecume_houle = ((crete(&i.ocean, xz) - 0.45) * 2.5).clamp(0.0, 1.0) * lisse(4.0, 12.0, i.vent);
            let a = ecume_fft * (0.5 + 0.7 * motif);
            let b = ecume_houle * lisse(0.35, 0.75, motif);
            let e = (a.max(b) * i.intensite).clamp(0.0, 1.0);
            somme += f64::from(e);
            fft_seule += usize::from(a * i.intensite >= SEUIL);
            houle_seule += usize::from(b * i.intensite >= SEUIL);
            total += usize::from(e >= SEUIL);
        }
    }
    let aire = (cote * cote) as f32;
    println!(
        "{} : vent {:.0} m/s, écume (intensité ≥ {SEUIL}) {:.2} % (FFT {:.2} %, houle {:.2} %), intensité moyenne \
         {:.3} ; Monahan {:.2} %",
        preset().1,
        i.vent,
        total as f32 / aire * 100.0,
        fft_seule as f32 / aire * 100.0,
        houle_seule as f32 / aire * 100.0,
        somme / f64::from(aire),
        monahan(i.vent) * 100.0
    );
}
