//! Caméras : Plan (orthographique, verticale, nord en haut) et 3D (orbite autour d'une cible). Les deux regardent la
//! même scène ; seules leurs couches de rendu diffèrent (style plan / style 3D).
//!
//! Plan : molette = zoom vers le curseur, clic droit ou molette maintenus = déplacer, flèches.
//! 3D : clic droit maintenu = tourner, clic molette = déplacer, molette = rapprocher, ZQSD / WASD (touches
//! physiques) = avancer la cible, A / E (AZERTY ; Q / E en QWERTY) = monter / descendre.

use bevy::{
    camera::{Exposure, Hdr, Viewport, visibility::RenderLayers},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    light::AtmosphereEnvironmentMapLight,
    math::ops,
    pbr::AtmosphereSettings,
    prelude::*,
    window::PrimaryWindow,
};

use super::{COUCHE_3D, COUCHE_PLAN, Disposition, Edition, Projet, ZoneVues};

/// Les deux vues.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vue {
    /// Plan orthographique.
    Plan,
    /// Perspective.
    Perspective,
}

/// Réglages de la caméra 3D.
#[derive(Resource, Debug, Clone, Copy)]
pub struct Orbite {
    /// Point visé.
    pub cible: Vec3,
    /// Distance à la cible (m).
    pub distance: f32,
    /// Lacet (rad).
    pub lacet: f32,
    /// Tangage (rad, positif = vue plongeante).
    pub tangage: f32,
    /// Hauteur ajoutée au-dessus du sol (A / E), m.
    pub elevation: f32,
}

/// Plan : centre visé et mètres par pixel logique.
#[derive(Resource, Debug, Clone, Copy)]
pub struct VuePlan {
    /// Centre `(x, z)`.
    pub centre: Vec2,
    /// Mètres par pixel.
    pub m_par_px: f32,
}

pub struct CamerasPlugin;

impl Plugin for CamerasPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, creer)
            .add_systems(Update, (disposer, naviguer_plan, naviguer_3d, raccourcis_vues))
            .add_systems(PostUpdate, appliquer.before(TransformSystems::Propagate));
    }
}

/// Fond du Plan quand il est seul à l'écran.
const FOND_PLAN: ClearColorConfig = ClearColorConfig::Custom(Color::srgb(0.93, 0.93, 0.9));

/// Altitude de la caméra Plan : au-dessus de tout relief autorisé.
fn hauteur_plan(projet: &Projet) -> f32 {
    projet.carte.manifeste.altitude_max_m as f32 + 600.0
}

fn creer(mut commands: Commands, projet: Res<Projet>) {
    let (a, b) = projet.carte.manifeste.bornes();
    let centre = Vec2::new(((a[0] + b[0]) * 0.5) as f32, ((a[1] + b[1]) * 0.5) as f32);
    let cote = (b[0] - a[0]).max(b[1] - a[1]) as f32;
    commands.insert_resource(VuePlan {
        centre,
        m_par_px: cote / 700.0,
    });
    commands.insert_resource(Orbite {
        cible: Vec3::new(centre.x, 0.0, centre.y),
        distance: cote * 0.9,
        lacet: 0.6,
        tangage: 0.75,
        elevation: 0.0,
    });
    let haut = hauteur_plan(&projet);
    commands.spawn((
        Vue::Plan,
        Camera3d::default(),
        Projection::Orthographic(OrthographicProjection {
            near: 0.0,
            far: haut + 1_000.0,
            ..OrthographicProjection::default_3d()
        }),
        // La 3D dessine d'abord : un effacement vide toute l'image (pas seulement le viewport), le Plan vient donc
        // en second et n'efface le fond que s'il est seul (voir `disposer`).
        Camera {
            order: 1,
            clear_color: FOND_PLAN,
            ..default()
        },
        // Même format d'image que la 3D (HDR) : les deux vues partagent la même texture, le Plan dessine par-dessus
        // (mini-carte, vue partagée) au lieu d'écrire dans une texture à part. Aplats sans éclairage : l'exposition
        // ne les change pas.
        Hdr,
        // En HDR, le tonemapping est une passe sur **toute** l'image partagée (pas seulement le viewport) : le Plan,
        // qui passe après la 3D, la tonemapperait une seconde fois. Sans tonemapping, ses aplats gardent aussi leurs
        // couleurs exactes.
        Tonemapping::None,
        DebandDither::Disabled,
        RenderLayers::from_layers(&[0, COUCHE_PLAN]),
        // Nord (−Z) en haut de l'écran.
        Transform::from_xyz(centre.x, haut, centre.y)
            .looking_at(Vec3::new(centre.x, 0.0, centre.y), -Vec3::Z),
    ));
    commands.spawn((
        Vue::Perspective,
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            near: 0.5,
            far: 40_000.0,
            ..default()
        }),
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        RenderLayers::from_layers(&[0, COUCHE_3D]),
        // Ciel physique (diffusion de Rayleigh / Mie) : le ciel, la brume lointaine (perspective aérienne) et la
        // lumière ambiante suivent la position du soleil. Unités réelles : soleil en lux, exposition « plein jour ».
        AtmosphereSettings {
            aerial_view_lut_max_distance: (cote * 8.0).clamp(4_000.0, 40_000.0),
            ..default()
        },
        AtmosphereEnvironmentMapLight::default(),
        Exposure::SUNLIGHT,
        Tonemapping::AcesFitted,
        Transform::default(),
    ));
}

/// Rectangle logique de chaque vue selon la disposition.
pub fn rect_de(vue: Vue, zone: Rect, disposition: Disposition) -> Option<Rect> {
    match (disposition, vue) {
        (Disposition::Plan, Vue::Plan) | (Disposition::Perspective, Vue::Perspective) => Some(zone),
        // Mini-carte : le Plan en incrustation, en bas à droite de la 3D.
        (Disposition::Perspective, Vue::Plan) => {
            let cote = (zone.width().min(zone.height()) * 0.3).clamp(150.0, 340.0);
            let fin = zone.max - Vec2::splat(12.0);
            Some(Rect::from_corners(fin - Vec2::splat(cote), fin))
        }
        (Disposition::Partagee, Vue::Plan) => Some(Rect::from_corners(
            zone.min,
            Vec2::new(zone.center().x, zone.max.y),
        )),
        (Disposition::Partagee, Vue::Perspective) => Some(Rect::from_corners(
            Vec2::new(zone.center().x, zone.min.y),
            zone.max,
        )),
        _ => None,
    }
}

/// Vue sous le curseur (hors panneaux).
pub fn vue_sous(curseur: Vec2, zone: &ZoneVues, disposition: Disposition) -> Option<Vue> {
    if zone.souris_sur_ui {
        return None;
    }
    [Vue::Plan, Vue::Perspective]
        .into_iter()
        .find(|v| rect_de(*v, zone.rect, disposition).is_some_and(|r| r.contains(curseur)))
}

fn disposer(
    window: Single<&Window, With<PrimaryWindow>>,
    zone: Res<ZoneVues>,
    edition: Res<Edition>,
    mut cameras: Query<(&Vue, &mut Camera)>,
) {
    let f = window.scale_factor();
    let taille = UVec2::new(window.physical_width(), window.physical_height());
    for (vue, mut camera) in &mut cameras {
        let Some(r) =
            rect_de(*vue, zone.rect, edition.disposition).filter(|r| r.width() >= 8.0 && r.height() >= 8.0)
        else {
            if *vue == Vue::Perspective {
                // La 3D cachée (Plan seul, première image avant la mesure des panneaux) reste active sur 1 pixel :
                // désactivée, Bevy 0.20.1 prépare quand même la sonde d'éclairage du ciel
                // (`AtmosphereEnvironmentMapLight`) sans vue ni tampon de transformations, et panique
                // (`bevy_pbr/src/atmosphere/environment.rs:124`). Le Plan, dessiné ensuite, recouvre ce pixel.
                camera.is_active = true;
                camera.viewport = Some(Viewport {
                    physical_position: UVec2::ZERO,
                    physical_size: UVec2::ONE,
                    ..default()
                });
            } else {
                camera.is_active = false;
            }
            continue;
        };
        let pos = (r.min * f).as_uvec2().min(taille.saturating_sub(UVec2::ONE));
        let dim = (r.size() * f).as_uvec2().max(UVec2::ONE).min(taille - pos);
        camera.is_active = true;
        if *vue == Vue::Plan {
            // Partagée ou mini-carte : la 3D a déjà effacé l'image ; le fond du Plan est un aplat de la scène.
            camera.clear_color = if edition.disposition != Disposition::Plan {
                ClearColorConfig::None
            } else {
                FOND_PLAN
            };
        }
        camera.viewport = Some(Viewport {
            physical_position: pos,
            physical_size: dim,
            ..default()
        });
    }
}

fn molette(scroll: &AccumulatedMouseScroll) -> f32 {
    match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    }
}

fn naviguer_plan(
    window: Single<&Window, With<PrimaryWindow>>,
    boutons: Res<ButtonInput<MouseButton>>,
    touches: Res<ButtonInput<KeyCode>>,
    mouvement: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    temps: Res<Time>,
    zone: Res<ZoneVues>,
    edition: Res<Edition>,
    mut plan: ResMut<VuePlan>,
) {
    // Mini-carte : cadrage fixe sur toute la carte (voir `appliquer`).
    if edition.disposition == Disposition::Perspective {
        return;
    }
    let Some(rect) = rect_de(Vue::Plan, zone.rect, edition.disposition) else {
        return;
    };
    let curseur = window.cursor_position();
    let survole = curseur.is_some_and(|c| vue_sous(c, &zone, edition.disposition) == Some(Vue::Plan));
    if survole && let Some(c) = curseur {
        let m = molette(&scroll);
        if m != 0.0 {
            // Zoom vers le curseur : le point sous la souris reste fixe.
            let avant = plan.centre + (c - rect.center()) * plan.m_par_px;
            plan.m_par_px = (plan.m_par_px * ops::powf(0.85, m)).clamp(0.02, 40.0);
            plan.centre = avant - (c - rect.center()) * plan.m_par_px;
        }
        if boutons.pressed(MouseButton::Right) || boutons.pressed(MouseButton::Middle) {
            let m = plan.m_par_px;
            plan.centre -= mouvement.delta * m;
        }
    }
    if !zone.clavier_ui {
        let mut d = Vec2::ZERO;
        for (k, v) in [
            (KeyCode::ArrowLeft, Vec2::NEG_X),
            (KeyCode::ArrowRight, Vec2::X),
            (KeyCode::ArrowUp, Vec2::NEG_Y),
            (KeyCode::ArrowDown, Vec2::Y),
        ] {
            if touches.pressed(k) {
                d += v;
            }
        }
        let m = plan.m_par_px;
        plan.centre += d * m * 600.0 * temps.delta_secs();
    }
}

fn naviguer_3d(
    window: Single<&Window, With<PrimaryWindow>>,
    boutons: Res<ButtonInput<MouseButton>>,
    touches: Res<ButtonInput<KeyCode>>,
    mouvement: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    temps: Res<Time>,
    zone: Res<ZoneVues>,
    edition: Res<Edition>,
    mut projet: ResMut<Projet>,
    mut orbite: ResMut<Orbite>,
) {
    let curseur = window.cursor_position();
    let survole = curseur.is_some_and(|c| vue_sous(c, &zone, edition.disposition) == Some(Vue::Perspective));
    if survole {
        let m = molette(&scroll);
        if m != 0.0 {
            orbite.distance = (orbite.distance * ops::powf(0.88, m)).clamp(5.0, 20_000.0);
        }
        if boutons.pressed(MouseButton::Right) {
            orbite.lacet -= mouvement.delta.x * 0.005;
            orbite.tangage = (orbite.tangage + mouvement.delta.y * 0.005).clamp(0.05, 1.55);
        }
        if boutons.pressed(MouseButton::Middle) {
            let (s, c) = ops::sin_cos(orbite.lacet);
            let droite = Vec3::new(c, 0.0, -s);
            let avant = Vec3::new(-s, 0.0, -c);
            let k = orbite.distance * 0.0015;
            orbite.cible += (-droite * mouvement.delta.x + avant * mouvement.delta.y) * k;
        }
    }
    let actif_3d = edition.disposition != Disposition::Plan;
    if actif_3d && !zone.clavier_ui {
        let (s, c) = ops::sin_cos(orbite.lacet);
        let droite = Vec3::new(c, 0.0, -s);
        let avant = Vec3::new(-s, 0.0, -c);
        let mut d = Vec3::ZERO;
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
        let vite = if touches.pressed(KeyCode::ShiftLeft) {
            3.0
        } else {
            1.0
        };
        let dist = orbite.distance;
        orbite.cible += d * dist * 0.8 * vite * temps.delta_secs();
        // A / E (touches physiques Q / E en QWERTY) : monter / descendre.
        let mut v = 0.0;
        if touches.pressed(KeyCode::KeyQ) {
            v += 1.0;
        }
        if touches.pressed(KeyCode::KeyE) {
            v -= 1.0;
        }
        orbite.elevation =
            (orbite.elevation + v * dist.max(20.0) * 0.6 * vite * temps.delta_secs()).clamp(-200.0, 3_000.0);
    }
    // La cible suit le sol.
    let p = [f64::from(orbite.cible.x), f64::from(orbite.cible.z)];
    if projet.carte.manifeste.contient(p) {
        let h = projet.carte.derive().terrain.hauteur(p) as f32;
        orbite.cible.y += (h - orbite.cible.y) * (temps.delta_secs() * 6.0).min(1.0);
    }
}

fn raccourcis_vues(touches: Res<ButtonInput<KeyCode>>, zone: Res<ZoneVues>, mut edition: ResMut<Edition>) {
    if zone.clavier_ui || !touches.just_pressed(KeyCode::Tab) {
        return;
    }
    let maj = touches.pressed(KeyCode::ShiftLeft) || touches.pressed(KeyCode::ShiftRight);
    edition.disposition = match (maj, edition.disposition) {
        (true, Disposition::Partagee) => Disposition::Perspective,
        (true, _) => Disposition::Partagee,
        (false, Disposition::Plan) => Disposition::Perspective,
        (false, _) => Disposition::Plan,
    };
}

fn appliquer(
    plan: Res<VuePlan>,
    orbite: Res<Orbite>,
    projet: Res<Projet>,
    zone: Res<ZoneVues>,
    edition: Res<Edition>,
    mut cameras: Query<(&Vue, &mut Transform, &mut Projection)>,
) {
    // Mini-carte : toute la carte dans l'incrustation.
    let mini = match (
        edition.disposition,
        rect_de(Vue::Plan, zone.rect, edition.disposition),
    ) {
        (Disposition::Perspective, Some(r)) => {
            let (a, b) = projet.carte.manifeste.bornes();
            let centre = Vec2::new(((a[0] + b[0]) * 0.5) as f32, ((a[1] + b[1]) * 0.5) as f32);
            let cote = (b[0] - a[0]).max(b[1] - a[1]) as f32;
            Some((centre, cote * 1.04 / r.width().min(r.height()).max(1.0)))
        }
        _ => None,
    };
    for (vue, mut t, mut projection) in &mut cameras {
        match vue {
            Vue::Plan => {
                let (centre, m_par_px) = mini.unwrap_or((plan.centre, plan.m_par_px));
                t.translation = Vec3::new(centre.x, hauteur_plan(&projet), centre.y);
                if let Projection::Orthographic(o) = &mut *projection {
                    o.scale = m_par_px;
                }
            }
            Vue::Perspective => {
                let (s, c) = ops::sin_cos(orbite.lacet);
                let (ts, tc) = ops::sin_cos(orbite.tangage);
                let dir = Vec3::new(s * tc, ts, c * tc);
                let cible = orbite.cible + Vec3::Y * orbite.elevation;
                *t = Transform::from_translation(cible + dir * orbite.distance).looking_at(cible, Vec3::Y);
            }
        }
    }
}
