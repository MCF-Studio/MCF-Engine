//! Tests de la carte des abris (pas de pluie sous les toits).
#![cfg(feature = "rendu")]

use bevy::camera::primitives::Aabb;
use bevy::prelude::*;
use mcf_meteo::abris::{GrilleAbris, SANS_ABRI, boite_monde};

#[test]
fn grille_vide_sans_abri() {
    let g = GrilleAbris::nouvelle(16, 1.0);
    assert_eq!(g.hauteur(3.0, 3.0), SANS_ABRI);
}

#[test]
fn toit_rasterise_avec_la_plus_haute_hauteur() {
    let mut g = GrilleAbris::nouvelle(32, 1.0);
    let origine = g.origine_pour(Vec2::ZERO);
    assert_eq!(origine, Vec2::splat(-16.0));
    g.remplir(
        origine,
        [
            (Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 3.0, 2.0)),
            // Un second toit plus haut qui chevauche le premier.
            (Vec3::new(1.0, 4.0, 1.0), Vec3::new(3.0, 5.0, 3.0)),
        ],
    );
    assert_eq!(g.hauteur(0.0, 0.0), 3.0);
    assert_eq!(g.hauteur(1.5, 1.5), 5.0);
    assert_eq!(g.hauteur(10.0, 10.0), SANS_ABRI);
    // Hors de la grille.
    assert_eq!(g.hauteur(100.0, 0.0), SANS_ABRI);
}

#[test]
fn boites_non_finies_ignorees() {
    let mut g = GrilleAbris::nouvelle(8, 1.0);
    g.remplir(Vec2::ZERO, [(Vec3::splat(f32::NAN), Vec3::ONE)]);
    assert!(g.hauteurs.iter().all(|&h| h == SANS_ABRI));
}

#[test]
fn origine_alignee_sur_les_cases() {
    let g = GrilleAbris::nouvelle(128, 0.75);
    let a = g.origine_pour(Vec2::new(10.3, -7.9));
    let b = g.origine_pour(Vec2::new(10.4, -7.85));
    // Un petit déplacement dans la même case ne change pas l'origine (pas de reconstruction inutile).
    assert_eq!(a, b);
    assert!(((a.x / 0.75) - (a.x / 0.75).round()).abs() < 1e-4);
}

#[test]
fn boite_monde_tournee() {
    // Boîte 2 × 1 × 1 tournée de 90° autour de Y : elle devient 1 × 1 × 2.
    let aabb = Aabb::from_min_max(Vec3::new(-1.0, -0.5, -0.5), Vec3::new(1.0, 0.5, 0.5));
    let t = GlobalTransform::from(
        Transform::from_xyz(5.0, 2.0, 0.0).with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
    );
    let (min, max) = boite_monde(&aabb, &t);
    assert!(min.abs_diff_eq(Vec3::new(4.5, 1.5, -1.0), 1e-4), "{min}");
    assert!(max.abs_diff_eq(Vec3::new(5.5, 2.5, 1.0), 1e-4), "{max}");
}

#[test]
fn image_au_bon_format() {
    let g = GrilleAbris::nouvelle(64, 1.0);
    let img = g.image();
    assert_eq!(img.data.as_ref().map(Vec::len), Some(64 * 64 * 4));
}

#[test]
fn materiau_mis_a_jour_seulement_si_changement_visible() {
    use mcf_meteo::SurfacesMeteo;
    use mcf_meteo::materiau_meteo::{UniformesSurface, appliquer};
    let mut u = UniformesSurface::default();
    let abris = Vec4::new(0.0, 0.0, 0.75, 128.0);
    let mut s = SurfacesMeteo {
        mouillage: 0.5,
        ..Default::default()
    };
    assert!(appliquer(&mut u, &s, 0.0, abris));
    s.mouillage += 0.001;
    assert!(
        !appliquer(&mut u, &s, 0.0, abris),
        "écart invisible : pas de renvoi au GPU"
    );
    s.mouillage += 0.01;
    assert!(appliquer(&mut u, &s, 0.0, abris));
    assert_eq!(u.mouillage, s.mouillage);
}
