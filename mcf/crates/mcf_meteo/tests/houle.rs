//! Tests de la houle (Gerstner calé sur JONSWAP) et de la hauteur de l'eau pour la flottaison.

use bevy::prelude::*;
use mcf_meteo::houle::{PERIODE_HOULE_S, deplacement, pic_jonswap, quantifier_pulsation, vagues};
use mcf_meteo::{EtatMeteo, Ocean, PresetMeteo};

#[test]
fn pic_jonswap_plausible() {
    // Mer du vent de tempête (22 m/s, fetch 800 km) : houle de plusieurs centaines de mètres, Hs ≈ 10 m.
    let (l, hs) = pic_jonswap(22.0, 800.0);
    assert!((300.0..500.0).contains(&l), "longueur {l}");
    assert!((8.0..12.0).contains(&hs), "Hs {hs}");
    // Brise de 5 m/s : vagues bien plus petites.
    let (l2, hs2) = pic_jonswap(5.0, 300.0);
    assert!(l2 < l && hs2 < 1.0, "{l2} {hs2}");
}

#[test]
fn pulsations_bouclent_sur_la_periode() {
    for v in vagues(&PresetMeteo::Tempete.etat(), 1.0) {
        let cycles = v.omega * PERIODE_HOULE_S / std::f32::consts::TAU;
        assert!((cycles - cycles.round()).abs() < 1e-3, "{cycles}");
    }
    assert!(quantifier_pulsation(1e-6) > 0.0, "au moins un cycle");
    // La surface est identique à t et t + période.
    let v = vagues(&PresetMeteo::Pluie.etat(), 1.0);
    let p = Vec2::new(12.3, -45.6);
    let a = deplacement(&v, p, 3.0);
    let b = deplacement(&v, p, 3.0 + PERIODE_HOULE_S);
    assert!(a.abs_diff_eq(b, 2e-3), "{a} {b}");
}

#[test]
fn vagues_dans_le_sens_du_vent() {
    let etat = EtatMeteo {
        vent_angle_deg: 90.0,
        ..PresetMeteo::Nuageux.etat()
    };
    let v = vagues(&etat, 1.0);
    // La vague principale va vers +X.
    assert!(v[0].direction.abs_diff_eq(Vec2::X, 1e-4), "{}", v[0].direction);
}

#[test]
fn raideur_bornee_pas_de_boucles() {
    for p in PresetMeteo::TOUS {
        for v in vagues(&p.etat(), 1.0) {
            // Q k A ≤ 0,15 par vague : la surface ne se recoupe pas.
            assert!(v.raideur * v.k * v.amplitude <= 0.15 + 1e-4);
        }
    }
}

#[test]
fn hauteur_retrouve_la_surface() {
    let mut ocean = Ocean::default();
    ocean.niveau = 2.0;
    let etat = PresetMeteo::Tempete.etat();
    ocean.avancer(&etat, 17.0);
    // Un point de la surface déplacée doit avoir la hauteur calculée par `hauteur` à sa verticale.
    for i in 0..20 {
        let repos = Vec2::new(i as f32 * 37.0 - 300.0, i as f32 * -11.0);
        let d = deplacement(ocean.vagues(), repos, ocean.temps());
        let x = repos.x + d.x;
        let z = repos.y + d.z;
        let h = ocean.hauteur(x, z);
        assert!(
            (h - (2.0 + d.y)).abs() < 0.05,
            "écart {} en ({x}, {z})",
            h - (2.0 + d.y)
        );
    }
}

#[test]
fn temps_borne() {
    let mut ocean = Ocean::default();
    let etat = PresetMeteo::Clair.etat();
    for _ in 0..(3600 * 60) {
        ocean.avancer(&etat, 1.0 / 60.0);
    }
    assert!((0.0..PERIODE_HOULE_S).contains(&ocean.temps()));
    ocean.avancer(&etat, f32::NAN);
    assert!(ocean.hauteur(0.0, 0.0).is_finite());
}

#[cfg(feature = "rendu")]
mod fft {
    use bevy::prelude::*;
    use mcf_meteo::ocean_fft::{EtatFft, ReglagesFft, bandes, calculer_params};
    use mcf_meteo::{EtatMeteo, Ocean, PresetMeteo};

    #[test]
    fn bandes_contigues_et_apres_le_pic() {
        let (k_min, k_max) = bandes([250.0, 47.0, 9.0], 0.02, 1.5);
        assert!((k_min.x - 0.03).abs() < 1e-6);
        // La fin d'une cascade est le début de la suivante : pas de trou ni de double comptage.
        assert_eq!(k_max.x, k_min.y);
        assert_eq!(k_max.y, k_min.z);
        assert!(k_max.z > 1e8);
    }

    #[test]
    fn ecume_seulement_par_vent_fort() {
        let o = Ocean::default();
        let r = ReglagesFft::default();
        let calme = calculer_params(
            &EtatMeteo {
                vent_vitesse: 2.0,
                ..default()
            },
            &o,
            &r,
            0.016,
        );
        let tempete = calculer_params(&PresetMeteo::Tempete.etat(), &o, &r, 0.016);
        assert_eq!(calme.seuil_ecume, 0.0);
        assert!(tempete.seuil_ecume > 0.7);
        assert!(calculer_params(&PresetMeteo::Clair.etat(), &o, &r, f32::NAN).dt == 0.0);
    }

    #[test]
    fn spectre_recalcule_seulement_si_le_vent_change() {
        let o = Ocean::default();
        let r = ReglagesFft::default();
        let mut e = EtatFft::default();
        let p = calculer_params(&PresetMeteo::Nuageux.etat(), &o, &r, 0.016);
        assert!(e.avancer(&p), "première image : calcul initial");
        assert!(!e.avancer(&p));
        let leger = calculer_params(
            &EtatMeteo {
                vent_vitesse: 9.1,
                ..PresetMeteo::Nuageux.etat()
            },
            &o,
            &r,
            0.016,
        );
        assert!(!e.avancer(&leger), "écart de 0,1 m/s : inchangé");
        let fort = calculer_params(&PresetMeteo::Tempete.etat(), &o, &r, 0.016);
        assert!(e.avancer(&fort));
        assert_eq!(e.generation_spectre, 2);
    }
}
