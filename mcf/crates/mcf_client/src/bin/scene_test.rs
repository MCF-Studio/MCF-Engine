//! Scène de test de la tranche minimale (phase 1b du jeu) : personnage animé, berline, lampadaire, souterrain.
//!
//! Les assets viennent du dépôt du jeu (`MCF-Remember-this-life/assets`). Chemin par défaut : voisin du dépôt moteur ;
//! pour un autre emplacement, `MCF_ASSETS=<dossier absolu>`.
//!
//! Commandes : ZQSD / WASD ou flèches = marcher, Maj = courir, Q/E = tourner la caméra, F1 = collisions visibles.

use std::f32::consts::PI;

use bevy::{
    asset::AssetPlugin, light::CascadeShadowConfigBuilder, prelude::*,
    world_serialization::WorldInstanceReady,
};

/// Marche et course de référence (`report.json` du personnage).
const VITESSE_MARCHE: f32 = 1.4;
const VITESSE_COURSE: f32 = 4.5;

const PERSO: &str = "personnages/perso_base/export.glb";
const BERLINE: &str = "vehicules/berline_a/export.glb";
const LAMPADAIRE: &str = "props/lampadaire/export.glb";
const GROTTE: &str = "souterrains/grotte_test/export.glb";

fn main() -> AppExit {
    let racine = std::env::var("MCF_ASSETS")
        .unwrap_or_else(|_| "../../../../MCF-Remember-this-life/assets".to_string());
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: racine,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Remember this life — scène de test".into(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.75, 0.82, 1.0),
            brightness: 600.0,
            ..default()
        })
        .init_resource::<MontreCollisions>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                deplacer_perso,
                suivre_camera,
                choisir_animation,
                masquer_collisions,
                basculer_collisions,
            )
                .chain(),
        )
        .run()
}

#[derive(Component)]
struct Perso;

#[derive(Component)]
struct CameraSuiveuse {
    lacet: f32,
}

/// Animations du personnage (nœuds du graphe).
#[derive(Component)]
struct Animations {
    graphe: Handle<AnimationGraph>,
    idle: AnimationNodeIndex,
    marche: AnimationNodeIndex,
    course: AnimationNodeIndex,
}

#[derive(Component, PartialEq, Clone, Copy)]
enum Allure {
    Arret,
    Marche,
    Course,
}

#[derive(Resource, Default)]
struct MontreCollisions(bool);

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut graphes: ResMut<Assets<AnimationGraph>>,
) {
    let scene =
        |chemin: &str| WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(chemin.to_string())));
    let clip = |i: usize| assets.load(GltfAssetLabel::Animation(i).from_asset(PERSO));

    // Ordre des clips dans le glb : idle, run, walk.
    let mut graphe = AnimationGraph::new();
    let idle = graphe.add_clip(clip(0), 1.0, graphe.root);
    let course = graphe.add_clip(clip(1), 1.0, graphe.root);
    let marche = graphe.add_clip(clip(2), 1.0, graphe.root);
    let graphe = graphes.add(graphe);

    commands
        .spawn((
            Perso,
            Allure::Arret,
            Animations {
                graphe,
                idle,
                marche,
                course,
            },
            scene(PERSO),
            Transform::default(),
        ))
        .observe(lancer_animation);

    commands.spawn((
        scene(BERLINE),
        Transform::from_xyz(-5.0, 0.0, -7.0).with_rotation(Quat::from_rotation_y(0.5)),
    ));
    commands.spawn((scene(LAMPADAIRE), Transform::from_xyz(4.0, 0.0, -4.0)));
    // Le souterrain est une coque ouverte au plan du sol : posé à part, on le regarde d'en haut.
    commands.spawn((scene(GROTTE), Transform::from_xyz(45.0, 0.0, 0.0)));

    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(60.0, 60.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.32, 0.34, 0.33),
            perceptual_roughness: 0.95,
            ..default()
        })),
    ));
    commands.spawn((
        Transform::from_rotation(Quat::from_euler(EulerRot::ZYX, 0.0, 1.0, -PI / 4.0)),
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            first_cascade_far_bound: 15.0,
            maximum_distance: 60.0,
            ..default()
        }
        .build(),
    ));
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 3.0, 6.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
        CameraSuiveuse { lacet: 0.0 },
    ));
    commands.spawn((
        Text::new("ZQSD / flèches : marcher · Maj : courir · Q/E : caméra · F1 : collisions"),
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            ..default()
        },
    ));
}

/// Quand la scène du personnage est instanciée : branche le graphe d'animation sur son `AnimationPlayer`.
fn lancer_animation(
    pret: On<WorldInstanceReady>,
    mut commands: Commands,
    enfants: Query<&Children>,
    anims: Query<&Animations>,
    mut joueurs: Query<&mut AnimationPlayer>,
) {
    let Ok(a) = anims.get(pret.entity) else {
        return;
    };
    for e in enfants.iter_descendants(pret.entity) {
        if let Ok(mut j) = joueurs.get_mut(e) {
            j.play(a.idle).repeat();
            commands.entity(e).insert(AnimationGraphHandle(a.graphe.clone()));
        }
    }
}

fn deplacer_perso(
    clavier: Res<ButtonInput<KeyCode>>,
    temps: Res<Time>,
    camera: Single<&CameraSuiveuse>,
    mut perso: Single<(&mut Transform, &mut Allure), With<Perso>>,
) {
    let mut dir = Vec2::ZERO; // x = droite, y = avant
    for (touches, d) in [
        ([KeyCode::KeyW, KeyCode::ArrowUp], Vec2::Y),
        ([KeyCode::KeyS, KeyCode::ArrowDown], -Vec2::Y),
        ([KeyCode::KeyD, KeyCode::ArrowRight], Vec2::X),
        ([KeyCode::KeyA, KeyCode::ArrowLeft], -Vec2::X),
    ] {
        if clavier.any_pressed(touches) {
            dir += d;
        }
    }
    let (tr, allure) = &mut *perso;
    if dir == Vec2::ZERO {
        **allure = Allure::Arret;
        return;
    }
    let course = clavier.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    **allure = if course { Allure::Course } else { Allure::Marche };
    let v = if course { VITESSE_COURSE } else { VITESSE_MARCHE };
    // Avant de la caméra (−Z tourné du lacet), projeté sur le sol.
    let rot = Quat::from_rotation_y(camera.lacet);
    let monde = rot * Vec3::new(dir.x, 0.0, -dir.y).normalize();
    tr.translation += monde * v * temps.delta_secs();
    // Le modèle regarde vers −Z : on le tourne vers la direction de marche.
    let cible = Quat::from_rotation_y(bevy::math::ops::atan2(-monde.x, -monde.z));
    tr.rotation = tr.rotation.slerp(cible, (temps.delta_secs() * 12.0).min(1.0));
}

fn suivre_camera(
    clavier: Res<ButtonInput<KeyCode>>,
    temps: Res<Time>,
    perso: Single<&Transform, (With<Perso>, Without<CameraSuiveuse>)>,
    mut cam: Single<(&mut Transform, &mut CameraSuiveuse)>,
) {
    let (tr, suiveuse) = &mut *cam;
    if clavier.pressed(KeyCode::KeyQ) {
        suiveuse.lacet += 1.5 * temps.delta_secs();
    }
    if clavier.pressed(KeyCode::KeyE) {
        suiveuse.lacet -= 1.5 * temps.delta_secs();
    }
    let cible = perso.translation + Vec3::Y * 1.2;
    let derriere = Quat::from_rotation_y(suiveuse.lacet) * Vec3::new(0.0, 1.8, 5.0);
    tr.translation = cible + derriere;
    tr.look_at(cible, Vec3::Y);
}

fn choisir_animation(
    perso: Single<(Entity, &Allure, &Animations), (With<Perso>, Changed<Allure>)>,
    enfants: Query<&Children>,
    mut joueurs: Query<&mut AnimationPlayer>,
) {
    let (e, allure, a) = *perso;
    let noeud = match allure {
        Allure::Arret => a.idle,
        Allure::Marche => a.marche,
        Allure::Course => a.course,
    };
    for d in enfants.iter_descendants(e) {
        if let Ok(mut j) = joueurs.get_mut(d) {
            j.stop_all();
            j.play(noeud).repeat();
        }
    }
}

/// Les maillages de collision (`*-col`) ne servent pas au rendu : cachés, sauf F1.
fn masquer_collisions(
    montre: Res<MontreCollisions>,
    mut nouveaux: Query<(&Name, &mut Visibility), Added<Name>>,
) {
    for (n, mut v) in &mut nouveaux {
        if n.as_str().ends_with("-col") {
            *v = if montre.0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
}

fn basculer_collisions(
    clavier: Res<ButtonInput<KeyCode>>,
    mut montre: ResMut<MontreCollisions>,
    mut noeuds: Query<(&Name, &mut Visibility)>,
) {
    if !clavier.just_pressed(KeyCode::F1) {
        return;
    }
    montre.0 = !montre.0;
    for (n, mut v) in &mut noeuds {
        if n.as_str().ends_with("-col") {
            *v = if montre.0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
}
