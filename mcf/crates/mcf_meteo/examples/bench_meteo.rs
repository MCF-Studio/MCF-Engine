//! Banc d'essai GPU de la météo : même plage que `demo_plage`, parcours caméra rejouable (plage → ras de l'eau →
//! montée dans les nuages → au-dessus → retour), avec le temps GPU de chaque passe (`passes.csv`).
//!
//! ```text
//! cargo bench-meteo -- --variante complet --preset tempete --duree 40
//! ```
//!
//! Variantes (chacune ajoute à la précédente, pour isoler le coût de chaque brique) :
//! - `base` : atmosphère, plage, soleil avec ombres, `McfMeteoPlugin` (état météo, vent) ;
//! - `ocean` : + océan FFT et vagues de bord ;
//! - `nuages` : + nuages volumétriques ;
//! - `complet` : + pluie / neige et son d'ambiance.
//!
//! Résultats dans `mcf/bench/results/<date>_meteo_<variante>/` (`summary.txt`, `frames.csv`, `passes.csv`).
//! `MCF_MSAA=0` : sans anticrénelage MSAA (×4 par défaut ; avant l'étape 18, les nuages le coupaient d'office
//! dans les variantes `nuages` et `complet`).
//! `MCF_CIEL=0` : coupe les nuages dans les reflets et la lumière ambiante (pour mesurer leur coût).
//! `MCF_OMBRE=0` : coupe l'ombre des nuages au sol (pour mesurer son coût).
//! Ne mesurer que sur un vrai GPU : un rendu logiciel (llvmpipe) ne donne que la preuve que l'outil fonctionne.

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
use bevy::window::{PresentMode, WindowResolution};
use mcf_bench::{BenchCamera, BenchPlugin, BenchSettings, CameraKey, CameraPath};
use mcf_meteo::bruit::perlin3;
use mcf_meteo::{
    AuditeurMeteo, ChangerMeteo, EclaboussuresPluie, McfMeteoPlugin, McfNuagesPlugin, McfOceanPlugin,
    McfPrecipitationsPlugin, McfSonPlugin, NuagesVolumetriques, PresetMeteo, SourceFond,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Variante {
    Base,
    Ocean,
    Nuages,
    Complet,
}

impl Variante {
    fn nom(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Ocean => "ocean",
            Self::Nuages => "nuages",
            Self::Complet => "complet",
        }
    }
}

#[derive(Resource, Debug, Clone)]
struct Args {
    variante: Variante,
    preset: PresetMeteo,
    nom_preset: String,
    duree: f32,
    largeur: u32,
    hauteur: u32,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut a = Self {
            variante: Variante::Complet,
            preset: PresetMeteo::Tempete,
            nom_preset: "tempete".to_owned(),
            duree: 40.0,
            largeur: 1920,
            hauteur: 1080,
        };
        let mut it = std::env::args().skip(1);
        while let Some(option) = it.next() {
            let mut valeur = |nom: &str| it.next().ok_or_else(|| format!("valeur manquante après {nom}"));
            match option.as_str() {
                "--variante" => {
                    a.variante = match valeur("--variante")?.as_str() {
                        "base" => Variante::Base,
                        "ocean" => Variante::Ocean,
                        "nuages" => Variante::Nuages,
                        "complet" => Variante::Complet,
                        v => return Err(format!("variante inconnue : {v} (base, ocean, nuages, complet)")),
                    }
                }
                "--preset" => {
                    let v = valeur("--preset")?;
                    a.preset = match v.as_str() {
                        "clair" => PresetMeteo::Clair,
                        "nuageux" => PresetMeteo::Nuageux,
                        "pluie" => PresetMeteo::Pluie,
                        "tempete" => PresetMeteo::Tempete,
                        "neige" => PresetMeteo::Neige,
                        _ => {
                            return Err(format!(
                                "preset inconnu : {v} (clair, nuageux, pluie, tempete, neige)"
                            ));
                        }
                    };
                    a.nom_preset = v;
                }
                "--duree" => a.duree = nombre::<f32>(&valeur("--duree")?)?.clamp(10.0, 3600.0),
                "--largeur" => a.largeur = nombre::<u32>(&valeur("--largeur")?)?.clamp(320, 7680),
                "--hauteur" => a.hauteur = nombre::<u32>(&valeur("--hauteur")?)?.clamp(240, 4320),
                autre => return Err(format!("option inconnue : {autre}")),
            }
        }
        Ok(a)
    }
}

fn nombre<T: std::str::FromStr>(s: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("nombre invalide : {s}"))
}

fn main() -> AppExit {
    let args = match Args::parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return AppExit::error();
        }
    };
    let v = args.variante;
    let mut reglages = BenchSettings::new(&format!("meteo_{}", v.nom()))
        .note("variante", v.nom())
        .note("preset", &args.nom_preset)
        .note("resolution", format!("{}x{}", args.largeur, args.hauteur));
    reglages.warmup_secs = 8.0; // cuisson du bruit des nuages et premières FFT.
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: format!("MCF Bench — météo ({})", v.nom()),
            present_mode: PresentMode::AutoNoVsync,
            resolution: WindowResolution::new(args.largeur, args.hauteur).with_scale_factor_override(1.0),
            ..default()
        }),
        ..default()
    }))
    .add_plugins((McfMeteoPlugin, BenchPlugin(reglages)))
    .insert_resource(SourceFond::new(hauteur_sol));
    if v >= Variante::Ocean {
        app.add_plugins(McfOceanPlugin);
    }
    if v >= Variante::Nuages {
        app.add_plugins(McfNuagesPlugin);
        // `MCF_OMBRE=0` : sans l'ombre des nuages au sol, pour mesurer son coût.
        app.insert_resource(mcf_meteo::ReglagesOmbreNuages {
            active: std::env::var("MCF_OMBRE").as_deref() != Ok("0"),
            ..default()
        });
    }
    if v >= Variante::Complet {
        app.add_plugins((McfPrecipitationsPlugin, McfSonPlugin));
    }
    app.insert_resource(args).add_systems(Startup, scene).run()
}

/// Ligne de rivage (z en fonction de x), identique à `demo_plage`. La terre est vers +Z.
fn ligne_rivage(x: f32) -> f32 {
    12.0 * ops::sin(x * 0.018)
        + 7.0 * ops::sin(x * 0.047 + 1.3)
        + perlin3(Vec3::new(x * 0.01, 3.7, 0.0), 0) * 14.0
}

/// Hauteur du sol (le niveau de la mer est 0), identique à `demo_plage`.
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

/// Parcours : plage → ras de l'eau → montée dans la couche nuageuse → au-dessus → redescente vers la plage.
fn parcours(duree: f32) -> Option<CameraPath> {
    let r = ligne_rivage(0.0);
    let cle = |t: f32, position: Vec3, look_at: Vec3| CameraKey {
        time: duree * t,
        position,
        look_at,
    };
    CameraPath::new(vec![
        cle(
            0.0,
            Vec3::new(0.0, hauteur_sol(0.0, r + 14.0) + 1.7, r + 14.0),
            Vec3::new(-10.0, 0.0, r - 60.0),
        ),
        cle(
            0.2,
            Vec3::new(-20.0, 3.0, r - 120.0),
            Vec3::new(-60.0, 0.0, r - 500.0),
        ),
        cle(
            0.4,
            Vec3::new(0.0, 600.0, r - 900.0),
            Vec3::new(200.0, 2000.0, r - 3000.0),
        ),
        cle(
            0.55,
            Vec3::new(100.0, 2500.0, r - 2000.0),
            Vec3::new(300.0, 2450.0, r - 3000.0),
        ),
        cle(
            0.75,
            Vec3::new(300.0, 5000.0, r - 2500.0),
            Vec3::new(800.0, 4300.0, r - 5500.0),
        ),
        cle(
            1.0,
            Vec3::new(-20.0, 35.0, r + 40.0),
            Vec3::new(0.0, 0.0, r - 40.0),
        ),
    ])
}

fn scene(
    mut commandes: Commands,
    args: Res<Args>,
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
    commandes.spawn((
        Name::new("Plage"),
        Mesh3d(maillages.add(maillage_terrain(300.0, -400.0, 200.0, 1.0))),
        MeshMaterial3d(materiaux.add(StandardMaterial {
            base_color: Color::srgb(0.78, 0.70, 0.52),
            perceptual_roughness: 0.95,
            ..default()
        })),
    ));
    let Some(chemin) = parcours(args.duree) else {
        error!("parcours caméra invalide");
        return;
    };
    let mut camera = commandes.spawn((
        Camera3d::default(),
        chemin.transform_at(0.0),
        BenchCamera(chemin),
        AuditeurMeteo,
        AtmosphereSettings::default(),
        Exposure { ev100: 13.0 },
        Tonemapping::AcesFitted,
        AtmosphereEnvironmentMapLight::default(),
        // `MCF_MSAA=0` : sans anticrénelage (MSAA ×4 par défaut, dans toutes les variantes).
        if std::env::var("MCF_MSAA").as_deref() == Ok("0") {
            Msaa::Off
        } else {
            Msaa::Sample4
        },
    ));
    if args.variante >= Variante::Nuages {
        // `MCF_CIEL=0` : reflets sans les nuages, pour mesurer leur coût.
        camera.insert(NuagesVolumetriques {
            ciel: std::env::var("MCF_CIEL").as_deref() != Ok("0"),
            ..default()
        });
    }
    // `MCF_ECLABOUSSURES=0` : pluie sans éclaboussures, pour mesurer leur coût (variante complet seulement).
    if args.variante >= Variante::Complet && std::env::var("MCF_ECLABOUSSURES").as_deref() != Ok("0") {
        camera.insert(EclaboussuresPluie);
    }
    demandes.write(ChangerMeteo {
        cible: args.preset.etat(),
        duree_s: 0.0,
    });
}
