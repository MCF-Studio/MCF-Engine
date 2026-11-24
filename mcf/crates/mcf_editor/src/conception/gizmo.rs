//! Gizmo de transformation des bâtiments sélectionnés (spec §6.5), dans le Plan et la 3D.
//!
//! - flèche rouge : déplacement sur X ; flèche bleue : déplacement sur Z ;
//! - carré blanc au centre : déplacement libre ;
//! - anneau vert : rotation autour du centre de la sélection, par crans de 5° (Alt : libre).
//!
//! Le gizmo garde la même taille à l'écran quel que soit le zoom. Le geste n'écrit rien avant le relâchement
//! (`Carte::transformer_batiments`, une seule annulation) ; pendant le geste, contours et modèles montrent l'aperçu.

use bevy::{camera::visibility::RenderLayers, prelude::*};
use mcf_carte::batiments::tourner;
use mcf_geo::P2;

use super::{
    COUCHE_3D, COUCHE_PLAN, CibleGlisse, Disposition, Edition, Glisse, Projet, Selection, camera::Vue,
};

/// Longueur d'une flèche à l'écran (pixels).
const TAILLE_PX: f32 = 90.0;
/// Rayon de l'anneau, en longueurs de flèche.
const ANNEAU: f32 = 1.25;
/// Tolérance de clic (pixels).
const TOLERANCE_PX: f32 = 9.0;
/// Cran de rotation (degrés).
pub const CRAN_DEG: f64 = 5.0;

/// Partie du gizmo saisie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Poignee {
    /// Déplacement sur X.
    AxeX,
    /// Déplacement sur Z.
    AxeZ,
    /// Déplacement libre.
    Libre,
    /// Rotation autour de Y.
    Rotation,
}

/// Repères du gizmo dans le Plan (au-dessus de tout).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct GizmosManipPlan;

/// Repères du gizmo dans la 3D (au-dessus de tout).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct GizmosManip3d;

pub struct GizmoPlugin;

impl Plugin for GizmoPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<GizmosManipPlan>()
            .init_gizmo_group::<GizmosManip3d>()
            .add_systems(Startup, regler)
            .add_systems(Update, dessiner.after(super::outils::SystemesOutils));
    }
}

fn regler(mut store: ResMut<GizmoConfigStore>) {
    let regler = |c: &mut GizmoConfig, couche: usize| {
        c.render_layers = RenderLayers::layer(couche);
        c.depth_bias = -1.0;
        c.line.width = 3.0;
    };
    regler(store.config_mut::<GizmosManipPlan>().0, COUCHE_PLAN);
    regler(store.config_mut::<GizmosManip3d>().0, COUCHE_3D);
}

/// Aperçu de la transformation en cours : `(décalage, degrés, pivot)`. `None` hors d'un geste sur des bâtiments.
pub fn apercu(edition: &Edition, alt: bool) -> Option<(P2, f64, P2)> {
    transformation(&edition.glisse?, alt)
}

/// Transformation `(décalage, degrés, pivot)` décrite par un geste sur des bâtiments (Alt : rotation sans crans).
pub fn transformation(g: &Glisse, alt: bool) -> Option<(P2, f64, P2)> {
    let g = *g;
    let d = [g.actuel[0] - g.depart[0], g.actuel[1] - g.depart[1]];
    match g.cible {
        CibleGlisse::Batiments => Some((d, 0.0, [0.0, 0.0])),
        CibleGlisse::Gizmo { poignee, pivot } => Some(match poignee {
            Poignee::AxeX => ([d[0], 0.0], 0.0, pivot),
            Poignee::AxeZ => ([0.0, d[1]], 0.0, pivot),
            Poignee::Libre => (d, 0.0, pivot),
            Poignee::Rotation => ([0.0, 0.0], angle_glisse(&g, pivot, !alt), pivot),
        }),
        _ => None,
    }
}

/// Angle (degrés) d'un vecteur du plan `(x, z)`, dans le sens de [`tourner`] (de +X vers −Z).
fn angle(v: P2) -> f64 {
    (-v[1]).atan2(v[0]).to_degrees()
}

/// Rotation décrite par un glisser autour de `pivot`, ramenée à ]−180 ; 180], par crans si `cran`.
pub fn angle_glisse(g: &Glisse, pivot: P2, cran: bool) -> f64 {
    let a = [g.depart[0] - pivot[0], g.depart[1] - pivot[1]];
    let b = [g.actuel[0] - pivot[0], g.actuel[1] - pivot[1]];
    if a[0].hypot(a[1]) < 0.25 || b[0].hypot(b[1]) < 0.25 {
        return 0.0;
    }
    let mut deg = angle(b) - angle(a);
    deg = (deg + 180.0).rem_euclid(360.0) - 180.0;
    if cran {
        (deg / CRAN_DEG).round() * CRAN_DEG
    } else {
        deg
    }
}

/// Centre de la sélection (moyenne des centres des bâtiments).
pub fn pivot(projet: &Projet, ids: &[mcf_carte::Id]) -> Option<P2> {
    let centres: Vec<P2> = ids
        .iter()
        .filter_map(|i| projet.carte.batiments.get(i))
        .map(mcf_carte::Batiment::centre)
        .collect();
    if centres.is_empty() {
        return None;
    }
    let n = centres.len() as f64;
    Some([
        centres.iter().map(|c| c[0]).sum::<f64>() / n,
        centres.iter().map(|c| c[1]).sum::<f64>() / n,
    ])
}

/// Point 3D du gizmo et longueur d'une flèche (m) qui fait [`TAILLE_PX`] pixels dans cette vue.
fn echelle(cam: &Camera, gt: &GlobalTransform, centre: Vec3) -> Option<f32> {
    let a = cam.world_to_viewport(gt, centre).ok()?;
    let b = cam.world_to_viewport(gt, centre + Vec3::X).ok()?;
    let c = cam.world_to_viewport(gt, centre + Vec3::Z).ok()?;
    let px_par_m = a.distance(b).max(a.distance(c));
    (px_par_m > 1e-4).then(|| (TAILLE_PX / px_par_m).clamp(0.5, 2_000.0))
}

/// Centre 3D du gizmo : au sol sous le pivot dans la 3D, au-dessus de tout dans le Plan (vue verticale).
fn centre(vue: Vue, projet: &mut Projet, pivot: P2) -> Vec3 {
    let y = match vue {
        Vue::Plan => projet.carte.manifeste.altitude_max_m + 40.0,
        Vue::Perspective => projet.carte.derive().terrain.hauteur(pivot) + 0.5,
    };
    Vec3::new(pivot[0] as f32, y as f32, pivot[1] as f32)
}

fn distance_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

/// Points de l'anneau (plan horizontal).
fn anneau(c: Vec3, r: f32) -> impl Iterator<Item = Vec3> {
    (0..=64).map(move |k| {
        let a = k as f32 / 64.0 * std::f32::consts::TAU;
        let (s, co) = bevy::math::ops::sin_cos(a);
        c + Vec3::new(co * r, 0.0, s * r)
    })
}

/// Poignée du gizmo sous le curseur dans cette vue (`None` : clic ailleurs).
pub fn poignee_sous(
    vue: Vue,
    cam: &Camera,
    gt: &GlobalTransform,
    curseur: Vec2,
    projet: &mut Projet,
    pivot: P2,
) -> Option<Poignee> {
    let c = centre(vue, projet, pivot);
    let l = echelle(cam, gt, c)?;
    let ecran = |w: Vec3| cam.world_to_viewport(gt, w).ok();
    let (sc, sx, sz) = (ecran(c)?, ecran(c + Vec3::X * l)?, ecran(c - Vec3::Z * l)?);
    let dx = distance_segment(curseur, sc.lerp(sx, 0.25), sx);
    let dz = distance_segment(curseur, sc.lerp(sz, 0.25), sz);
    if dx.min(dz) <= TOLERANCE_PX {
        return Some(if dx <= dz { Poignee::AxeX } else { Poignee::AxeZ });
    }
    if curseur.distance(sc) <= TOLERANCE_PX * 1.6 {
        return Some(Poignee::Libre);
    }
    let pts: Vec<Vec2> = anneau(c, l * ANNEAU).filter_map(ecran).collect();
    let sur_anneau = pts
        .windows(2)
        .any(|w| distance_segment(curseur, w[0], w[1]) <= TOLERANCE_PX);
    sur_anneau.then_some(Poignee::Rotation)
}

#[allow(clippy::too_many_arguments)]
fn dessiner(
    mut plan: Gizmos<GizmosManipPlan>,
    mut trois_d: Gizmos<GizmosManip3d>,
    cameras: Query<(&Vue, &Camera, &GlobalTransform)>,
    touches: Res<ButtonInput<KeyCode>>,
    edition: Res<Edition>,
    mut projet: ResMut<Projet>,
) {
    let Selection::Batiments(ids) = &edition.selection else {
        return;
    };
    if edition.promenade {
        return;
    }
    let Some(mut p) = pivot(&projet, ids) else {
        return;
    };
    let alt = touches.pressed(KeyCode::AltLeft) || touches.pressed(KeyCode::AltRight);
    let apercu = apercu(&edition, alt);
    let actif = match edition.glisse {
        Some(Glisse {
            cible: CibleGlisse::Gizmo { poignee, .. },
            ..
        }) => Some(poignee),
        _ => None,
    };
    if let Some((d, deg, pv)) = apercu {
        let q = tourner(p, pv, deg);
        p = [q[0] + d[0], q[1] + d[1]];
    }
    for (vue, cam, gt) in &cameras {
        if !cam.is_active || (*vue == Vue::Perspective && edition.disposition == Disposition::Plan) {
            continue;
        }
        let c = centre(*vue, &mut projet, p);
        let Some(l) = echelle(cam, gt, c) else {
            continue;
        };
        let clair = |x: Poignee, base: Color| {
            if actif == Some(x) {
                Color::srgb(1.0, 0.9, 0.2)
            } else {
                base
            }
        };
        let rouge = clair(Poignee::AxeX, Color::srgb(0.92, 0.22, 0.2));
        let bleu = clair(Poignee::AxeZ, Color::srgb(0.25, 0.5, 0.95));
        let vert = clair(Poignee::Rotation, Color::srgb(0.3, 0.85, 0.35));
        let blanc = clair(Poignee::Libre, Color::WHITE);
        let carre = l * 0.12;
        let cotes = [
            c + Vec3::new(-carre, 0.0, -carre),
            c + Vec3::new(carre, 0.0, -carre),
            c + Vec3::new(carre, 0.0, carre),
            c + Vec3::new(-carre, 0.0, carre),
            c + Vec3::new(-carre, 0.0, -carre),
        ];
        // Pendant une rotation, rayon de départ et rayon courant.
        let rayons = match (edition.glisse, apercu) {
            (Some(g), Some((_, deg, pv))) if actif == Some(Poignee::Rotation) => {
                let a =
                    Vec2::new((g.depart[0] - pv[0]) as f32, (g.depart[1] - pv[1]) as f32).normalize_or_zero();
                let b = tourner([f64::from(a.x), f64::from(a.y)], [0.0, 0.0], deg);
                Some((
                    c + Vec3::new(a.x, 0.0, a.y) * l * ANNEAU,
                    c + Vec3::new(b[0] as f32, 0.0, b[1] as f32) * l * ANNEAU,
                    deg,
                ))
            }
            _ => None,
        };
        macro_rules! tracer {
            ($g:expr) => {{
                let g = &mut $g;
                g.arrow(c + Vec3::X * l * 0.25, c + Vec3::X * l, rouge);
                g.arrow(c - Vec3::Z * l * 0.25, c - Vec3::Z * l, bleu);
                g.linestrip(cotes, blanc);
                g.linestrip(anneau(c, l * ANNEAU), vert);
                if let Some((a, b, _)) = rayons {
                    g.line(c, a, Color::srgba(1.0, 1.0, 1.0, 0.5));
                    g.line(c, b, Color::srgb(1.0, 0.9, 0.2));
                }
            }};
        }
        match vue {
            Vue::Plan => tracer!(plan),
            Vue::Perspective => tracer!(trois_d),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glisse(depart: P2, actuel: P2) -> Glisse {
        Glisse {
            depart,
            actuel,
            cible: CibleGlisse::Gizmo {
                poignee: Poignee::Rotation,
                pivot: [0.0, 0.0],
            },
        }
    }

    #[test]
    fn angle_dans_le_sens_de_tourner() {
        // De +X vers −Z : +90°, comme `tourner` et `Quat::from_rotation_y`.
        let g = glisse([10.0, 0.0], [0.0, -10.0]);
        assert!((angle_glisse(&g, [0.0, 0.0], false) - 90.0).abs() < 1e-9);
        let q = tourner([10.0, 0.0], [0.0, 0.0], 90.0);
        assert!(q[0].abs() < 1e-9 && (q[1] + 10.0).abs() < 1e-9);
        let r = Quat::from_rotation_y(90f32.to_radians()) * Vec3::X;
        assert!(r.x.abs() < 1e-6 && (r.z + 1.0).abs() < 1e-6);
    }

    #[test]
    fn crans_et_bornes() {
        let g = glisse([10.0, 0.0], [10.0, -1.2]);
        let libre = angle_glisse(&g, [0.0, 0.0], false);
        assert!((libre - 6.84).abs() < 0.01, "{libre}");
        assert_eq!(angle_glisse(&g, [0.0, 0.0], true), 5.0);
        // Demi-tour par le bas : reste dans ]−180 ; 180].
        let g = glisse([10.0, 0.1], [-10.0, 0.1]);
        assert!(angle_glisse(&g, [0.0, 0.0], false).abs() <= 180.0);
        // Départ sur le pivot : pas de rotation.
        assert_eq!(
            angle_glisse(&glisse([0.0, 0.0], [5.0, 5.0]), [0.0, 0.0], false),
            0.0
        );
    }
}
