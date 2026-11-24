//! Gizmo de transformation des bâtiments sélectionnés (spec §6.5), dans le Plan et la 3D.
//!
//! Couleurs d'axe habituelles : X rouge, Y vert, Z bleu.
//! - flèches : déplacement sur X, Y (monter / descendre, 3D seulement) et Z ;
//! - carré blanc au centre : déplacement libre au sol ;
//! - anneau vert (horizontal) : cap, rotation autour du centre de la sélection ;
//! - anneaux rouge et bleu (3D, un seul bâtiment modélisé) : tangage et roulis autour des axes X et Z du modèle.
//!
//! Crans : 5° pour les rotations, 10 cm en hauteur ; Alt : libre. Le gizmo garde la même taille à l'écran quel que
//! soit le zoom. Le geste n'écrit rien avant le relâchement (`Carte::transformer_batiments`, une seule annulation) ;
//! pendant le geste, contours et modèles montrent l'aperçu.

use bevy::{camera::visibility::RenderLayers, prelude::*};
use mcf_carte::{Transformation, batiments::tourner};
use mcf_geo::P2;

use super::{
    COUCHE_3D, COUCHE_PLAN, CibleGlisse, Disposition, Edition, Glisse, Projet, Selection, camera::Vue,
};

/// Longueur d'une flèche à l'écran (pixels).
const TAILLE_PX: f32 = 90.0;
/// Rayon de l'anneau de cap, en longueurs de flèche.
const ANNEAU: f32 = 1.25;
/// Rayon des anneaux d'inclinaison.
const ANNEAU_INCLINAISON: f32 = 1.0;
/// Tolérance de clic (pixels).
const TOLERANCE_PX: f32 = 9.0;
/// Cran de rotation (degrés).
pub const CRAN_DEG: f64 = 5.0;
/// Cran de hauteur (m).
pub const CRAN_Y_M: f64 = 0.1;

/// Partie du gizmo saisie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Poignee {
    /// Déplacement sur X.
    AxeX,
    /// Déplacement vertical.
    AxeY,
    /// Déplacement sur Z.
    AxeZ,
    /// Déplacement libre au sol.
    Libre,
    /// Rotation autour de Y (cap).
    Cap,
    /// Inclinaison autour de l'axe X du modèle.
    Tangage,
    /// Inclinaison autour de l'axe Z du modèle.
    Roulis,
}

/// Ce que le geste a besoin de savoir de l'écran au moment de la saisie.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Saisie {
    /// Poignée saisie.
    pub poignee: Poignee,
    /// Centre de rotation au sol (m).
    pub pivot: P2,
    /// Flèche Y : déplacement à l'écran (pixels) d'un mètre vers le haut.
    pub px_par_m_y: Vec2,
    /// Anneaux d'inclinaison : centre du gizmo à l'écran (pixels).
    pub centre_px: Vec2,
    /// Anneaux d'inclinaison : +1 si l'axe pointe vers la caméra, −1 sinon (sens de rotation à l'écran).
    pub signe: f32,
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

/// Aperçu de la transformation en cours. `None` hors d'un geste sur des bâtiments.
pub fn apercu(edition: &Edition, alt: bool) -> Option<Transformation> {
    transformation(&edition.glisse?, alt)
}

fn cran(v: f64, pas: f64, libre: bool) -> f64 {
    if libre { v } else { (v / pas).round() * pas }
}

/// Transformation décrite par un geste sur des bâtiments (Alt : sans crans).
pub fn transformation(g: &Glisse, alt: bool) -> Option<Transformation> {
    let d = [g.actuel[0] - g.depart[0], g.actuel[1] - g.depart[1]];
    let s = match g.cible {
        CibleGlisse::Batiments => return Some(Transformation::deplacement(d)),
        CibleGlisse::Gizmo(s) => s,
        _ => return None,
    };
    let base = Transformation {
        pivot: s.pivot,
        ..Transformation::default()
    };
    let ecran = g.ecran[1] - g.ecran[0];
    Some(match s.poignee {
        Poignee::AxeX => Transformation {
            delta: [d[0], 0.0],
            ..base
        },
        Poignee::AxeZ => Transformation {
            delta: [0.0, d[1]],
            ..base
        },
        Poignee::Libre => Transformation { delta: d, ..base },
        Poignee::AxeY => {
            let n2 = s.px_par_m_y.length_squared();
            let dy = if n2 > 1e-6 {
                f64::from(ecran.dot(s.px_par_m_y) / n2)
            } else {
                0.0
            };
            Transformation {
                dy_m: cran(dy, CRAN_Y_M, alt),
                ..base
            }
        }
        Poignee::Cap => Transformation {
            cap_deg: angle_glisse(g, s.pivot, !alt),
            ..base
        },
        Poignee::Tangage | Poignee::Roulis => {
            let deg = cran(
                angle_ecran(g.ecran[0], g.ecran[1], s.centre_px, s.signe),
                CRAN_DEG,
                alt,
            );
            if s.poignee == Poignee::Tangage {
                Transformation {
                    tangage_deg: deg,
                    ..base
                }
            } else {
                Transformation {
                    roulis_deg: deg,
                    ..base
                }
            }
        }
    })
}

/// Angle (degrés) dont tourne le curseur autour de `centre` à l'écran, de `de` vers `vers`, dans le sens
/// trigonométrique d'un écran Y vers le haut (les pixels ont Y vers le bas), multiplié par `signe`.
fn angle_ecran(de: Vec2, vers: Vec2, centre: Vec2, signe: f32) -> f64 {
    let (a, b) = (de - centre, vers - centre);
    if a.length() < 3.0 || b.length() < 3.0 {
        return 0.0;
    }
    let (a, b) = (Vec2::new(a.x, -a.y), Vec2::new(b.x, -b.y));
    f64::from(signe * a.angle_to(b)).to_degrees()
}

/// Angle (degrés) d'un vecteur du plan `(x, z)`, dans le sens de [`tourner`] (de +X vers −Z).
fn angle(v: P2) -> f64 {
    (-v[1]).atan2(v[0]).to_degrees()
}

/// Rotation décrite par un glisser au sol autour de `pivot`, ramenée à ]−180 ; 180], par crans si `cran`.
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

/// Repère du gizmo pour la sélection : position du centre (pivot, décalage vertical moyen) et, pour un seul
/// bâtiment modélisé, sa rotation (axes des anneaux d'inclinaison).
struct Repere {
    pivot: P2,
    dy: f64,
    rotation: Option<Quat>,
}

fn repere(projet: &Projet, ids: &[mcf_carte::Id], t: Option<&Transformation>) -> Option<Repere> {
    let mut bats: Vec<mcf_carte::Batiment> = ids
        .iter()
        .filter_map(|i| projet.carte.batiments.get(i).cloned())
        .collect();
    if let Some(t) = t {
        for b in bats.iter_mut().filter(|b| !b.verrou) {
            b.transformer(t);
        }
    }
    let n = bats.len();
    if n == 0 {
        return None;
    }
    let somme = bats.iter().fold([0.0, 0.0], |s, b| {
        let c = b.centre();
        [s[0] + c[0], s[1] + c[1]]
    });
    let dy = bats.iter().map(|b| b.decalage_y_m).sum::<f64>() / n as f64;
    let rotation = match bats.as_slice() {
        [b] => b.modele.as_ref().map(super::modeles::rotation),
        _ => None,
    };
    Some(Repere {
        pivot: [somme[0] / n as f64, somme[1] / n as f64],
        dy,
        rotation,
    })
}

/// Longueur d'une flèche (m) qui fait [`TAILLE_PX`] pixels dans cette vue autour de `centre`.
fn echelle(cam: &Camera, gt: &GlobalTransform, centre: Vec3) -> Option<f32> {
    let a = cam.world_to_viewport(gt, centre).ok()?;
    let b = cam.world_to_viewport(gt, centre + Vec3::X).ok()?;
    let c = cam.world_to_viewport(gt, centre + Vec3::Z).ok()?;
    let px_par_m = a.distance(b).max(a.distance(c));
    (px_par_m > 1e-4).then(|| (TAILLE_PX / px_par_m).clamp(0.5, 2_000.0))
}

/// Centre 3D du gizmo : au sol (plus le décalage vertical) dans la 3D, au-dessus de tout dans le Plan.
fn centre(vue: Vue, projet: &mut Projet, pivot: P2, dy: f64) -> Vec3 {
    let y = match vue {
        Vue::Plan => projet.carte.manifeste.altitude_max_m + 40.0,
        Vue::Perspective => projet.carte.derive().terrain.hauteur(pivot) + dy + 0.5,
    };
    Vec3::new(pivot[0] as f32, y as f32, pivot[1] as f32)
}

fn distance_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

/// Points d'un anneau de rayon `r` autour de `c`, perpendiculaire à `axe`.
fn anneau(c: Vec3, r: f32, axe: Vec3) -> impl Iterator<Item = Vec3> {
    let rot = Quat::from_rotation_arc(Vec3::Z, axe.normalize_or(Vec3::Y));
    (0..=64).map(move |k| {
        let a = k as f32 / 64.0 * std::f32::consts::TAU;
        let (s, co) = bevy::math::ops::sin_cos(a);
        c + rot * Vec3::new(co * r, s * r, 0.0)
    })
}

/// Géométrie du gizmo dans une vue.
struct Formes {
    c: Vec3,
    l: f32,
    /// Flèche Y et anneaux d'inclinaison montrés (3D seulement).
    vertical: bool,
    /// Axes des anneaux d'inclinaison (axes X et Z du modèle), un seul bâtiment modélisé.
    inclinaison: Option<(Vec3, Vec3)>,
}

fn formes(vue: Vue, cam: &Camera, gt: &GlobalTransform, projet: &mut Projet, r: &Repere) -> Option<Formes> {
    let c = centre(vue, projet, r.pivot, r.dy);
    let l = echelle(cam, gt, c)?;
    let vertical = vue == Vue::Perspective;
    let inclinaison = r
        .rotation
        .filter(|_| vertical)
        .map(|q| (q * Vec3::X, q * Vec3::Z));
    Some(Formes {
        c,
        l,
        vertical,
        inclinaison,
    })
}

/// Poignée du gizmo sous le curseur dans cette vue (`None` : clic ailleurs).
pub fn poignee_sous(
    vue: Vue,
    cam: &Camera,
    gt: &GlobalTransform,
    curseur: Vec2,
    projet: &mut Projet,
    ids: &[mcf_carte::Id],
) -> Option<Saisie> {
    let r = repere(projet, ids, None)?;
    let f = formes(vue, cam, gt, projet, &r)?;
    let (c, l) = (f.c, f.l);
    let ecran = |w: Vec3| cam.world_to_viewport(gt, w).ok();
    let sc = ecran(c)?;
    let saisie = |poignee| Saisie {
        poignee,
        pivot: r.pivot,
        px_par_m_y: ecran(c + Vec3::Y).map_or(Vec2::ZERO, |p| p - sc),
        centre_px: sc,
        signe: 1.0,
    };
    // 1. Flèches.
    let mut fleches = vec![(Poignee::AxeX, Vec3::X), (Poignee::AxeZ, Vec3::NEG_Z)];
    if f.vertical {
        fleches.push((Poignee::AxeY, Vec3::Y));
    }
    let proche = fleches
        .iter()
        .filter_map(|(p, axe)| {
            let (a, b) = (ecran(c + *axe * l * 0.25)?, ecran(c + *axe * l)?);
            Some((*p, distance_segment(curseur, a, b)))
        })
        .filter(|(_, d)| *d <= TOLERANCE_PX)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((p, _)) = proche {
        return Some(saisie(p));
    }
    // 2. Centre.
    if curseur.distance(sc) <= TOLERANCE_PX * 1.6 {
        return Some(saisie(Poignee::Libre));
    }
    // 3. Anneaux : cap, puis inclinaisons.
    let mut anneaux = vec![(Poignee::Cap, l * ANNEAU, Vec3::Y)];
    if let Some((ax, az)) = f.inclinaison {
        anneaux.push((Poignee::Tangage, l * ANNEAU_INCLINAISON, ax));
        anneaux.push((Poignee::Roulis, l * ANNEAU_INCLINAISON, az));
    }
    let oeil = gt.translation();
    for (p, rayon, axe) in anneaux {
        let pts: Vec<Vec2> = anneau(c, rayon, axe).filter_map(ecran).collect();
        if pts
            .windows(2)
            .any(|w| distance_segment(curseur, w[0], w[1]) <= TOLERANCE_PX)
        {
            let mut s = saisie(p);
            s.signe = if axe.dot(oeil - c) >= 0.0 { 1.0 } else { -1.0 };
            return Some(s);
        }
    }
    None
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
    let alt = touches.pressed(KeyCode::AltLeft) || touches.pressed(KeyCode::AltRight);
    let apercu = apercu(&edition, alt);
    let Some(r) = repere(&projet, ids, apercu.as_ref()) else {
        return;
    };
    let actif = match edition.glisse {
        Some(Glisse {
            cible: CibleGlisse::Gizmo(s),
            ..
        }) => Some(s.poignee),
        _ => None,
    };
    for (vue, cam, gt) in &cameras {
        if !cam.is_active || (*vue == Vue::Perspective && edition.disposition == Disposition::Plan) {
            continue;
        }
        let Some(f) = formes(*vue, cam, gt, &mut projet, &r) else {
            continue;
        };
        let (c, l) = (f.c, f.l);
        let clair = |x: Poignee, base: Color| {
            if actif == Some(x) {
                Color::srgb(1.0, 0.9, 0.2)
            } else {
                base
            }
        };
        let (rouge, vert, bleu) = (
            Color::srgb(0.92, 0.22, 0.2),
            Color::srgb(0.3, 0.85, 0.35),
            Color::srgb(0.25, 0.5, 0.95),
        );
        let carre = l * 0.12;
        let cotes = [
            c + Vec3::new(-carre, 0.0, -carre),
            c + Vec3::new(carre, 0.0, -carre),
            c + Vec3::new(carre, 0.0, carre),
            c + Vec3::new(-carre, 0.0, carre),
            c + Vec3::new(-carre, 0.0, -carre),
        ];
        // Pendant une rotation de cap, rayon de départ et rayon courant.
        let rayons = match (edition.glisse, apercu) {
            (Some(g), Some(t)) if actif == Some(Poignee::Cap) => {
                let a = Vec2::new(
                    (g.depart[0] - t.pivot[0]) as f32,
                    (g.depart[1] - t.pivot[1]) as f32,
                )
                .normalize_or_zero();
                let b = tourner([f64::from(a.x), f64::from(a.y)], [0.0, 0.0], t.cap_deg);
                Some((
                    c + Vec3::new(a.x, 0.0, a.y) * l * ANNEAU,
                    c + Vec3::new(b[0] as f32, 0.0, b[1] as f32) * l * ANNEAU,
                ))
            }
            _ => None,
        };
        macro_rules! tracer {
            ($g:expr) => {{
                let g = &mut $g;
                g.arrow(
                    c + Vec3::X * l * 0.25,
                    c + Vec3::X * l,
                    clair(Poignee::AxeX, rouge),
                );
                g.arrow(
                    c - Vec3::Z * l * 0.25,
                    c - Vec3::Z * l,
                    clair(Poignee::AxeZ, bleu),
                );
                if f.vertical {
                    g.arrow(
                        c + Vec3::Y * l * 0.25,
                        c + Vec3::Y * l,
                        clair(Poignee::AxeY, vert),
                    );
                }
                g.linestrip(cotes, clair(Poignee::Libre, Color::WHITE));
                g.linestrip(anneau(c, l * ANNEAU, Vec3::Y), clair(Poignee::Cap, vert));
                if let Some((ax, az)) = f.inclinaison {
                    let r = l * ANNEAU_INCLINAISON;
                    g.linestrip(
                        anneau(c, r, ax),
                        clair(Poignee::Tangage, rouge.with_alpha(0.85)),
                    );
                    g.linestrip(
                        anneau(c, r, az),
                        clair(Poignee::Roulis, bleu.with_alpha(0.85)),
                    );
                }
                if let Some((a, b)) = rayons {
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

    fn saisie(poignee: Poignee) -> Saisie {
        Saisie {
            poignee,
            pivot: [0.0, 0.0],
            px_par_m_y: Vec2::new(0.0, -20.0),
            centre_px: Vec2::new(100.0, 100.0),
            signe: 1.0,
        }
    }

    fn glisse(depart: P2, actuel: P2) -> Glisse {
        Glisse {
            depart,
            actuel,
            ecran: [Vec2::ZERO; 2],
            cible: CibleGlisse::Gizmo(saisie(Poignee::Cap)),
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

    #[test]
    fn fleche_y_suit_le_curseur_a_l_ecran() {
        // 30 px par mètre vers le haut de l'écran : 47 px plus haut = 1,567 m, 1,6 m avec le cran de 10 cm.
        let g = Glisse {
            depart: [0.0, 0.0],
            actuel: [0.0, 0.0],
            ecran: [Vec2::new(300.0, 300.0), Vec2::new(305.0, 253.0)],
            cible: CibleGlisse::Gizmo(Saisie {
                px_par_m_y: Vec2::new(0.0, -30.0),
                ..saisie(Poignee::AxeY)
            }),
        };
        let libre = transformation(&g, true).expect("geste").dy_m;
        assert!((libre - 47.0 / 30.0).abs() < 1e-6, "{libre}");
        let cran = transformation(&g, false).expect("geste").dy_m;
        assert!((cran - 1.6).abs() < 1e-9, "{cran}");
        let t = transformation(&g, false).expect("geste");
        assert_eq!(
            (t.delta, t.cap_deg),
            ([0.0, 0.0], 0.0),
            "la flèche Y ne déplace qu'en hauteur"
        );
    }

    #[test]
    fn anneau_d_inclinaison_sens_main_droite() {
        // Axe vers la caméra : un quart de tour dans le sens trigonométrique de l'écran (Y vers le haut) = +90°.
        let c = Vec2::new(100.0, 100.0);
        let deg = angle_ecran(Vec2::new(150.0, 100.0), Vec2::new(100.0, 50.0), c, 1.0);
        assert!((deg - 90.0).abs() < 1e-4, "{deg}");
        assert!((angle_ecran(Vec2::new(150.0, 100.0), Vec2::new(100.0, 50.0), c, -1.0) + 90.0).abs() < 1e-4);
        // Règle de la main droite : vu depuis la pointe de +X, une rotation de +90° autour de X envoie +Y sur +Z,
        // ce qui paraît trigonométrique quand on regarde vers −X.
        let q = Quat::from_rotation_x(90f32.to_radians()) * Vec3::Y;
        assert!((q - Vec3::Z).length() < 1e-6);
    }
}
