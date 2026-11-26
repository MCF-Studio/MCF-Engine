//! Tests de l'état météo et des transitions (sans rendu).

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::{TimePlugin, TimeUpdateStrategy};
use mcf_meteo::{ChangerMeteo, EtatMeteo, McfMeteoPlugin, PresetMeteo, TransitionMeteo};

const EPS: f32 = 1e-4;

fn app_test(pas: Duration) -> App {
    let mut app = App::new();
    app.add_plugins((TimePlugin, McfMeteoPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(pas));
    // Première image : le temps démarre (delta nul).
    app.update();
    app
}

fn etat(app: &App) -> EtatMeteo {
    *app.world().resource::<EtatMeteo>()
}

#[test]
fn etat_par_defaut_est_clair() {
    let app = app_test(Duration::from_millis(100));
    assert_eq!(etat(&app), PresetMeteo::Clair.etat());
}

#[test]
fn interpolation_aux_bornes() {
    let a = PresetMeteo::Clair.etat();
    let b = PresetMeteo::Tempete.etat();
    assert_eq!(EtatMeteo::interpoler(&a, &b, 0.0), a);
    let fin = EtatMeteo::interpoler(&a, &b, 1.0);
    assert!((fin.vent_vitesse - b.vent_vitesse).abs() < EPS);
    assert!((fin.precipitations - b.precipitations).abs() < EPS);
}

#[test]
fn angle_du_vent_par_le_chemin_le_plus_court() {
    let a = EtatMeteo {
        vent_angle_deg: 350.0,
        ..default()
    };
    let b = EtatMeteo {
        vent_angle_deg: 10.0,
        ..default()
    };
    let milieu = EtatMeteo::interpoler(&a, &b, 0.5);
    // 350° → 10° passe par 0°, pas par 180°.
    assert!(milieu.vent_angle_deg.abs() < EPS || (milieu.vent_angle_deg - 360.0).abs() < EPS);
}

#[test]
fn direction_du_vent_dans_le_repere_mcf() {
    // 0° = vers l'avant (−Z), 90° = vers la droite (+X).
    let nord = EtatMeteo {
        vent_angle_deg: 0.0,
        ..default()
    }
    .direction_vent();
    assert!(nord.abs_diff_eq(Vec3::NEG_Z, EPS));
    let est = EtatMeteo {
        vent_angle_deg: 90.0,
        ..default()
    }
    .direction_vent();
    assert!(est.abs_diff_eq(Vec3::X, EPS));
}

#[test]
fn pluie_ou_neige_selon_la_temperature() {
    let pluie = PresetMeteo::Pluie.etat();
    assert_eq!(pluie.facteur_neige(), 0.0);
    assert!((pluie.pluie() - pluie.precipitations).abs() < EPS);
    let neige = PresetMeteo::Neige.etat();
    assert_eq!(neige.facteur_neige(), 1.0);
    assert!((neige.neige() - neige.precipitations).abs() < EPS);
    assert_eq!(neige.pluie(), 0.0);
    // À 0,25 °C : moitié pluie, moitié neige.
    let mixte = EtatMeteo {
        temperature_c: 0.25,
        precipitations: 1.0,
        ..default()
    };
    assert!((mixte.facteur_neige() - 0.5).abs() < EPS);
}

#[test]
fn bornage_des_valeurs_recues() {
    let mut e = EtatMeteo {
        couverture_nuageuse: 3.0,
        vent_vitesse: f32::NAN,
        vent_angle_deg: -90.0,
        precipitations: -1.0,
        temperature_c: f32::INFINITY,
        ..default()
    };
    e.borner();
    assert_eq!(e.couverture_nuageuse, 1.0);
    assert_eq!(e.vent_vitesse, EtatMeteo::default().vent_vitesse);
    assert!((e.vent_angle_deg - 270.0).abs() < EPS);
    assert_eq!(e.precipitations, 0.0);
    assert_eq!(e.temperature_c, EtatMeteo::default().temperature_c);
}

#[test]
fn transition_douce_vers_un_preset() {
    // Pas de 0,2 s : sous le plafond par défaut de `Time<Virtual>` (0,25 s par image).
    let mut app = app_test(Duration::from_millis(200));
    let depart = etat(&app);
    let cible = PresetMeteo::Tempete.etat();
    app.world_mut()
        .write_message(ChangerMeteo { cible, duree_s: 10.0 });

    // 25 images de 0,2 s = mi-parcours : smoothstep(0,5) = 0,5.
    for _ in 0..25 {
        app.update();
    }
    let milieu = etat(&app);
    let attendu = (depart.vent_vitesse + cible.vent_vitesse) * 0.5;
    assert!(
        (milieu.vent_vitesse - attendu).abs() < 1e-3,
        "vent à mi-parcours : {} (attendu {attendu})",
        milieu.vent_vitesse
    );
    assert!(app.world().resource::<TransitionMeteo>().en_cours());

    for _ in 0..25 {
        app.update();
    }
    let fin = etat(&app);
    assert!((fin.vent_vitesse - cible.vent_vitesse).abs() < EPS);
    assert!((fin.precipitations - cible.precipitations).abs() < EPS);
    assert!(!app.world().resource::<TransitionMeteo>().en_cours());
}

#[test]
fn changement_immediat() {
    let mut app = app_test(Duration::from_millis(16));
    app.world_mut().write_message(ChangerMeteo {
        cible: PresetMeteo::Neige.etat(),
        duree_s: 0.0,
    });
    app.update();
    assert_eq!(etat(&app), PresetMeteo::Neige.etat());
}

#[test]
fn reglage_manuel_conserve_hors_transition() {
    let mut app = app_test(Duration::from_millis(16));
    app.world_mut().resource_mut::<EtatMeteo>().couverture_nuageuse = 0.9;
    app.update();
    app.update();
    assert_eq!(etat(&app).couverture_nuageuse, 0.9);
}

#[test]
fn tous_les_presets_sont_valides() {
    for p in PresetMeteo::TOUS {
        let e = p.etat();
        let mut borne = e;
        borne.borner();
        assert_eq!(e, borne, "le préréglage {} sort de son domaine", p.nom());
    }
}
