//! Tests du modèle des surfaces (mouillé, flaques, neige).

use mcf_meteo::{EtatMeteo, PresetMeteo, SurfacesMeteo};

fn simuler(s: &mut SurfacesMeteo, e: &EtatMeteo, secondes: f32) {
    let pas = 1.0 / 30.0;
    for _ in 0..(secondes / pas) as u32 {
        s.avancer(e, pas);
    }
}

#[test]
fn la_pluie_mouille_puis_remplit_les_flaques() {
    let mut s = SurfacesMeteo::default();
    let pluie = PresetMeteo::Pluie.etat();
    simuler(&mut s, &pluie, 5.0);
    assert!(
        s.mouillage > 0.1 && s.mouillage < 0.9,
        "mouillage {}",
        s.mouillage
    );
    assert_eq!(s.flaques, 0.0, "pas de flaques avant saturation");
    simuler(&mut s, &pluie, 120.0);
    assert_eq!(s.mouillage, 1.0);
    assert!(s.flaques > 0.3, "flaques {}", s.flaques);
}

#[test]
fn le_sol_seche_apres_la_pluie_et_les_flaques_plus_lentement() {
    let mut s = SurfacesMeteo {
        mouillage: 1.0,
        flaques: 0.2,
        ..Default::default()
    };
    let clair = PresetMeteo::Clair.etat();
    simuler(&mut s, &clair, 10.0);
    // La flaque garde ses bords mouillés : mouillage ≥ 3 × flaques.
    assert!(s.mouillage >= (s.flaques * 3.0).min(1.0) - 1e-5);
    simuler(&mut s, &clair, 600.0);
    assert!(s.flaques < 0.2);
    assert!(s.mouillage < 0.5, "mouillage {}", s.mouillage);
}

#[test]
fn pas_de_sechage_sous_la_pluie() {
    let mut s = SurfacesMeteo {
        mouillage: 1.0,
        ..Default::default()
    };
    simuler(&mut s, &PresetMeteo::Tempete.etat(), 60.0);
    assert_eq!(s.mouillage, 1.0);
}

#[test]
fn la_neige_tient_quand_il_gele_et_fond_en_mouillant() {
    let mut s = SurfacesMeteo::default();
    let neige = PresetMeteo::Neige.etat();
    // ~1 couche complète en 2 min de forte neige (précipitations 0,7 : un peu plus long).
    simuler(&mut s, &neige, 200.0);
    assert!(s.neige > 0.9, "neige {}", s.neige);
    assert_eq!(
        s.mouillage, 0.0,
        "la neige ne mouille pas tant qu'elle ne fond pas"
    );

    let redoux = EtatMeteo {
        temperature_c: 12.0,
        precipitations: 0.0,
        ..PresetMeteo::Clair.etat()
    };
    simuler(&mut s, &redoux, 30.0);
    assert!(s.neige < 0.9);
    assert!(s.mouillage > 0.0, "l'eau de fonte mouille le sol");
    simuler(&mut s, &redoux, 600.0);
    assert_eq!(s.neige, 0.0);
}

#[test]
fn valeurs_toujours_bornees() {
    let mut s = SurfacesMeteo::default();
    for p in PresetMeteo::TOUS {
        simuler(&mut s, &p.etat(), 30.0);
        for v in [s.mouillage, s.flaques, s.neige] {
            assert!((0.0..=1.0).contains(&v));
        }
    }
    s.avancer(&PresetMeteo::Pluie.etat(), f32::NAN);
    assert!(s.mouillage.is_finite());
}
