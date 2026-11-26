//! Reproduit des coups de pinceau « Abaisser » sur une vraie carte et mesure l'effet sur le terrain final : ce qui
//! monte, ce qui descend, et l'écart avec un calcul complet.
//!
//! `cargo run -p mcf_carte --release --example mesurer_pinceau -- <dossier> <x> <z> <rayon> <coups>`

use mcf_carte::{Carte, relief::Pinceau};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut carte = Carte::ouvrir(std::path::Path::new(&a[1])).expect("carte");
    let (x, z): (f64, f64) = (a[2].parse().expect("x"), a[3].parse().expect("z"));
    let rayon: f64 = a[4].parse().expect("rayon");
    let coups: usize = a[5].parse().expect("coups");
    let avant_src = carte.relief.clone();
    let avant = carte.derive().terrain.clone();
    // Comme l'éditeur : 6 m/s, une image à 60 i/s.
    for _ in 0..coups {
        carte.pinceau(Pinceau::Abaisser, [x, z], rayon, 6.0 / 60.0, 0.0);
        carte.derive();
    }
    carte.fin_trait();
    let apres = carte.derive().terrain.clone();
    let r = &carte.relief;
    let (mut monte, mut descend, mut max_monte, mut max_src_monte) = (0, 0, 0.0f32, 0.0f32);
    let mut pire = (0.0f32, [0.0, 0.0]);
    for i in 0..apres.h.len() {
        let d = apres.h[i] - avant.h[i];
        if d > 0.01 {
            monte += 1;
            if d > max_monte {
                max_monte = d;
                pire = (d, apres.position(i % apres.nx, i / apres.nx));
            }
        } else if d < -0.01 {
            descend += 1;
        }
        max_src_monte = max_src_monte.max(r.h[i] - avant_src.h[i]);
    }
    println!(
        "source : monte au plus de {max_src_monte:.3} m ; terrain final : {monte} échantillon(s) montent (max {max_monte:.2} m en {:?}), {descend} descendent",
        pire.1
    );
    println!(
        "sous le pinceau : source {:.2} → {:.2} m, final {:.2} → {:.2} m",
        avant_src.hauteur([x, z]),
        r.hauteur([x, z]),
        avant.hauteur([x, z]),
        apres.hauteur([x, z])
    );
    println!("coupe en x (z = {z}) : dx | source avant → après | final avant → après");
    for k in -12..=12 {
        let p = [x + f64::from(k) * 8.0, z];
        println!(
            "{:+5} | {:7.2} → {:7.2} | {:7.2} → {:7.2}",
            k * 8,
            avant_src.hauteur(p),
            r.hauteur(p),
            avant.hauteur(p),
            apres.hauteur(p)
        );
    }
    let d = carte.derive().clone();
    let complet =
        mcf_carte::relief::terrain_final(&carte.relief, &d.geo, &d.altitudes, &carte.manifeste.profils);
    let ecarts = complet
        .h
        .iter()
        .zip(&apres.h)
        .filter(|(a, b)| a.to_bits() != b.to_bits())
        .count();
    println!("écart avec un calcul complet : {ecarts} échantillon(s)");
}
