//! Réplique CPU de la mer du vent (la conformité au GPU est vérifiée par l'exemple `verifier_fft`).

use std::time::Instant;

use bevy::prelude::*;
use mcf_meteo::mer_du_vent::{ParamsSpectre, Spectre};
use mcf_meteo::rivage::{hauteur_eau, params_bord};
use mcf_meteo::{CarteRivage, EtatMeteo, MerDuVent, Ocean, ParametresRivage, PresetMeteo, ReglagesFft};

/// Points pseudo-aléatoires reproductibles dans un carré de 400 m.
fn points(n: usize) -> Vec<Vec2> {
    let mut graine = 0x4D43_465Fu64;
    (0..n)
        .map(|_| {
            let mut tirer = || {
                graine = graine
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                ((graine >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 400.0
            };
            Vec2::new(tirer(), tirer())
        })
        .collect()
}

/// Écart RMS relatif entre les `n` premiers modes et le spectre complet.
fn ecart_relatif(spectre: &Spectre, n: usize, t: f32) -> f32 {
    let (mut e2, mut v2) = (0.0f32, 0.0f32);
    for p in points(200) {
        let complet = spectre.deplacement(usize::MAX, t, p, 1.2);
        let reduit = spectre.deplacement(n, t, p, 1.2);
        e2 += (complet - reduit).length_squared();
        v2 += complet.length_squared();
    }
    (e2 / v2.max(1e-12)).sqrt()
}

#[test]
#[ignore = "mesure : cargo test --release -p mcf_meteo --test mer_du_vent -- --ignored --nocapture"]
fn mesure_modes() {
    for preset in [
        PresetMeteo::Clair,
        PresetMeteo::Nuageux,
        PresetMeteo::Pluie,
        PresetMeteo::Tempete,
    ] {
        let p = ParamsSpectre::depuis(&preset.etat(), &ReglagesFft::default());
        let debut = Instant::now();
        let spectre = Spectre::construire(&p, true);
        let construction = debut.elapsed();
        print!(
            "{preset:?} : {} modes, construction {:.2} ms ;",
            spectre.modes.len(),
            construction.as_secs_f64() * 1e3
        );
        for n in [64, 128, 256, 384, 512, 1024] {
            let e: f32 = spectre.modes[..n.min(spectre.modes.len())]
                .iter()
                .map(|m| m.h0.length_squared() + m.h0c.length_squared())
                .sum();
            let (mut e2, mut v2) = (0.0f32, 0.0f32);
            for q in points(200) {
                let c = spectre.deplacement(usize::MAX, 37.0, q, 1.2).y;
                let r = spectre.deplacement(n, 37.0, q, 1.2).y;
                e2 += (c - r) * (c - r);
                v2 += c * c;
            }
            print!(
                " {n}: énergie {:.1} %, écart {:.1} %, vertical RMS {:.1} cm sur {:.1} cm |",
                e / spectre.energie_totale * 100.0,
                ecart_relatif(&spectre, n, 37.0) * 100.0,
                (e2 / 200.0).sqrt() * 100.0,
                (v2 / 200.0).sqrt() * 100.0
            );
        }
        println!();
    }
    for modes in [384u32, 512, 1024] {
        let reglages = ReglagesFft {
            modes_flottaison: modes,
            ..default()
        };
        let etat = PresetMeteo::Tempete.etat();
        let mut ocean = Ocean::default();
        ocean.avancer(&etat, 3.0);
        let mut mer = MerDuVent::default();
        mer.avancer(&etat, &ocean, &reglages);
        let debut = Instant::now();
        mer.avancer(&etat, &ocean, &reglages);
        let image = debut.elapsed();
        let pts = points(2000);
        let carte = CarteRivage::default();
        let b = params_bord(&etat, &ParametresRivage::default());
        let debut = Instant::now();
        let mut s = 0.0;
        for p in &pts {
            s += hauteur_eau(&ocean, Some(&mer), &carte, &b, p.x, p.y);
        }
        println!(
            "{modes} modes : mise à jour par image {:.1} µs, hauteur_eau {:.1} µs par objet ({s:.1})",
            image.as_secs_f64() * 1e6,
            debut.elapsed().as_secs_f64() * 1e6 / pts.len() as f64
        );
    }
}

fn mer(preset: PresetMeteo) -> (Ocean, MerDuVent) {
    let etat = preset.etat();
    let mut ocean = Ocean::default();
    ocean.avancer(&etat, 12.5);
    let mut mer = MerDuVent::default();
    mer.avancer(&etat, &ocean, &ReglagesFft::default());
    (ocean, mer)
}

#[test]
fn spectre_deterministe_et_dans_les_bandes() {
    let p = ParamsSpectre::depuis(&PresetMeteo::Pluie.etat(), &ReglagesFft::default());
    let a = Spectre::construire(&p, true);
    assert_eq!(
        a,
        Spectre::construire(&p, true),
        "même météo, même mer (client et serveur)"
    );
    assert!(a.modes.len() > 1000);
    for m in &a.modes {
        // Demi-plan : le conjugué en −k n'est pas stocké.
        assert!(m.k.y > 0.0 || (m.k.y == 0.0 && m.k.x > 0.0), "{:?}", m.k);
        assert!(m.k.length() >= p.k_min[0] * 0.999);
        assert!(m.h0.is_finite() && m.h0c.is_finite());
    }
    // Triés par énergie décroissante.
    let e = |m: &mcf_meteo::mer_du_vent::Mode| m.h0.length_squared() + m.h0c.length_squared();
    assert!(a.modes.windows(2).all(|w| e(&w[0]) >= e(&w[1])));
}

#[test]
fn motif_boucle_sur_200_s() {
    let p = ParamsSpectre::depuis(&PresetMeteo::Nuageux.etat(), &ReglagesFft::default());
    let s = Spectre::construire(&p, true);
    for q in points(20) {
        let a = s.deplacement(256, 13.0, q, 1.2);
        let b = s.deplacement(256, 213.0, q, 1.2);
        assert!((a - b).length() < 2e-3, "{a} / {b}");
    }
}

#[test]
fn modes_gardes_proches_du_spectre_complet() {
    // Mesuré : avec 512 modes, l'écart RMS est de 2 à 39 % du déplacement de la mer du vent selon la météo (le
    // reste est fait de rides de quelques centimètres, sans effet sur un objet flottant).
    let p = ParamsSpectre::depuis(&PresetMeteo::Pluie.etat(), &ReglagesFft::default());
    let s = Spectre::construire(&p, true);
    assert!(ecart_relatif(&s, 512, 37.0) < 0.2);
    assert!(ecart_relatif(&s, 4096, 37.0) < ecart_relatif(&s, 128, 37.0));
    let p = ParamsSpectre::depuis(&PresetMeteo::Tempete.etat(), &ReglagesFft::default());
    assert!(ecart_relatif(&Spectre::construire(&p, true), 512, 37.0) < 0.05);
}

#[test]
fn hauteur_avec_la_mer_du_vent() {
    let (ocean, mer) = mer(PresetMeteo::Tempete);
    assert_eq!(mer.modes_actifs(), 512);
    assert!(mer.energie_gardee() > 0.99 && mer.energie_gardee() <= 1.0);
    let etat = PresetMeteo::Tempete.etat();
    let b = params_bord(&etat, &ParametresRivage::default());
    let carte = CarteRivage::default();
    let mut ecart_max = 0.0f32;
    for q in points(100) {
        let sans = hauteur_eau(&ocean, None, &carte, &b, q.x, q.y);
        let avec = hauteur_eau(&ocean, Some(&mer), &carte, &b, q.x, q.y);
        assert!(avec.is_finite());
        ecart_max = ecart_max.max((avec - sans).abs());
        // Le point de surface trouvé est bien à la verticale demandée (inversion du déplacement horizontal).
        let (repos, d) = mcf_meteo::rivage::repos_sous(&ocean, Some(&mer), &carte, &b, q);
        let arrivee = repos + Vec2::new(d.x, d.z);
        assert!((avec - d.y - ocean.niveau).abs() < 1e-6);
        assert!((arrivee - q).length() < 0.05, "{q} → {arrivee}");
    }
    assert!(ecart_max > 1.0, "la mer du vent compte en tempête : {ecart_max}");
}

#[test]
fn plugin_met_a_jour_la_mer() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, mcf_meteo::McfMeteoPlugin));
    app.update();
    app.update();
    let mer = app.world().resource::<MerDuVent>();
    assert_eq!(mer.generation(), 1);
    assert_eq!(mer.modes_actifs(), 512);
    let _ = EtatMeteo::default();
}
