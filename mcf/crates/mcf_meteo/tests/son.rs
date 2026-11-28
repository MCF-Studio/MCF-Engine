//! Tests de l'ambiance sonore (synthèse sans sortie audio) et des éclairs.

use bevy::prelude::*;
use mcf_meteo::houle::PERIODE_HOULE_S;
use mcf_meteo::orage::{Eclair, VITESSE_SON};
use mcf_meteo::rivage::params_bord;
use mcf_meteo::son::{
    EntreesSon, FREQUENCE, PLAFOND_SORTIE, SEUIL_LIMITEUR, SynthAmbiance, calculer_entrees, limiter,
};
use mcf_meteo::{CarteRivage, Ocean, Orage, ParametresRivage, PresetMeteo, ReglagesSon, SourceFond};

/// Rend `secondes` de son par blocs de 10 ms.
fn rendre(synth: &mut SynthAmbiance, e: &EntreesSon, r: &ReglagesSon, secondes: f32) -> Vec<f32> {
    let images = (secondes * FREQUENCE as f32) as usize;
    let mut sortie = vec![0.0; images * 2];
    let mut e = *e;
    for (i, bloc) in sortie.chunks_mut(960).enumerate() {
        e.temps_ocean = (i as f32 * 0.01) % PERIODE_HOULE_S;
        synth.rendre(&e, r, bloc);
    }
    sortie
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// Énergie des aigus : RMS de la différence entre échantillons successifs d'un canal.
fn rms_aigus(x: &[f32]) -> f32 {
    let g: Vec<f32> = x.iter().step_by(2).copied().collect();
    rms(&g.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>())
}

#[test]
fn sortie_finie_et_bornee() {
    let mut s = SynthAmbiance::default();
    let e = EntreesSon {
        vent_vitesse: 40.0,
        precipitations: 1.0,
        amplitude_bord: 3.0,
        distance_rivage: 0.0,
        ..default()
    };
    s.tonnerre(0.0, 1.0);
    let x = rendre(
        &mut s,
        &e,
        &ReglagesSon {
            general: 10.0,
            ..default()
        },
        2.0,
    );
    assert!(x.iter().all(|v| v.is_finite() && v.abs() < 1.0));
    // Entrées absurdes : pas de NaN.
    let folle = EntreesSon {
        vent_vitesse: f32::NAN,
        distance_rivage: f32::INFINITY,
        pulsation_bord: f32::NAN,
        ..default()
    };
    let y = rendre(&mut s, &folle, &ReglagesSon::default(), 0.5);
    assert!(y.iter().all(|v| v.is_finite()));
}

#[test]
fn muet_et_deterministe() {
    let e = EntreesSon::default();
    let x = rendre(
        &mut SynthAmbiance::default(),
        &e,
        &ReglagesSon {
            muet: true,
            ..default()
        },
        0.2,
    );
    assert!(x.iter().all(|&v| v == 0.0));
    let a = rendre(&mut SynthAmbiance::new(7), &e, &ReglagesSon::default(), 0.5);
    let b = rendre(&mut SynthAmbiance::new(7), &e, &ReglagesSon::default(), 0.5);
    assert_eq!(a, b);
}

#[test]
fn la_pluie_ajoute_des_aigus() {
    let r = ReglagesSon::default();
    let sec = EntreesSon {
        distance_rivage: 2000.0,
        ..default()
    };
    let pluie = EntreesSon {
        precipitations: 1.0,
        ..sec
    };
    let abri = EntreesSon {
        abrite: true,
        ..pluie
    };
    // On saute la montée des niveaux (lissage) avant de mesurer.
    let mesurer =
        |e: &EntreesSon| rms_aigus(&rendre(&mut SynthAmbiance::default(), e, &r, 3.0)[96_000 * 2..]);
    let (a_sec, a_pluie, a_abri) = (mesurer(&sec), mesurer(&pluie), mesurer(&abri));
    assert!(a_pluie > a_sec * 3.0, "sec {a_sec}, pluie {a_pluie}");
    assert!(a_abri < a_pluie, "abri {a_abri}, pluie {a_pluie}");
}

#[test]
fn le_vent_fort_est_plus_fort() {
    let r = ReglagesSon::default();
    let calme = EntreesSon {
        vent_vitesse: 2.0,
        distance_rivage: 2000.0,
        amplitude_bord: 0.0,
        ..default()
    };
    let tempete = EntreesSon {
        vent_vitesse: 25.0,
        ..calme
    };
    let a = rms(&rendre(&mut SynthAmbiance::default(), &calme, &r, 3.0)[96_000 * 2..]);
    let b = rms(&rendre(&mut SynthAmbiance::default(), &tempete, &r, 3.0)[96_000 * 2..]);
    assert!(b > a * 2.0, "calme {a}, tempête {b}");
}

#[test]
fn ressac_cale_sur_les_vagues() {
    // Seul le ressac : énergie maximale quand la crête arrive au rivage (phase 0), minimale à mi-période.
    let r = ReglagesSon {
        mer: 0.0,
        vent: 0.0,
        pluie: 0.0,
        tonnerre: 0.0,
        ..default()
    };
    let omega = std::f32::consts::TAU / 8.0; // une vague toutes les 8 s
    let e = EntreesSon {
        amplitude_bord: 1.0,
        pulsation_bord: omega,
        distance_rivage: 5.0,
        ..default()
    };
    let x = rendre(&mut SynthAmbiance::default(), &e, &r, 24.0);
    let fenetre = |t: f32| {
        let i = (t * FREQUENCE as f32) as usize * 2;
        rms(&x[i..i + 9600])
    };
    // Crêtes à t = 8, 16 s ; creux à 12, 20 s (après les 8 premières secondes de montée).
    let crete = fenetre(16.0).min(fenetre(8.0));
    let creux = fenetre(12.0).max(fenetre(20.0));
    assert!(crete > creux * 2.0, "crête {crete}, creux {creux}");
}

#[test]
fn tonnerre_apres_le_delai() {
    let r = ReglagesSon {
        mer: 0.0,
        vent: 0.0,
        pluie: 0.0,
        ressac: 0.0,
        ..default()
    };
    let e = EntreesSon {
        distance_rivage: 2000.0,
        ..default()
    };
    let mut s = SynthAmbiance::default();
    let e1 = Eclair::new(0.5);
    s.tonnerre(e1.delai_tonnerre_s, e1.proximite);
    assert_eq!(s.tonnerres_actifs(), 1);
    let d = e1.delai_tonnerre_s;
    let x = rendre(&mut s, &e, &r, d + 3.0);
    let avant = rms(&x[((d - 1.0) * FREQUENCE as f32) as usize * 2..(d * FREQUENCE as f32) as usize * 2 - 2]);
    let apres =
        rms(&x[((d + 0.5) * FREQUENCE as f32) as usize * 2..((d + 2.5) * FREQUENCE as f32) as usize * 2]);
    assert!(apres > avant * 10.0 + 1e-4, "avant {avant}, après {apres}");
    // Le roulement s'éteint après 12 s.
    rendre(&mut s, &e, &r, 13.0);
    assert_eq!(s.tonnerres_actifs(), 0);
}

#[test]
fn eclairs_seulement_pendant_l_orage() {
    let mut o = Orage::default();
    let clair = PresetMeteo::Clair.etat();
    for _ in 0..1000 {
        assert!(o.avancer(&clair, 0.1).is_none());
    }
    let mut orage = PresetMeteo::Tempete.etat();
    orage.precipitations = 1.0;
    orage.type_nuages = 0.9;
    orage.couverture_nuageuse = 0.95;
    orage.temperature_c = 15.0;
    assert!(Orage::orageux(&orage));
    let mut instants = Vec::new();
    for i in 0..6000 {
        if let Some(e) = o.avancer(&orage, 0.1) {
            assert!((250.0..=3500.0).contains(&e.distance_m));
            assert!((e.delai_tonnerre_s - e.distance_m / VITESSE_SON).abs() < 1e-3);
            instants.push(i as f32 * 0.1);
            assert!(o.eclat() > 0.9);
        }
    }
    // 10 minutes d'orage : un éclair toutes les 7 à 29 s.
    assert!(instants.len() >= 20 && instants.len() <= 90, "{}", instants.len());
    for w in instants.windows(2) {
        assert!(w[1] - w[0] >= 6.9 && w[1] - w[0] <= 29.2, "{w:?}");
    }
    // Le flash s'éteint.
    for _ in 0..10 {
        o.avancer(&clair, 0.1);
    }
    assert_eq!(o.eclat(), 0.0);
    // Désactivé : jamais d'éclair.
    o.eclairs = false;
    assert!((0..1000).all(|_| o.avancer(&orage, 0.1).is_none()));
}

#[test]
fn entrees_depuis_le_monde() {
    let etat = PresetMeteo::Pluie.etat();
    let ocean = Ocean::default();
    let mut carte = CarteRivage::default();
    // Terre vers +Z (pente de 5 %).
    carte.mettre_a_jour(
        Some(&SourceFond::new(|_, z| z * 0.05)),
        Vec2::ZERO,
        0.0,
        &ParametresRivage::default(),
        false,
    );
    let b = params_bord(&etat, &ParametresRivage::default());
    // Sur la plage à 20 m du bord, regard vers la mer (−Z) : la droite est −X ; le rivage est devant, pan ≈ 0.
    let e = calculer_entrees(
        &etat,
        &ocean,
        &carte,
        &b,
        Vec3::new(0.0, 2.0, 20.0),
        Vec3::NEG_X,
        false,
        false,
    );
    assert!((e.distance_rivage - 20.0).abs() < 1.5, "{}", e.distance_rivage);
    assert!(e.pan_rivage.abs() < 0.1);
    // Regard vers +X (droite = +Z) : le rivage (vers −Z) est à gauche.
    let e2 = calculer_entrees(
        &etat,
        &ocean,
        &carte,
        &b,
        Vec3::new(0.0, 2.0, 20.0),
        Vec3::Z,
        false,
        false,
    );
    assert!(e2.pan_rivage < -0.9, "{}", e2.pan_rivage);
    assert_eq!(e.precipitations, etat.precipitations);
    // Sans grille : loin du rivage.
    let loin = calculer_entrees(
        &etat,
        &ocean,
        &CarteRivage::default(),
        &b,
        Vec3::Y,
        Vec3::X,
        true,
        false,
    );
    assert!(loin.distance_rivage > 5000.0 && loin.abrite);
}

/// Sur une plage, le son reste à un niveau d'ambiance et le limiteur ne travaille presque pas, même en tempête
/// (avant correction : niveau efficace de 0,70 avant limiteur et 15 % des échantillons au-dessus de 1).
#[test]
fn niveau_d_ambiance_sans_saturation() {
    let mut precedent = 0.0;
    for (vent, precip) in [(5.0f32, 0.0), (9.0, 0.0), (11.0, 0.6), (22.0, 1.0)] {
        let e = EntreesSon {
            vent_vitesse: vent,
            precipitations: precip,
            amplitude_bord: (0.25 + vent * 0.035).clamp(0.2, 1.4),
            distance_rivage: 14.0,
            ..default()
        };
        let y = rendre(&mut SynthAmbiance::default(), &e, &ReglagesSon::default(), 20.0);
        let y = &y[96_000 * 2..];
        let niveau = rms(y);
        let limites = y.iter().filter(|v| v.abs() > SEUIL_LIMITEUR).count() as f32 / y.len() as f32;
        assert!((0.04..0.2).contains(&niveau), "vent {vent} : niveau {niveau}");
        assert!(limites < 0.05, "vent {vent} : {limites} des échantillons limités");
        // Plus de vent et de pluie : plus fort.
        assert!(niveau > precedent);
        precedent = niveau;
    }
}

#[test]
fn limiteur_transparent_puis_doux() {
    // Sous le seuil : inchangé. Au-dessus : croissant, impair, jamais au plafond.
    for m in [0.0f32, 0.1, -0.3, 0.5, -0.5] {
        assert_eq!(limiter(m), m);
    }
    let mut avant = SEUIL_LIMITEUR;
    for i in 1..200 {
        let m = SEUIL_LIMITEUR + i as f32 * 0.05;
        let y = limiter(m);
        assert!(y >= avant && y < PLAFOND_SORTIE + 1e-6 && y < 1.0, "{m} -> {y}");
        assert_eq!(limiter(-m), -y);
        avant = y;
    }
    // Pas de marche au seuil.
    assert!((limiter(SEUIL_LIMITEUR + 1e-3) - SEUIL_LIMITEUR - 1e-3).abs() < 1e-4);
}
