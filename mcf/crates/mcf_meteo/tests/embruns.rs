//! Tests des embruns (simulation sur le CPU, sans rendu).

use bevy::prelude::*;
use mcf_meteo::embruns::{
    Bouffee, MAX_EMBRUNS, attributs_maillage, force_selon_amplitude, luminance_embruns,
};
use mcf_meteo::rivage::params_bord;
use mcf_meteo::{CarteRivage, Embruns, Ocean, ParametresRivage, PresetMeteo, ReglagesEmbruns, SourceFond};

/// Plage en pente douce : la terre vers +Z, le rivage en z = 0.
fn plage() -> CarteRivage {
    let source = SourceFond::new(|_, z| z * 0.05);
    let mut carte = CarteRivage::default();
    carte.mettre_a_jour(Some(&source), Vec2::ZERO, 0.0, &ParametresRivage::default(), true);
    carte
}

fn simuler(preset: PresetMeteo, duree_s: f32) -> Embruns {
    let etat = preset.etat();
    let carte = plage();
    let bord = params_bord(&etat, &ParametresRivage::default());
    let reglages = ReglagesEmbruns::default();
    let mut ocean = Ocean::default();
    let mut embruns = Embruns::default();
    let dt = 1.0 / 60.0;
    for _ in 0..(duree_s / dt) as usize {
        ocean.avancer(&etat, dt);
        embruns.avancer(dt, etat.vent(), ocean.niveau);
        embruns.semer(
            Vec2::new(0.0, 10.0),
            dt,
            &ocean,
            None,
            &carte,
            &bord,
            etat.vent(),
            &reglages,
        );
    }
    embruns
}

#[test]
fn embruns_en_tempete_pas_par_temps_calme() {
    let tempete = simuler(PresetMeteo::Tempete, 8.0);
    assert!(
        tempete.bouffees().len() > 50,
        "{} bouffées en tempête",
        tempete.bouffees().len()
    );
    assert!(tempete.bouffees().len() <= MAX_EMBRUNS);
    // Elles naissent en mer (z < 0 à quelques mètres près : la crête et le vent les poussent), au-dessus de l'eau.
    for b in tempete.bouffees() {
        assert!(
            b.position.is_finite() && b.position.z < 25.0 && b.position.y > -0.5,
            "{b:?}"
        );
        assert!(b.age < b.vie && (0.0..=1.0).contains(&b.force) && b.taille > 0.0);
    }
    let clair = simuler(PresetMeteo::Clair, 8.0);
    assert!(
        clair.bouffees().len() * 5 < tempete.bouffees().len(),
        "{} par temps clair, {} en tempête",
        clair.bouffees().len(),
        tempete.bouffees().len()
    );
}

#[test]
fn force_selon_les_vagues() {
    assert_eq!(force_selon_amplitude(0.0), 0.0);
    assert_eq!(force_selon_amplitude(0.3), 0.0);
    assert_eq!(force_selon_amplitude(1.5), 1.0);
    assert!(force_selon_amplitude(0.6) > 0.0 && force_selon_amplitude(0.6) < force_selon_amplitude(0.9));
}

#[test]
fn les_bouffees_retombent_et_meurent() {
    let mut embruns = simuler(PresetMeteo::Tempete, 6.0);
    assert!(!embruns.bouffees().is_empty());
    // Sans nouvelle naissance, tout a disparu après la plus longue durée de vie (1,8 s).
    for _ in 0..150 {
        embruns.avancer(1.0 / 60.0, Vec3::ZERO, 0.0);
    }
    assert!(embruns.bouffees().is_empty());
    // Entrées invalides : rien ne casse.
    let mut e = simuler(PresetMeteo::Tempete, 2.0);
    e.avancer(f32::NAN, Vec3::splat(f32::NAN), 0.0);
    assert!(e.bouffees().iter().all(|b| b.position.is_finite()));
}

#[test]
fn maillage_de_taille_fixe() {
    let b = Bouffee {
        position: Vec3::new(1.0, 2.0, 3.0),
        vitesse: Vec3::ZERO,
        age: 0.5,
        vie: 1.0,
        taille: 0.4,
        force: 0.8,
        graine: 0.25,
    };
    let (positions, coins, donnees) = attributs_maillage(&[b]);
    assert_eq!(positions.len(), MAX_EMBRUNS * 4);
    assert_eq!(coins.len(), MAX_EMBRUNS * 4);
    assert_eq!(donnees.len(), MAX_EMBRUNS * 4);
    assert_eq!(positions[3], [1.0, 2.0, 3.0]);
    assert_eq!(donnees[0], [0.5, 0.4, 0.25, 0.8]);
    // Quads inutilisés : demi-largeur nulle (dégénérés dans le shader).
    assert_eq!(donnees[4][1], 0.0);
}

#[test]
fn luminance_plus_forte_par_beau_temps() {
    let r = ReglagesEmbruns::default();
    let clair = luminance_embruns(&PresetMeteo::Clair.etat(), &r, Some(100_000.0));
    let tempete = luminance_embruns(&PresetMeteo::Tempete.etat(), &r, Some(100_000.0));
    assert!(clair > tempete && tempete > 0.0);
    assert!(luminance_embruns(&PresetMeteo::Tempete.etat(), &r, Some(f32::NAN)).is_finite());
}
