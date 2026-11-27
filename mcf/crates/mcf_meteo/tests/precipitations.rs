//! Tests de la pluie et de la neige (partie CPU : maillage, uniformes, transport). Le rendu lui-même est vérifié
//! avec l'exemple `demo_meteo` (capture d'écran).
#![cfg(feature = "rendu")]

use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use mcf_meteo::precipitations::{EtatPrecipitations, PERIODE_S, calculer_uniformes, maillage_particules};
use mcf_meteo::{EtatMeteo, PresetMeteo, ReglagesPrecipitations};

#[test]
fn maillage_un_quad_par_particule() {
    let m = maillage_particules(1000);
    let Some(VertexAttributeValues::Float32x3(positions)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else {
        panic!("positions absentes");
    };
    assert_eq!(positions.len(), 4000);
    // L'indice de particule est dans z.
    assert_eq!(positions[4 * 999][2], 999.0);
    let Some(Indices::U32(indices)) = m.indices() else {
        panic!("indices absents");
    };
    assert_eq!(indices.len(), 6000);
    assert!(indices.iter().all(|&i| i < 4000));
}

#[test]
fn quantite_selon_les_precipitations() {
    let r = ReglagesPrecipitations::default();
    let clair = calculer_uniformes(&PresetMeteo::Clair.etat(), Vec3::ZERO, &r, None, Vec3::ZERO, 0.0);
    // `actives` est privé côté Rust : on le vérifie par égalité avec un état sans précipitations.
    let sec = calculer_uniformes(
        &EtatMeteo {
            precipitations: 0.0,
            ..PresetMeteo::Clair.etat()
        },
        Vec3::ZERO,
        &r,
        None,
        Vec3::ZERO,
        0.0,
    );
    assert_eq!(clair, sec);
    let tempete = calculer_uniformes(
        &PresetMeteo::Tempete.etat(),
        Vec3::ZERO,
        &r,
        None,
        Vec3::ZERO,
        0.0,
    );
    assert_ne!(tempete, sec);
}

#[test]
fn transport_boucle_sur_la_boite_et_la_periode() {
    let mut e = EtatPrecipitations::default();
    let boite = Vec3::new(70.0, 30.0, 70.0);
    // Une heure de tempête à 60 images/s.
    for _ in 0..(3600 * 60) {
        e.avancer(Vec3::new(22.0, -3.0, -5.0), boite, 1.0 / 60.0);
    }
    let d = e.decalage_vent();
    assert!(d.x >= 0.0 && d.x < 70.0 && d.z >= 0.0 && d.z < 70.0, "{d}");
    // Le vent vertical ne déplace pas la boîte (la chute est gérée par le shader).
    assert_eq!(d.y, 0.0);
    assert!(e.temps() >= 0.0 && e.temps() < PERIODE_S);
}

#[test]
fn valeurs_non_finies_ignorees() {
    let mut e = EtatPrecipitations::default();
    e.avancer(Vec3::splat(f32::NAN), Vec3::splat(70.0), f32::NAN);
    e.avancer(Vec3::ONE, Vec3::splat(70.0), f32::INFINITY);
    assert_eq!(e.temps(), 0.0);
}

#[test]
fn luminance_suit_l_eclairage_de_la_scene() {
    use mcf_meteo::precipitations::luminance_precipitations;
    let r = ReglagesPrecipitations::default();
    let pluie = PresetMeteo::Pluie.etat();
    // Soleil physique (≈ 130 000 lux) : bien plus lumineux que la valeur fixe, sinon la pluie sort noire avec une
    // exposition EV100 13 (facteur ≈ 1e-4).
    let physique = luminance_precipitations(&pluie, &r, Some(130_000.0));
    assert!(physique * 1e-4 > 0.5, "{physique}");
    // Proportionnelle à l'éclairement, et plus faible quand les nuages cachent le soleil.
    let demi = luminance_precipitations(&pluie, &r, Some(65_000.0));
    assert!((physique / demi - 2.0).abs() < 1e-4);
    assert!(luminance_precipitations(&PresetMeteo::Tempete.etat(), &r, Some(130_000.0)) < physique);
    // Sans lumière directionnelle : la valeur fixe des réglages.
    assert!(
        (luminance_precipitations(&pluie, &r, None) - 2500.0 * (0.35 + 0.65 * pluie.intensite_soleil)).abs()
            < 1e-3
    );
    assert!(luminance_precipitations(&pluie, &r, Some(f32::NAN)).is_finite());
}
