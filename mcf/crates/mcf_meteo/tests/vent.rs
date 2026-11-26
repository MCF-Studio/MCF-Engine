//! Tests du bruit et du champ de vent.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::{TimePlugin, TimeUpdateStrategy};
use mcf_meteo::bruit::{hash, perlin3};
use mcf_meteo::vent::rotationnel_bruit;
use mcf_meteo::{ChampVent, EtatMeteo, McfMeteoPlugin, PresetMeteo};

#[test]
fn hachage_identique_a_skyforge() {
    // Valeurs de référence de la fonction d'origine (lowbias32), recalculées indépendamment en Python.
    assert_eq!(hash(0), 0);
    assert_eq!(hash(1), 0x688990c0);
    assert_eq!(hash(2), 0xd1132181);
    assert_eq!(hash(0xdead_beef), 0xe628c683);
}

#[test]
fn perlin_borne_et_nul_aux_noeuds() {
    for i in 0..2000 {
        let p = Vec3::new(i as f32 * 0.37, i as f32 * 0.11 - 50.0, i as f32 * -0.23);
        let v = perlin3(p, 0);
        assert!(v.abs() <= 1.0, "perlin3({p}) = {v}");
    }
    // Aux nœuds du réseau, le bruit de gradient vaut 0.
    assert!(perlin3(Vec3::new(3.0, -2.0, 7.0), 0).abs() < 1e-6);
}

#[test]
fn perlin_periodique() {
    let p = Vec3::new(1.3, 4.7, -2.2);
    let a = perlin3(p, 30);
    let b = perlin3(p + Vec3::new(30.0, -60.0, 90.0), 30);
    assert!((a - b).abs() < 1e-4);
}

#[test]
fn turbulence_a_divergence_nulle() {
    // div(rot(A)) = 0. Avec le même pas que le rotationnel (0,1), les différences centrées commutent : la
    // divergence discrète est nulle à l'arrondi près.
    let h = 0.1;
    for i in 0..50 {
        let p = Vec3::new(i as f32 * 0.71, 3.0 + i as f32 * 0.13, -(i as f32) * 0.37);
        let div = (rotationnel_bruit(p + Vec3::X * h).x - rotationnel_bruit(p - Vec3::X * h).x
            + rotationnel_bruit(p + Vec3::Y * h).y
            - rotationnel_bruit(p - Vec3::Y * h).y
            + rotationnel_bruit(p + Vec3::Z * h).z
            - rotationnel_bruit(p - Vec3::Z * h).z)
            / (2.0 * h);
        let norme = rotationnel_bruit(p).length().max(1.0);
        assert!(div.abs() / norme < 1e-3, "divergence {div} en {p}");
    }
}

#[test]
fn profil_de_couche_limite() {
    // Sans rafales ni turbulence : vitesse = U10 × profil.
    let etat = EtatMeteo {
        rafales: 0.0,
        vent_vitesse: 10.0,
        ..PresetMeteo::Clair.etat()
    };
    let champ = ChampVent::default();
    let moyenne = |y: f32| {
        // La turbulence (0,3 × 0,12 × U) moyenne sur plusieurs points.
        let n = 64;
        (0..n)
            .map(|i| {
                champ
                    .echantillonner(&etat, Vec3::new(i as f32 * 13.0, y, i as f32 * 7.0))
                    .vitesse
            })
            .sum::<Vec3>()
            .dot(etat.direction_vent())
            / n as f32
    };
    let a_10m = moyenne(10.0);
    assert!((a_10m - 10.0).abs() < 1.0, "à 10 m : {a_10m} m/s");
    // Plus faible près du sol, plus fort en altitude, plafonné à 1,6 × U10.
    assert!(moyenne(1.0) < a_10m);
    assert!(moyenne(100.0) > a_10m);
    assert!(moyenne(5000.0) < 1.6 * 10.0 + 1.0);
}

#[test]
fn vent_nul_sans_vitesse() {
    let etat = EtatMeteo {
        vent_vitesse: 0.0,
        ..default()
    };
    let e = ChampVent::default().echantillonner(&etat, Vec3::new(5.0, 20.0, -3.0));
    assert_eq!(e.vitesse, Vec3::ZERO);
    assert_eq!(e.turbulence, 0.0);
}

#[test]
fn rafales_font_varier_le_vent() {
    let calme = EtatMeteo {
        rafales: 0.0,
        ..PresetMeteo::Tempete.etat()
    };
    let tempete = PresetMeteo::Tempete.etat();
    let champ = ChampVent::default();
    let ecart = |e: &EtatMeteo| {
        let v: Vec<f32> = (0..200)
            .map(|i| {
                champ
                    .echantillonner(e, Vec3::new(i as f32 * 9.0, 30.0, 0.0))
                    .vitesse
                    .length()
            })
            .collect();
        let m = v.iter().sum::<f32>() / v.len() as f32;
        (v.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / v.len() as f32).sqrt()
    };
    assert!(ecart(&tempete) > 2.0 * ecart(&calme));
}

#[test]
fn transport_borne_sur_une_longue_session() {
    let mut champ = ChampVent::default();
    let etat = PresetMeteo::Tempete.etat();
    // 10 heures à 60 images/s.
    for _ in 0..(10 * 3600 * 60) {
        champ.avancer(&etat, 1.0 / 60.0);
    }
    let t = champ.transport();
    assert!(
        t.min_element() >= 0.0 && t.max_element() <= 2500.0,
        "transport {t}"
    );
    assert!(champ.phase_rafales() >= 0.0 && champ.phase_rafales() <= 30.0);
    assert!(
        champ
            .echantillonner(&etat, Vec3::new(0.0, 10.0, 0.0))
            .vitesse
            .is_finite()
    );
}

#[test]
fn le_plugin_fait_avancer_le_vent() {
    let mut app = App::new();
    app.add_plugins((TimePlugin, McfMeteoPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)));
    app.update();
    let avant = app.world().resource::<ChampVent>().transport();
    for _ in 0..10 {
        app.update();
    }
    let apres = app.world().resource::<ChampVent>().transport();
    let etat = *app.world().resource::<EtatMeteo>();
    // 1 s de vent à 5 m/s dans la direction du vent.
    let attendu = etat.direction_vent() * etat.vent_vitesse;
    // Le transport boucle sur 2 500 m : on compare l'écart ramené dans [-1250, 1250).
    let ecart =
        (apres - avant - attendu + Vec3::splat(1250.0)).rem_euclid(Vec3::splat(2500.0)) - Vec3::splat(1250.0);
    assert!(
        ecart.abs().max_element() < 1e-3,
        "{avant} -> {apres}, attendu +{attendu}"
    );
}
