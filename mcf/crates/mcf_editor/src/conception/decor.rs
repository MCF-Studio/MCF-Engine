//! Décor posé depuis la bibliothèque (`mcf_carte::decor`) : modèles affichés dans la 3D autour de l'œil de la caméra,
//! marques dans le Plan, repères de sélection et aperçu de la pose.
//!
//! Le modèle d'un objet est retrouvé par son identifiant dans le catalogue **validé** de la bibliothèque (chemin
//! relatif confiné, vérifié une fois sur disque : fichier ordinaire, pas de lien, resté dans le dossier). Un
//! identifiant inconnu ou un modèle absent ne bloque rien : l'objet reste une marque.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use bevy::{
    camera::visibility::{RenderLayers, VisibilityRange},
    prelude::*,
    world_serialization::WorldInstanceReady,
};
use mcf_carte::{Id, ObjetDecor, Revisions};

use super::{
    COUCHE_3D, CibleGlisse, Edition, Outil, Projet, Selection,
    bibliotheque::{self, Catalogue as Bibliotheque},
    camera::{Orbite, VuePlan},
    outils::{GizmosDessus, GizmosPlan, GizmosPlanDessus, REPERES_PLAN_M, Reperes},
};

/// Distance de l'œil au-delà de laquelle les modèles du décor ne sont plus affichés (m) : ce sont de petits objets.
pub const PORTEE_DECOR_M: f32 = 600.0;
/// Case du chargement par zone (m).
const PAS_ZONE_M: f32 = 100.0;
/// Marques du Plan dessinées au plus par image.
const MARQUES_MAX: usize = 6_000;

pub struct DecorPlugin;

impl Plugin for DecorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Instances>()
            .init_resource::<Modeles>()
            .add_systems(
                Update,
                (synchroniser, dessiner).after(super::outils::SystemesOutils),
            );
    }
}

/// Modèles de la bibliothèque : identifiant → chemin d'asset s'il est présent et sûr (vérifié une fois).
#[derive(Resource, Default)]
struct Modeles(HashMap<String, Option<String>>);

impl Modeles {
    fn chemin(&mut self, biblio: &Bibliotheque, id: &str) -> Option<String> {
        if let Some(c) = self.0.get(id) {
            return c.clone();
        }
        let chemin = biblio.biblio.as_ref().and_then(|b| {
            let o = b
                .sections
                .iter()
                .flat_map(|s| &s.familles)
                .flat_map(|f| &f.objets)
                .find(|o| o.id == id)?;
            let racine = bibliotheque::racine_assets().join(bibliotheque::DOSSIER);
            present(&racine, &o.fichier).then(|| format!("{}/{}", bibliotheque::DOSSIER, o.fichier))
        });
        self.0.insert(id.to_owned(), chemin.clone());
        chemin
    }
}

/// Le modèle existe, est un fichier ordinaire (pas un lien) et reste dans le dossier une fois résolu.
fn present(racine: &Path, fichier: &str) -> bool {
    let chemin = racine.join(fichier);
    let ordinaire = std::fs::symlink_metadata(&chemin).is_ok_and(|m| m.is_file());
    match (racine.canonicalize(), chemin.canonicalize()) {
        (Ok(r), Ok(c)) => ordinaire && c.starts_with(r),
        _ => false,
    }
}

/// Modèles instanciés : objet → (entité, modèle).
#[derive(Resource, Default)]
struct Instances(HashMap<Id, (Entity, String)>);

/// Racine d'un modèle de décor.
#[derive(Component)]
struct ModeleDecor;

/// Place d'un objet : posé au sol sous sa position, plus son décalage ; tourné de son cap ; mis à l'échelle.
pub fn placement(o: &ObjetDecor, sol: f64) -> Transform {
    Transform::from_xyz(
        o.position[0] as f32,
        (sol + o.decalage_y_m) as f32,
        o.position[1] as f32,
    )
    .with_rotation(Quat::from_rotation_y((o.cap_deg as f32).to_radians()))
    .with_scale(Vec3::splat(o.echelle as f32))
}

/// Œil de la caméra 3D au sol et sa hauteur.
fn oeil(orbite: &Orbite) -> (Vec3, f32) {
    let (s, c) = bevy::math::ops::sin_cos(orbite.lacet);
    let avant = Vec3::new(-s, 0.0, -c);
    (
        orbite.cible - avant * orbite.distance * bevy::math::ops::cos(orbite.tangage),
        orbite.distance * bevy::math::ops::sin(orbite.tangage),
    )
}

/// Objet tel qu'il est vu pendant un glisser des objets sélectionnés.
fn vu(edition: &Edition, id: Id, o: &ObjetDecor) -> ObjetDecor {
    let mut v = o.clone();
    if let (Some(g), Selection::Objets(ids)) = (&edition.glisse, &edition.selection)
        && g.cible == CibleGlisse::Objets
        && ids.contains(&id)
        && !o.verrou
    {
        v.position = [
            o.position[0] + g.actuel[0] - g.depart[0],
            o.position[1] + g.actuel[1] - g.depart[1],
        ];
    }
    v
}

/// Garde les modèles en accord avec la carte, autour de l'œil de la caméra. Ne travaille que si la carte, le geste
/// ou la case de l'œil ont changé.
#[allow(clippy::too_many_arguments)]
fn synchroniser(
    mut commands: Commands,
    assets: Res<AssetServer>,
    biblio: Res<Bibliotheque>,
    edition: Res<Edition>,
    orbite: Res<Orbite>,
    mut projet: ResMut<Projet>,
    mut modeles: ResMut<Modeles>,
    mut instances: ResMut<Instances>,
    mut transforms: Query<&mut Transform, With<ModeleDecor>>,
    mut vu_avant: Local<Option<(Revisions, Option<super::Glisse>, Option<(i64, i64)>)>>,
) {
    let (oeil, hauteur) = oeil(&orbite);
    let zone = (hauteur < PORTEE_DECOR_M).then(|| {
        (
            (oeil.x / PAS_ZONE_M).floor() as i64,
            (oeil.z / PAS_ZONE_M).floor() as i64,
        )
    });
    let etat = (projet.carte.revisions, edition.glisse, zone);
    if *vu_avant == Some(etat) {
        return;
    }
    *vu_avant = Some(etat);
    let portee = PORTEE_DECOR_M + PAS_ZONE_M * 1.5;
    let proches: Vec<(Id, ObjetDecor)> = if zone.is_some() {
        projet
            .carte
            .decor
            .iter()
            .filter(|(_, o)| {
                Vec2::new(o.position[0] as f32 - oeil.x, o.position[1] as f32 - oeil.z).length() < portee
            })
            .map(|(id, o)| (*id, vu(&edition, *id, o)))
            .collect()
    } else {
        Vec::new()
    };
    let terrain = &projet.carte.derive().terrain;
    let mut gardes = HashSet::new();
    for (id, o) in proches {
        let Some(chemin) = modeles.chemin(&biblio, &o.modele) else {
            continue;
        };
        let t = placement(&o, terrain.hauteur(o.position));
        gardes.insert(id);
        match instances.0.get(&id) {
            Some((e, m)) if *m == o.modele => {
                if let Ok(mut tr) = transforms.get_mut(*e) {
                    *tr = t;
                }
            }
            autre => {
                if let Some((e, _)) = autre {
                    commands.entity(*e).despawn();
                }
                let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(chemin));
                let e = commands
                    .spawn((
                        ModeleDecor,
                        WorldAssetRoot(scene),
                        t,
                        RenderLayers::layer(COUCHE_3D),
                    ))
                    .observe(preparer)
                    .id();
                instances.0.insert(id, (e, o.modele.clone()));
            }
        }
    }
    instances.0.retain(|id, (e, _)| {
        let garder = gardes.contains(id);
        if !garder {
            commands.entity(*e).despawn();
        }
        garder
    });
}

/// Modèle instancié : couche 3D sur toute la hiérarchie, portée de visibilité, maillages de collision `-col` cachés.
fn preparer(
    pret: On<WorldInstanceReady>,
    mut commands: Commands,
    enfants: Query<&Children>,
    noms: Query<&Name>,
) {
    let racine = pret.event_target();
    let portee = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: (PORTEE_DECOR_M - 80.0)..PORTEE_DECOR_M,
        use_aabb: false,
    };
    for e in enfants.iter_descendants(racine) {
        let mut ec = commands.entity(e);
        ec.insert((RenderLayers::layer(COUCHE_3D), portee.clone()));
        if noms.get(e).is_ok_and(|n| n.as_str().contains("-col")) {
            ec.insert(Visibility::Hidden);
        }
    }
}

/// Repère d'un objet : un cercle et un trait vers son avant (+Z du modèle tourné du cap).
fn marque(g: &mut Gizmos<GizmosPlan>, p: Vec3, cap_deg: f64, rayon: f32, c: Color) {
    let iso = Isometry3d::new(p, Quat::from_rotation_arc(Vec3::Z, Vec3::Y));
    g.circle(iso, rayon, c);
    let avant = Quat::from_rotation_y((cap_deg as f32).to_radians()) * Vec3::Z;
    g.line(p, p + avant * rayon * 1.6, c);
}

/// Marques du décor dans le Plan, sélection (Plan et 3D), aperçu de la pose sous le curseur.
#[allow(clippy::too_many_arguments)]
fn dessiner(
    mut plan: Gizmos<GizmosPlan>,
    mut dessus_3d: Gizmos<GizmosDessus>,
    mut dessus_plan: Gizmos<GizmosPlanDessus>,
    edition: Res<Edition>,
    vue_plan: Res<VuePlan>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    mut projet: ResMut<Projet>,
) {
    let carte = &mut projet.carte;
    let y_plan = (carte.manifeste.altitude_max_m + REPERES_PLAN_M) as f32;
    let y_marques = y_plan - 2.0;
    // Marques dans l'étendue du Plan (les plus proches du centre d'abord si elles sont trop nombreuses).
    let (lx, lz) = window.map_or((2000.0, 1200.0), |w| (w.width(), w.height()));
    let demi = Vec2::new(lx, lz) * 0.5 * vue_plan.m_par_px;
    let c = vue_plan.centre;
    let rayon = (vue_plan.m_par_px * 4.0).max(0.5);
    let brun = Color::srgb(0.45, 0.3, 0.15);
    if !carte.decor.is_empty() && vue_plan.m_par_px < 8.0 {
        let mut dans: Vec<(f32, Id)> = carte
            .decor
            .iter()
            .filter_map(|(id, o)| {
                let d = Vec2::new(o.position[0] as f32, o.position[1] as f32) - c;
                (d.x.abs() <= demi.x && d.y.abs() <= demi.y).then(|| (d.length_squared(), *id))
            })
            .collect();
        if dans.len() > MARQUES_MAX {
            dans.select_nth_unstable_by(MARQUES_MAX, |a, b| a.0.total_cmp(&b.0));
            dans.truncate(MARQUES_MAX);
        }
        for (_, id) in dans {
            if let Some(o) = carte.decor.get(&id) {
                let o = vu(&edition, id, o);
                let p = Vec3::new(o.position[0] as f32, y_marques, o.position[1] as f32);
                marque(&mut plan, p, o.cap_deg, rayon, brun);
            }
        }
    }
    let choisis: Vec<ObjetDecor> = match &edition.selection {
        Selection::Objets(ids) => ids
            .iter()
            .filter_map(|id| carte.decor.get(id).map(|o| vu(&edition, *id, o)))
            .collect(),
        _ => Vec::new(),
    };
    let terrain = &carte.derive().terrain;
    let mut dessus = Reperes {
        d3: &mut dessus_3d,
        plan: &mut dessus_plan,
        y_plan,
    };
    let horizontal = |p: Vec3| Isometry3d::new(p, Quat::from_rotation_arc(Vec3::Z, Vec3::Y));
    {
        let jaune = Color::srgb(1.0, 0.82, 0.1);
        let gris = Color::srgb(0.6, 0.6, 0.6);
        for o in &choisis {
            let h = terrain.hauteur(o.position) + o.decalage_y_m;
            let p = Vec3::new(o.position[0] as f32, h as f32 + 0.1, o.position[1] as f32);
            let r = (0.9 * o.echelle as f32).max(0.4);
            let couleur = if o.verrou { gris } else { jaune };
            dessus.circle(horizontal(p), r, couleur);
            let avant = Quat::from_rotation_y((o.cap_deg as f32).to_radians()) * Vec3::Z;
            dessus.arrow(p, p + avant * r * 1.8, couleur);
        }
    }
    if let (Outil::PoserObjet(_), Some((p, h))) = (edition.outil, edition.curseur) {
        let q = Vec3::new(p[0] as f32, h as f32 + 0.1, p[1] as f32);
        dessus.circle(horizontal(q), 0.8, Color::WHITE);
        let avant = Quat::from_rotation_y((edition.cap_pose as f32).to_radians()) * Vec3::Z;
        dessus.arrow(q, q + avant * 1.6, Color::WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_au_sol_tourne_et_mis_a_l_echelle() {
        let mut o = ObjetDecor::nouveau("bench_wood", [10.0, -4.0], 90.0);
        o.echelle = 2.0;
        o.decalage_y_m = 0.5;
        let t = placement(&o, 12.0);
        assert!((t.translation - Vec3::new(10.0, 12.5, -4.0)).length() < 1e-5);
        // Cap 90° : l'avant du modèle (+Z) regarde vers +X (même convention que les bâtiments).
        let avant = t.rotation * Vec3::Z;
        assert!((avant - Vec3::X).length() < 1e-5, "{avant}");
        assert_eq!(t.scale, Vec3::splat(2.0));
    }

    #[test]
    fn modele_hors_du_dossier_refuse() {
        let racine = bibliotheque::racine_assets().join(bibliotheque::DOSSIER);
        assert!(!present(&racine, "../bibliotheque.json"));
        assert!(!present(&racine, "rien/absent.glb"));
    }
}
