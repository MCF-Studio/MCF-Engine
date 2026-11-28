//! Tests des vagues de bord : grille du rivage (distance signée exacte, normale), mise à jour, modèle de vagues
//! (levée, déferlement de McCowan, jet de rive, bouclage dans le temps) et hauteur de l'eau pour la flottaison.

use bevy::prelude::*;
use mcf_meteo::houle::PERIODE_HOULE_S;
use mcf_meteo::rivage::{
    EchantillonRivage, GrilleRivage, LOIN_DU_RIVAGE, ParamsBord, amplitude_houle, attenuation_houle,
    hauteur_eau, params_bord, vagues_bord,
};
use mcf_meteo::{CarteRivage, Ocean, ParametresRivage, PresetMeteo, SourceFond};

/// Plage droite : la terre est vers +Z, pente de 5 %.
fn plage(_x: f32, z: f32) -> f32 {
    z * 0.05
}

#[test]
fn distance_signee_plage_droite() {
    let g = GrilleRivage::construire(&plage, Vec2::ZERO, 0.0, 2.0, 64);
    for z in [-40.0, -10.0, -3.0, 3.0, 10.0, 40.0] {
        let e = g.echantillonner(Vec2::new(5.0, z)).expect("dans la grille");
        assert!((e.distance - z).abs() <= 1.01, "z {z} : distance {}", e.distance);
        assert!(e.normale.abs_diff_eq(Vec2::Y, 1e-3), "normale {}", e.normale);
        assert!((e.fond - z * 0.05).abs() < 0.06);
    }
    assert!(g.echantillonner(Vec2::new(500.0, 0.0)).is_none());
}

#[test]
fn distance_signee_ile_ronde() {
    // Île de 30 m de rayon : la distance est celle au cercle (exacte à une case près).
    let ile = |x: f32, z: f32| 30.0 - Vec2::new(x, z).length();
    let g = GrilleRivage::construire(&ile, Vec2::ZERO, 0.0, 1.0, 128);
    for p in [
        Vec2::new(50.0, 0.0),
        Vec2::new(-20.0, 35.0),
        Vec2::new(10.0, 5.0),
        Vec2::new(0.0, -55.0),
    ] {
        let e = g.echantillonner(p).expect("dans la grille");
        let attendu = 30.0 - p.length();
        assert!(
            (e.distance - attendu).abs() < 1.2,
            "{p} : {} au lieu de {attendu}",
            e.distance
        );
        // La normale pointe vers la terre (le centre de l'île).
        assert!(
            e.normale.dot(-p.normalize()) > 0.95,
            "{p} : normale {}",
            e.normale
        );
    }
}

#[test]
fn sans_rivage_dans_la_grille() {
    let large = GrilleRivage::construire(&|_, _| -50.0, Vec2::ZERO, 0.0, 2.0, 16);
    assert_eq!(
        large.echantillonner(Vec2::ZERO).expect("dans la grille").distance,
        -LOIN_DU_RIVAGE
    );
    let terre = GrilleRivage::construire(&|_, _| 10.0, Vec2::ZERO, 0.0, 2.0, 16);
    assert_eq!(
        terre.echantillonner(Vec2::ZERO).expect("dans la grille").distance,
        LOIN_DU_RIVAGE
    );
    // Valeurs non finies remplacées (fond très profond).
    let nan = GrilleRivage::construire(&|_, _| f32::NAN, Vec2::ZERO, 0.0, 2.0, 16);
    assert!(nan.echantillonner(Vec2::ZERO).expect("dans la grille").fond < -100.0);
}

#[test]
fn reconstruction_seulement_si_necessaire() {
    let source = SourceFond::new(plage);
    let p = ParametresRivage {
        taille: 64,
        ..default()
    };
    let mut carte = CarteRivage::default();
    assert!(carte.mettre_a_jour(Some(&source), Vec2::ZERO, 0.0, &p, false));
    let g0 = carte.generation();
    // Petit déplacement : même grille.
    assert!(!carte.mettre_a_jour(Some(&source), Vec2::new(3.0, -4.0), 0.0, &p, false));
    // Changement de niveau, grand déplacement, ou forçage : reconstruction.
    assert!(carte.mettre_a_jour(Some(&source), Vec2::ZERO, 0.5, &p, false));
    assert!(carte.mettre_a_jour(Some(&source), Vec2::new(200.0, 0.0), 0.5, &p, false));
    assert!(carte.mettre_a_jour(Some(&source), Vec2::new(200.0, 0.0), 0.5, &p, true));
    assert_eq!(carte.generation(), g0 + 3);
    // La grille couvre toujours le centre.
    assert!(carte.echantillonner(Vec2::new(200.0, 0.0)).is_some());
    // Plus de source : plus de grille.
    assert!(carte.mettre_a_jour(None, Vec2::ZERO, 0.0, &p, false));
    assert!(carte.grille().is_none());
    assert!(!carte.mettre_a_jour(None, Vec2::ZERO, 0.0, &p, false));
}

#[test]
fn parametres_selon_la_meteo() {
    let p = ParametresRivage::default();
    let calme = params_bord(&PresetMeteo::Clair.etat(), &p);
    let tempete = params_bord(&PresetMeteo::Tempete.etat(), &p);
    assert!(tempete.amplitude > calme.amplitude);
    assert!(tempete.amplitude <= 1.4 && calme.amplitude >= 0.2);
    assert!(tempete.jet >= calme.jet && tempete.jet <= p.jet_de_rive);
    // Pulsation quantifiée : boucle sur la période de la houle.
    let cycles = tempete.omega * PERIODE_HOULE_S / std::f32::consts::TAU;
    assert!((cycles - cycles.round()).abs() < 1e-3);
    let sans = params_bord(
        &PresetMeteo::Tempete.etat(),
        &ParametresRivage {
            vagues: 0.0,
            ..default()
        },
    );
    assert_eq!(sans.amplitude, 0.0);
}

fn echantillon(distance: f32, fond: f32) -> EchantillonRivage {
    EchantillonRivage {
        fond,
        distance,
        normale: Vec2::Y,
    }
}

#[test]
fn vagues_bornees_par_le_deferlement() {
    let b = ParamsBord {
        amplitude: 1.4,
        longueur: 24.0,
        jet: 0.0,
        omega: 1.6,
    };
    for distance in [-120.0, -80.0, -40.0, -10.0, -2.0] {
        let profondeur = -distance * 0.05;
        let mut haut = f32::MIN;
        let mut bas = f32::MAX;
        let mut ecume = 0.0f32;
        for i in 0..200 {
            let t = i as f32 * 0.1;
            let v = vagues_bord(
                Vec2::new(3.0, distance),
                &echantillon(distance, -profondeur),
                profondeur,
                &b,
                t,
            );
            haut = haut.max(v.deplacement.y);
            bas = bas.min(v.deplacement.y);
            ecume = ecume.max(v.ecume);
            // La face du rouleau se dresse mais ne se replie pas : pente finie.
            assert!(v.pente.is_finite() && v.deplacement.is_finite());
        }
        // McCowan : hauteur crête à creux ≤ 0,78 h (+ 6 cm). La crête est pointue et le creux plat : la crête
        // monte plus haut que le creux ne descend.
        assert!(
            haut - bas <= 0.78 * profondeur.max(0.05) + 0.06 + 1e-4,
            "d {distance} : {bas} à {haut}"
        );
        assert!(haut >= -bas - 1e-4, "d {distance} : crête {haut}, creux {bas}");
        // Près du bord, la houle déferle (écume) ; au large non.
        if distance > -40.0 {
            assert!(ecume > 0.3, "d {distance} : écume {ecume}");
        }
    }
    // Loin en mer et loin dans les terres : rien.
    let loin = vagues_bord(Vec2::ZERO, &echantillon(-500.0, -25.0), 25.0, &b, 1.0);
    assert_eq!(loin.deplacement, Vec3::ZERO);
    let terre = vagues_bord(Vec2::ZERO, &echantillon(50.0, 3.0), -3.0, &b, 1.0);
    assert_eq!(terre.deplacement, Vec3::ZERO);
}

#[test]
fn jet_de_rive_et_bouclage() {
    let b = params_bord(&PresetMeteo::Pluie.etat(), &ParametresRivage::default());
    let e = echantillon(-5.0, -0.25);
    let p = Vec2::new(12.0, -5.0);
    let mut min = f32::MAX;
    let mut max = f32::MIN;
    for i in 0..400 {
        let t = i as f32 * 0.05;
        let v = vagues_bord(p, &e, 0.25, &b, t);
        min = min.min(v.deplacement.y);
        max = max.max(v.deplacement.y);
        // Identique une période de houle plus tard (le temps de l'océan boucle sur 200 s).
        let w = vagues_bord(p, &e, 0.25, &b, t + PERIODE_HOULE_S);
        assert!(v.deplacement.abs_diff_eq(w.deplacement, 2e-3), "t {t}");
    }
    // Le bord monte et descend.
    assert!(max - min > 0.1, "{min} {max}");
}

#[test]
fn attenuation_en_eau_peu_profonde() {
    assert!(attenuation_houle(0.0, 0.5) <= 0.12 + 1e-6);
    assert_eq!(attenuation_houle(50.0, 0.5), 1.0);
    // Houle de tempête de 8 m sur 10 m de fond : limitée à 0,35 h.
    let a = attenuation_houle(10.0, 8.0);
    assert!((a * 8.0 - 3.53).abs() < 0.01, "{a}");
}

#[test]
fn hauteur_de_l_eau() {
    let mut ocean = Ocean::default();
    ocean.avancer(&PresetMeteo::Tempete.etat(), 12.3);
    let b = params_bord(&PresetMeteo::Tempete.etat(), &ParametresRivage::default());
    // Sans grille : la houle seule, comme `Ocean::hauteur`.
    let vide = CarteRivage::default();
    let h = hauteur_eau(&ocean, None, &vide, &b, 10.0, -20.0);
    assert!((h - ocean.hauteur(10.0, -20.0)).abs() < 0.05, "{h}");
    // Près de la plage : l'eau ne descend pas plus bas que la houle limitée le permet.
    let mut carte = CarteRivage::default();
    carte.mettre_a_jour(
        Some(&SourceFond::new(plage)),
        Vec2::ZERO,
        0.0,
        &ParametresRivage::default(),
        false,
    );
    let profondeur = 1.0; // z = -20
    let h = hauteur_eau(&ocean, None, &carte, &b, 0.0, -20.0);
    let borne = amplitude_houle(&ocean) * attenuation_houle(profondeur, amplitude_houle(&ocean))
        + 0.39 * profondeur
        + 0.03
        + b.jet
        + 0.1;
    assert!(h.abs() <= borne, "{h} > {borne}");
}

#[test]
fn sable_mouille_selon_le_jet_de_rive() {
    use mcf_meteo::rivage::{MARGE_SABLE_HUMIDE, hauteur_mouillee, params_bord};
    let p = ParametresRivage::default();
    let calme = hauteur_mouillee(&params_bord(&PresetMeteo::Clair.etat(), &p));
    let tempete = hauteur_mouillee(&params_bord(&PresetMeteo::Tempete.etat(), &p));
    // Toujours au moins la marge, et plus haut quand le vent forcit.
    assert!(calme >= MARGE_SABLE_HUMIDE);
    assert!(tempete > calme, "calme {calme} m, tempête {tempete} m");
    // Bornée par le jet de rive réglé : pas de plage entière mouillée.
    assert!(tempete <= p.jet_de_rive + MARGE_SABLE_HUMIDE + 1e-6);
}
