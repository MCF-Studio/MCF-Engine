//! Scène de calibrage : beaucoup d'objets instanciés sur une grande zone, survolée par un parcours caméra
//! rejouable. Sert à valider l'outil de mesure et à donner un premier ordre de grandeur, **pas** à trancher la
//! migration (c'est le rôle du test A complet, avec de vrais modèles mid poly et des LOD).
//!
//! ```text
//! cargo bench-scene -- --objets 100000 --modeles 20 --materiaux 8 --lampes 200 --taille 2000 --ombres
//! ```

use bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};
use mcf_bench::{BenchCamera, BenchPlugin, BenchSettings, CameraKey, CameraPath};

#[derive(Resource, Debug, Clone)]
struct Args {
    objets: usize,
    modeles: usize,
    materiaux: usize,
    lampes: usize,
    /// Côté de la zone, en mètres.
    taille: f32,
    ombres: bool,
    duree: f32,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut a = Self {
            objets: 100_000,
            modeles: 20,
            materiaux: 8,
            lampes: 200,
            taille: 2000.0,
            ombres: false,
            duree: 40.0,
        };
        let mut it = std::env::args().skip(1);
        while let Some(flag) = it.next() {
            let mut val = |name: &str| it.next().ok_or_else(|| format!("valeur manquante après {name}"));
            match flag.as_str() {
                "--objets" => a.objets = num(&val("--objets")?)?,
                "--modeles" => a.modeles = num::<usize>(&val("--modeles")?)?.max(1),
                "--materiaux" => a.materiaux = num::<usize>(&val("--materiaux")?)?.max(1),
                "--lampes" => a.lampes = num(&val("--lampes")?)?,
                "--taille" => a.taille = num::<f32>(&val("--taille")?)?.clamp(100.0, 10_000.0),
                "--duree" => a.duree = num::<f32>(&val("--duree")?)?.clamp(5.0, 3600.0),
                "--ombres" => a.ombres = true,
                other => return Err(format!("option inconnue : {other}")),
            }
        }
        Ok(a)
    }
}

fn num<T: std::str::FromStr>(s: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("nombre invalide : {s}"))
}

/// Générateur pseudo-aléatoire déterministe (même graine ⇒ même scène, d'une machine à l'autre).
struct Lcg(u64);

impl Lcg {
    fn next_f32(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 40) as f32) / ((1u64 << 24) as f32)
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }
}

fn main() -> AppExit {
    let args = match Args::parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return AppExit::error();
        }
    };
    let settings = BenchSettings::new("calibrage")
        .note("objets", args.objets)
        .note("modeles", args.modeles)
        .note("materiaux", args.materiaux)
        .note("lampes", args.lampes)
        .note("taille_m", args.taille)
        .note("ombres", args.ombres);
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "MCF Bench — calibrage".into(),
                present_mode: PresentMode::AutoNoVsync,
                resolution: WindowResolution::new(1920, 1080).with_scale_factor_override(1.0),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(BenchPlugin(settings))
        .insert_resource(args)
        .add_systems(Startup, setup)
        .run()
}

fn setup(
    mut commands: Commands,
    args: Res<Args>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let half = args.taille / 2.0;
    let mut rng = Lcg(0x4D43_465F_4245_4E43); // « MCF_BENC »

    // Modèles partagés : c'est ce qui permet à Bevy de regrouper les objets (instancing automatique).
    let shapes: Vec<Handle<Mesh>> = (0..args.modeles)
        .map(|i| {
            let s = 1.0 + (i % 5) as f32 * 0.5;
            meshes.add(match i % 5 {
                0 => Mesh::from(Cuboid::new(s, s * 2.0, s)),
                1 => Sphere::new(s * 0.6)
                    .mesh()
                    .ico(3)
                    .unwrap_or_else(|_| Mesh::from(Sphere::new(s))),
                2 => Mesh::from(Cylinder::new(s * 0.4, s * 3.0)),
                3 => Mesh::from(Capsule3d::new(s * 0.4, s)),
                _ => Mesh::from(Torus::new(s * 0.3, s * 0.7)),
            })
        })
        .collect();
    let mats: Vec<Handle<StandardMaterial>> = (0..args.materiaux)
        .map(|i| {
            let h = i as f32 / args.materiaux as f32 * 360.0;
            materials.add(StandardMaterial {
                base_color: Color::hsl(h, 0.45, 0.55),
                perceptual_roughness: 0.7,
                ..default()
            })
        })
        .collect();

    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(args.taille, args.taille))),
        MeshMaterial3d(materials.add(Color::srgb(0.32, 0.42, 0.28))),
    ));

    for i in 0..args.objets {
        let x = rng.range(-half, half);
        let z = rng.range(-half, half);
        let yaw = rng.range(0.0, std::f32::consts::TAU);
        commands.spawn((
            Mesh3d(shapes[i % shapes.len()].clone()),
            MeshMaterial3d(mats[(i / shapes.len()) % mats.len()].clone()),
            Transform::from_xyz(x, 1.0, z).with_rotation(Quat::from_rotation_y(yaw)),
        ));
    }

    // Lampes de rue : sans ombre (règle du projet : l'ombre est une exception budgétée).
    let lamp_mesh = meshes.add(Sphere::new(0.3));
    let lamp_mat = materials.add(StandardMaterial {
        emissive: LinearRgba::rgb(8.0, 6.0, 3.0),
        ..default()
    });
    for _ in 0..args.lampes {
        let p = Vec3::new(rng.range(-half, half), 5.0, rng.range(-half, half));
        commands.spawn((
            PointLight {
                intensity: 200_000.0,
                range: 25.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(p),
            Mesh3d(lamp_mesh.clone()),
            MeshMaterial3d(lamp_mat.clone()),
            NotShadowCaster,
        ));
    }

    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            shadow_maps_enabled: args.ombres,
            ..default()
        },
        Transform::from_xyz(1.0, 2.0, 0.5).looking_at(Vec3::ZERO, Vec3::Y),
        CascadeShadowConfigBuilder {
            maximum_distance: 300.0,
            ..default()
        }
        .build(),
    ));

    // Parcours : survol en altitude, descente, traversée au ras du sol, remontée.
    let d = args.duree;
    let h = half * 0.8;
    let keys = vec![
        CameraKey {
            time: 0.0,
            position: Vec3::new(-h, 400.0, -h),
            look_at: Vec3::ZERO,
        },
        CameraKey {
            time: d * 0.25,
            position: Vec3::new(h, 250.0, -h),
            look_at: Vec3::new(0.0, 0.0, 0.0),
        },
        CameraKey {
            time: d * 0.45,
            position: Vec3::new(h * 0.3, 8.0, 0.0),
            look_at: Vec3::new(0.0, 2.0, 0.0),
        },
        CameraKey {
            time: d * 0.7,
            position: Vec3::new(-h * 0.3, 2.0, h * 0.2),
            look_at: Vec3::new(-h, 2.0, h * 0.2),
        },
        CameraKey {
            time: d,
            position: Vec3::new(-h, 150.0, h),
            look_at: Vec3::ZERO,
        },
    ];
    let Some(path) = CameraPath::new(keys) else {
        error!("parcours caméra invalide");
        return;
    };
    commands.spawn((Camera3d::default(), path.transform_at(0.0), BenchCamera(path)));
}
