//! Mesure le coût d'une retouche de route sur une vraie carte : ouverture, premiers dérivés, puis un nœud déplacé de
//! 5 m et les dérivés recalculés, étape par étape.
//!
//! `cargo run -p mcf_carte --release --example mesurer_edition -- <dossier de carte>`

use std::time::Instant;

use mcf_carte::{Carte, geometrie, ilots, relief};

fn main() {
    let dossier = std::env::args().nth(1).expect("dossier de carte attendu");
    let t = Instant::now();
    let mut carte = Carte::ouvrir(std::path::Path::new(&dossier)).expect("carte");
    println!("ouverture : {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    let t = Instant::now();
    carte.derive();
    println!(
        "dérivés complets (1re fois) : {:.0} ms",
        t.elapsed().as_secs_f64() * 1e3
    );

    // Étapes séparées, sur la carte telle quelle.
    let profils = carte.manifeste.profils;
    let t = Instant::now();
    let geo = geometrie::calculer(&carte.reseau, &profils);
    let t_geo = t.elapsed();
    let t = Instant::now();
    let alt = relief::altitudes_reseau(&carte.reseau, &geo, &carte.relief, &profils);
    let t_alt = t.elapsed();
    let t = Instant::now();
    let _terrain = relief::terrain_final(&carte.relief, &geo, &alt, &profils);
    let t_ter = t.elapsed();
    let t = Instant::now();
    let _il = ilots::calculer(&carte.reseau, &geo, &profils);
    let t_il = t.elapsed();
    println!(
        "géométrie {:.0} ms · altitudes {:.0} ms · terrain {:.0} ms · îlots {:.0} ms",
        t_geo.as_secs_f64() * 1e3,
        t_alt.as_secs_f64() * 1e3,
        t_ter.as_secs_f64() * 1e3,
        t_il.as_secs_f64() * 1e3
    );

    // Un nœud déplacé de 5 m, comme un glisser dans l'éditeur.
    let (id, p) = carte
        .reseau
        .noeuds
        .iter()
        .map(|(i, n)| (*i, n.position))
        .nth(carte.reseau.noeuds.len() / 2)
        .expect("nœud");
    let t = Instant::now();
    carte
        .modifier("mesure", |tx| {
            let (r, m) = tx.reseau_et_manifeste();
            r.deplacer_noeud(id, [p[0] + 5.0, p[1]], m);
            Ok(())
        })
        .expect("déplacement");
    println!(
        "commande (validation) : {:.0} ms",
        t.elapsed().as_secs_f64() * 1e3
    );
    let t = Instant::now();
    carte.derive();
    let d = carte.derive();
    println!(
        "dérivés après un nœud déplacé : {:.0} ms ({} bloc(s) de terrain refait(s))",
        t.elapsed().as_secs_f64() * 1e3,
        d.maj_terrain.blocs.as_ref().map_or(0, |b| b.len())
    );
    let t = Instant::now();
    let n = carte.conflits().len();
    println!(
        "conflits bâtiments / routes : {:.0} ms ({n})",
        t.elapsed().as_secs_f64() * 1e3
    );
    let t = Instant::now();
    carte.annuler();
    carte.derive();
    println!("annulation + dérivés : {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
}
