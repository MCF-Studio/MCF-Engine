//! Promenade : un personnage du jeu marche sur la carte, pour juger l'échelle (rues, bâtiments, distances) au niveau
//! du sol. P (ou le bouton « 🚶 Se promener ») le pose là où regarde la caméra 3D ; ZQSD / WASD (touches physiques)
//! le font marcher par rapport à la caméra, Maj pour courir ; la caméra le suit (clic droit : tourner autour,
//! molette : distance). P ou Échap pour revenir à l'édition.
//!
//! Modèle : `assets/personnages/perso_homme.glb`, copié du jeu (non versionné : asset du jeu). Le jeu l'affiche à
//! l'échelle 0,153 (1,65 m) : même échelle ici, pour voir la carte comme en jeu.

use std::time::Duration;

use bevy::{camera::visibility::RenderLayers, prelude::*, world_serialization::WorldInstanceReady};
use mcf_geo::P2;

use super::{COUCHE_3D, Disposition, Edition, Projet, ZoneVues, camera::Orbite, outils::GizmosPlan};

/// Chemin du modèle, relatif au dossier `assets` de l'éditeur.
const MODELE: &str = "personnages/perso_homme.glb";
/// Échelle du modèle dans le jeu (`ShopKeeper3D.tscn`) : 10,8 unités → 1,65 m.
const ECHELLE: f32 = 0.153;
/// Vitesses (m/s) : marche et course légère.
const MARCHE: f32 = 1.4;
const COURSE: f32 = 3.6;
/// Clips du modèle : attente, marche, course.
const CLIPS: [&str; 3] = ["q_Idle_Loop", "q_Walk_Loop", "q_Jog_Fwd_Loop"];
/// Hauteur des yeux de la caméra au-dessus des pieds (m).
const REGARD: f32 = 1.4;

pub struct PromenadePlugin;

impl Plugin for PromenadePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Promenade>()
            .add_systems(Update, (basculer, marcher).chain());
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Allure {
    #[default]
    Arret,
    Marche,
    Course,
}

impl Allure {
    fn clip(self) -> usize {
        match self {
            Self::Arret => 0,
            Self::Marche => 1,
            Self::Course => 2,
        }
    }
}

/// État de la promenade.
#[derive(Resource, Default)]
pub struct Promenade {
    /// Modèle en cours de chargement ou chargé.
    gltf: Option<Handle<Gltf>>,
    /// Graphe d'animation et nœuds des trois clips.
    graphe: Option<(Handle<AnimationGraph>, Vec<AnimationNodeIndex>)>,
    /// Personnage posé.
    marcheur: Option<Entity>,
    /// Position au sol (m) et cap (rad, 0 = vers +Z).
    position: P2,
    cap: f32,
    allure: Allure,
    /// Était active à l'image précédente.
    active: bool,
    /// Caméra avant la promenade (distance, tangage), rendue à la sortie.
    camera_avant: Option<(f32, f32, f32)>,
}

/// Racine du personnage.
#[derive(Component)]
struct Marcheur;

fn chemin_modele() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(MODELE)
}

#[allow(clippy::too_many_arguments)]
fn basculer(
    mut commands: Commands,
    touches: Res<ButtonInput<KeyCode>>,
    zone: Res<ZoneVues>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    mut graphes: ResMut<Assets<AnimationGraph>>,
    mut promenade: ResMut<Promenade>,
    mut edition: ResMut<Edition>,
    mut projet: ResMut<Projet>,
    mut orbite: ResMut<Orbite>,
) {
    if !zone.clavier_ui
        && (touches.just_pressed(KeyCode::KeyP)
            || (edition.promenade && touches.just_pressed(KeyCode::Escape)))
    {
        edition.promenade = !edition.promenade;
    }
    // Entrée en promenade.
    if edition.promenade && !promenade.active {
        if !chemin_modele().is_file() {
            edition.promenade = false;
            projet.echec(format!(
                "Promenade : modèle introuvable ({}). Copiez perso_homme.glb du jeu à cet endroit.",
                chemin_modele().display()
            ));
            return;
        }
        promenade.active = true;
        promenade.gltf.get_or_insert_with(|| assets.load(MODELE));
        promenade.position = [f64::from(orbite.cible.x), f64::from(orbite.cible.z)];
        promenade.camera_avant = Some((orbite.distance, orbite.tangage, orbite.elevation));
        orbite.distance = 5.0;
        orbite.tangage = 0.3;
        orbite.elevation = REGARD;
        edition.disposition = Disposition::Perspective;
        projet.info(
            "Promenade : ZQSD pour marcher, Maj pour courir, clic droit pour tourner la caméra, P ou Échap pour sortir.",
        );
    }
    // Sortie.
    if !edition.promenade && promenade.active {
        promenade.active = false;
        if let Some(e) = promenade.marcheur.take() {
            commands.entity(e).despawn();
        }
        if let Some((d, t, e)) = promenade.camera_avant.take() {
            orbite.distance = d;
            orbite.tangage = t;
            orbite.elevation = e;
        }
        projet.info("Promenade terminée.");
        return;
    }
    // Personnage à poser dès que le modèle est chargé.
    if promenade.active
        && promenade.marcheur.is_none()
        && let Some(h) = &promenade.gltf
        && assets.is_loaded_with_dependencies(h)
        && let Some(gltf) = gltfs.get(h)
    {
        let Some(scene) = gltf.default_scene.clone() else {
            projet.echec("Promenade : le modèle n'a pas de scène.");
            edition.promenade = false;
            return;
        };
        if promenade.graphe.is_none() {
            let clips: Option<Vec<_>> = CLIPS
                .iter()
                .map(|n| gltf.named_animations.get(*n).cloned())
                .collect();
            let Some(clips) = clips else {
                projet.echec(format!("Promenade : clips {CLIPS:?} absents du modèle."));
                edition.promenade = false;
                return;
            };
            let (graphe, noeuds) = AnimationGraph::from_clips(clips);
            promenade.graphe = Some((graphes.add(graphe), noeuds));
        }
        let modele = commands
            .spawn((
                WorldAssetRoot(scene),
                Transform::from_scale(Vec3::splat(ECHELLE)),
                RenderLayers::layer(COUCHE_3D),
            ))
            .observe(preparer_animations)
            .id();
        let e = commands
            .spawn((
                Marcheur,
                Transform::default(),
                Visibility::default(),
                RenderLayers::layer(COUCHE_3D),
            ))
            .add_child(modele)
            .id();
        promenade.marcheur = Some(e);
        promenade.allure = Allure::Arret;
    }
}

/// Scène du modèle instanciée : couche 3D sur toute la hiérarchie (sinon la caméra 3D ne la voit pas), graphe
/// d'animation branché sur le lecteur, attente en boucle.
fn preparer_animations(
    pret: On<WorldInstanceReady>,
    mut commands: Commands,
    promenade: Res<Promenade>,
    enfants: Query<&Children>,
    mut lecteurs: Query<&mut AnimationPlayer>,
) {
    let racine = pret.event_target();
    let Some((graphe, noeuds)) = &promenade.graphe else {
        return;
    };
    {
        for e in enfants.iter_descendants(racine) {
            commands.entity(e).insert(RenderLayers::layer(COUCHE_3D));
            if let Ok(mut lecteur) = lecteurs.get_mut(e) {
                let mut transitions = AnimationTransitions::new();
                transitions.play(&mut lecteur, noeuds[0], Duration::ZERO).repeat();
                commands
                    .entity(e)
                    .insert((AnimationGraphHandle(graphe.clone()), transitions));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn marcher(
    touches: Res<ButtonInput<KeyCode>>,
    temps: Res<Time>,
    zone: Res<ZoneVues>,
    mut promenade: ResMut<Promenade>,
    mut projet: ResMut<Projet>,
    mut orbite: ResMut<Orbite>,
    mut racines: Query<&mut Transform, With<Marcheur>>,
    enfants: Query<&Children>,
    mut lecteurs: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
    mut plan: Gizmos<GizmosPlan>,
) {
    if !promenade.active {
        return;
    }
    // Direction voulue, relative à la caméra.
    let (s, c) = bevy::math::ops::sin_cos(orbite.lacet);
    let avant = Vec2::new(-s, -c);
    let droite = Vec2::new(c, -s);
    let mut d = Vec2::ZERO;
    if !zone.clavier_ui {
        for (k, v) in [
            (KeyCode::KeyW, avant),
            (KeyCode::KeyS, -avant),
            (KeyCode::KeyA, -droite),
            (KeyCode::KeyD, droite),
        ] {
            if touches.pressed(k) {
                d += v;
            }
        }
    }
    let court = touches.pressed(KeyCode::ShiftLeft) || touches.pressed(KeyCode::ShiftRight);
    let allure = match (d.length_squared() > 0.0, court) {
        (false, _) => Allure::Arret,
        (true, false) => Allure::Marche,
        (true, true) => Allure::Course,
    };
    if allure != Allure::Arret {
        let d = d.normalize();
        let v = if allure == Allure::Course { COURSE } else { MARCHE };
        let dt = temps.delta_secs().min(0.1);
        let suivant = [
            promenade.position[0] + f64::from(d.x * v * dt),
            promenade.position[1] + f64::from(d.y * v * dt),
        ];
        if projet.carte.manifeste.contient(suivant) {
            promenade.position = suivant;
        }
        // Le modèle regarde vers +Z (glTF) : cap = angle de +Z vers la direction de marche.
        let voulu = bevy::math::ops::atan2(d.x, d.y);
        let ecart = (voulu - promenade.cap + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        promenade.cap += ecart * (dt * 12.0).min(1.0);
    }
    let p = promenade.position;
    let h = projet.carte.derive().terrain.hauteur(p) as f32;
    let pied = Vec3::new(p[0] as f32, h, p[1] as f32);
    // Caméra : la cible suit le personnage (la caméra 3D remet sa hauteur au sol, `elevation` = hauteur du regard).
    orbite.cible = pied;
    orbite.elevation = REGARD;
    plan.circle(
        Isometry3d::new(
            pied.with_y(projet.carte.manifeste.altitude_max_m as f32 + 40.0),
            Quat::from_rotation_arc(Vec3::Z, Vec3::Y),
        ),
        6.0,
        Color::srgb(1.0, 0.3, 0.1),
    );
    let Some(racine) = promenade.marcheur else {
        return;
    };
    if let Ok(mut t) = racines.get_mut(racine) {
        t.translation = pied;
        t.rotation = Quat::from_rotation_y(promenade.cap);
    }
    // Clip selon l'allure (fondu 0,2 s).
    if allure != promenade.allure
        && let Some((_, noeuds)) = &promenade.graphe
    {
        for e in enfants.iter_descendants(racine) {
            if let Ok((mut lecteur, mut transitions)) = lecteurs.get_mut(e) {
                transitions
                    .play(&mut lecteur, noeuds[allure.clip()], Duration::from_millis(200))
                    .repeat();
            }
        }
        promenade.allure = allure;
    }
}
