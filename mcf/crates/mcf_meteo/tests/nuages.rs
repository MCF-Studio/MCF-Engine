//! Tests de la partie CPU des nuages volumétriques : dérive bouclée, paramètres envoyés au GPU, soleil, taille de
//! la marche. Le rendu lui-même est vérifié par captures (voir `mcf/docs/meteo.md`).
#![cfg(feature = "rendu")]

use bevy::prelude::*;
use mcf_meteo::nuages::{
    BOUCLE_DECALAGE_M, BOUCLE_TEMPS_S, DeriveNuages, PERIODE_CARTE_M, SoleilNuages, calculer_params,
    decalage_reprojection, taille_marche,
};
use mcf_meteo::{EtatMeteo, PresetMeteo, ReglagesNuages};

fn entier(x: f32) -> bool {
    (x - x.round()).abs() < 1e-4
}

#[test]
fn boucles_invisibles() {
    let r = ReglagesNuages::default();
    // Le décalage boucle sur un multiple de toutes les périodes de motif : aucun saut au bouclage.
    assert!(entier(BOUCLE_DECALAGE_M / PERIODE_CARTE_M));
    assert!(entier(BOUCLE_DECALAGE_M / r.taille_forme));
    assert!(entier(BOUCLE_DECALAGE_M * 1.3 / r.taille_detail));
    // Le temps des volutes aussi (montée de 2 m/s × 1,3 dans le motif de détail).
    assert!(entier(BOUCLE_TEMPS_S * 2.0 * 1.3 / r.taille_detail));
}

#[test]
fn derive_suit_le_vent_et_reste_bornee() {
    let mut etat = PresetMeteo::Tempete.etat();
    etat.vent_angle_deg = 90.0; // vent vers +X
    let mut d = DeriveNuages::default();
    d.avancer(&etat, 1.0, 1.0);
    // Vent d'altitude = 2,5 × le vent au sol.
    assert!(
        (d.decalage().x - etat.vent_vitesse * 2.5).abs() < 1e-3,
        "{}",
        d.decalage()
    );
    assert!(d.decalage().y.abs() < 1e-2);
    // Des heures de jeu : tout reste dans les boucles (précision f32 conservée).
    for _ in 0..20_000 {
        d.avancer(&etat, 1.0, 0.25);
    }
    assert!((0.0..BOUCLE_DECALAGE_M).contains(&d.decalage().x));
    assert!((0.0..BOUCLE_DECALAGE_M).contains(&d.decalage().y));
    assert!((0.0..BOUCLE_TEMPS_S).contains(&d.temps()));
    // Pas de temps invalides ignorés.
    let avant = d;
    d.avancer(&etat, 1.0, f32::NAN);
    d.avancer(&etat, 1.0, -1.0);
    assert_eq!(d, avant);
}

#[test]
fn parametres_bornes() {
    let mut etat = EtatMeteo {
        couverture_nuageuse: 1.7,
        ..PresetMeteo::Nuageux.etat()
    };
    etat.intensite_soleil = 0.5;
    let reglages = ReglagesNuages {
        bas: 2000.0,
        haut: 1000.0, // incohérent : corrigé
        pas: 10_000,
        ..default()
    };
    let soleil = SoleilNuages {
        direction: Vec3::new(0.0, 2.0, 0.0),
        eclairement: 100_000.0,
        couleur: Vec3::ONE,
    };
    let p = calculer_params(&etat, &reglages, &DeriveNuages::default(), Some(soleil));
    assert!(p.haut >= p.bas + 100.0);
    assert_eq!(p.couverture, 1.0);
    assert!(p.pas <= 256);
    assert!((p.soleil_direction.length() - 1.0).abs() < 1e-5);
    // L'intensité du soleil de la météo atténue l'éclairement de la lumière.
    assert!((p.soleil_lux - 50_000.0).abs() < 1e-2);
    // Sans soleil : pas de lumière (et pas de NaN).
    let sans = calculer_params(&etat, &reglages, &DeriveNuages::default(), None);
    assert_eq!(sans.soleil_lux, 0.0);
    assert!(sans.soleil_direction.is_finite());
}

#[test]
fn soleil_depuis_une_lumiere_directionnelle() {
    // Lumière posée au-dessus et qui éclaire vers l'origine : le soleil est en haut.
    let t = GlobalTransform::from(Transform::from_xyz(1.0, 2.0, -0.5).looking_at(Vec3::ZERO, Vec3::Y));
    let l = DirectionalLight {
        illuminance: 1234.0,
        ..default()
    };
    let s = SoleilNuages::depuis_lumiere(&l, &t);
    assert!(
        s.direction
            .abs_diff_eq(Vec3::new(1.0, 2.0, -0.5).normalize(), 1e-5),
        "{}",
        s.direction
    );
    assert_eq!(s.eclairement, 1234.0);
}

#[test]
fn taille_de_la_marche() {
    assert_eq!(taille_marche(UVec2::new(1280, 720), 0.5), UVec2::new(640, 360));
    assert_eq!(taille_marche(UVec2::new(1281, 721), 0.5), UVec2::new(641, 361));
    assert_eq!(taille_marche(UVec2::new(0, 0), 0.5), UVec2::ONE);
    assert_eq!(taille_marche(UVec2::new(100, 100), f32::NAN), UVec2::new(50, 50));
    assert_eq!(taille_marche(UVec2::new(100, 100), 4.0), UVec2::new(100, 100));
}

#[test]
fn reprojection_couvre_les_4_pixels_du_bloc() {
    // 4 images consécutives marchent chacune un pixel différent du bloc 2 × 2, puis le cycle recommence.
    let mut vus: Vec<UVec2> = (0..4).map(decalage_reprojection).collect();
    assert!(vus.iter().all(|d| d.x < 2 && d.y < 2));
    vus.sort_by_key(|d| (d.x, d.y));
    vus.dedup();
    assert_eq!(vus.len(), 4);
    for i in 0..8 {
        assert_eq!(decalage_reprojection(i), decalage_reprojection(i + 4));
    }
}
