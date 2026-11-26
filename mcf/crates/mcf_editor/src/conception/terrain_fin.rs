//! Terrain en pleine résolution autour de la caméra 3D.
//!
//! Le terrain affiché pour toute la carte est sous-échantillonné (un point tous les 32 m sur 32 km) : trop grossier
//! pour une route de 10 m nivelée dans le relief à 4 m, ou pour le personnage qui marche sur ce relief (sol « aux
//! genoux », bosses qui traversent la chaussée). Ici, des tuiles de [`TUILE`] échantillons en pleine résolution sont
//! fabriquées autour de l'œil de la caméra (jusqu'à [`PORTEE_FIN_M`]) et retirées quand on s'éloigne ; le morceau
//! grossier de même emprise s'efface à la même distance (fondu des portées de visibilité). Le sol vu, le sol des
//! routes et le sol de la promenade sont alors le même terrain final.

use std::collections::HashMap;

use bevy::{
    camera::visibility::{RenderLayers, VisibilityRange},
    prelude::*,
};
use mcf_carte::Revisions;

use super::{COUCHE_3D, Projet, camera::Orbite, textures::Surfaces};

/// Côté d'une tuile, en échantillons de relief (128 × 4 m = 512 m).
pub const TUILE: usize = 128;
/// Distance de la caméra jusqu'à laquelle le terrain est en pleine résolution (m).
pub const PORTEE_FIN_M: f32 = 1500.0;
/// Largeur du fondu entre fin et grossier (m).
const FONDU_M: f32 = 150.0;
/// Tuiles fabriquées par image au plus (le déplacement reste fluide).
const TUILES_PAR_IMAGE: usize = 6;

/// Portée du terrain grossier : visible au-delà du terrain fin.
pub fn portee_grossier() -> VisibilityRange {
    VisibilityRange {
        start_margin: (PORTEE_FIN_M - FONDU_M)..PORTEE_FIN_M,
        end_margin: f32::MAX / 2.0..f32::MAX,
        use_aabb: true,
    }
}

fn portee_fin() -> VisibilityRange {
    VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: (PORTEE_FIN_M - FONDU_M)..PORTEE_FIN_M,
        use_aabb: true,
    }
}

#[derive(Component)]
struct TuileFine;

#[derive(Resource, Default)]
struct Tuiles {
    faites: HashMap<(usize, usize), Entity>,
    terrain_vu: Option<(u64, u64, u64)>,
}

pub struct TerrainFinPlugin;

impl Plugin for TerrainFinPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tuiles>().add_systems(Update, suivre);
    }
}

fn suivre(
    mut commands: Commands,
    mut projet: ResMut<Projet>,
    orbite: Res<Orbite>,
    surfaces: Res<Surfaces>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tuiles: ResMut<Tuiles>,
) {
    // Terrain modifié (pinceau, routes) : toutes les tuiles sont refaites.
    let r: Revisions = projet.carte.revisions;
    let vu = (r.reseau, r.relief, r.manifeste);
    if tuiles.terrain_vu != Some(vu) {
        for (_, e) in tuiles.faites.drain() {
            commands.entity(e).despawn();
        }
        tuiles.terrain_vu = Some(vu);
    }
    let eau = projet.carte.manifeste.niveau_eau_m;
    let t = &projet.carte.derive().terrain;
    if t.nx < 2 || t.nz < 2 {
        return;
    }
    // Œil de la caméra (au sol) et hauteur : vue de très haut, rien de fin à montrer.
    let (s, c) = bevy::math::ops::sin_cos(orbite.lacet);
    let avant = Vec3::new(-s, 0.0, -c);
    let oeil = orbite.cible - avant * orbite.distance * bevy::math::ops::cos(orbite.tangage);
    let hauteur = orbite.distance * bevy::math::ops::sin(orbite.tangage);
    let cote_m = TUILE as f32 * t.resolution as f32;
    let (tx, tz) = ((t.nx - 1).div_ceil(TUILE), (t.nz - 1).div_ceil(TUILE));
    // Distance horizontale de l'œil au rectangle d'une tuile.
    let distance = |i: usize, j: usize| {
        let x0 = t.origine[0] as f32 + i as f32 * cote_m;
        let z0 = t.origine[1] as f32 + j as f32 * cote_m;
        let dx = (x0 - oeil.x).max(oeil.x - (x0 + cote_m)).max(0.0);
        let dz = (z0 - oeil.z).max(oeil.z - (z0 + cote_m)).max(0.0);
        Vec2::new(dx, dz).length()
    };
    // Retirer les tuiles éloignées.
    let loin = PORTEE_FIN_M + 600.0;
    tuiles.faites.retain(|&(i, j), e| {
        let garder = distance(i, j) < loin;
        if !garder {
            commands.entity(*e).despawn();
        }
        garder
    });
    if hauteur > PORTEE_FIN_M {
        return;
    }
    // Fabriquer les tuiles manquantes, les plus proches d'abord.
    let pres = PORTEE_FIN_M + 200.0;
    let rayon = (pres / cote_m).ceil() as i64 + 1;
    let (ci, cj) = (
        ((oeil.x - t.origine[0] as f32) / cote_m).floor() as i64,
        ((oeil.z - t.origine[1] as f32) / cote_m).floor() as i64,
    );
    let mut a_faire: Vec<(f32, usize, usize)> = Vec::new();
    for j in (cj - rayon).max(0)..=(cj + rayon).min(tz as i64 - 1) {
        for i in (ci - rayon).max(0)..=(ci + rayon).min(tx as i64 - 1) {
            let (i, j) = (i as usize, j as usize);
            let d = distance(i, j);
            if d < pres && !tuiles.faites.contains_key(&(i, j)) {
                a_faire.push((d, i, j));
            }
        }
    }
    a_faire.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, i, j) in a_faire.into_iter().take(TUILES_PAR_IMAGE) {
        let (x0, z0) = (i * TUILE, j * TUILE);
        let xs: Vec<usize> = (x0..=(x0 + TUILE).min(t.nx - 1)).collect();
        let zs: Vec<usize> = (z0..=(z0 + TUILE).min(t.nz - 1)).collect();
        let (_, trois_d) = super::scene::maillage_morceau(t, &xs, &zs, eau);
        let (mesh, transform) = super::scene::recentrer(trois_d);
        let e = commands
            .spawn((
                TuileFine,
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(surfaces.herbe.clone()),
                transform,
                RenderLayers::layer(COUCHE_3D),
                portee_fin(),
            ))
            .id();
        tuiles.faites.insert((i, j), e);
        debug!("terrain fin : tuile ({i}, {j}), {} en tout", tuiles.faites.len());
    }
}
