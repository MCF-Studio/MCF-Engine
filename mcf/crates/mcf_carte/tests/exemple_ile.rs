//! Bout en bout sur l'exemple de la convention des cartes sources (`docs/CONVENTION_CARTES_SOURCE.md`,
//! images générées par `mcf/tools/exemple_carte/generer.py`) : ce que fait l'auteur dans l'éditeur, sans l'éditeur.
//! Lancer avec `--nocapture` pour voir les mesures.

use std::path::PathBuf;
use std::time::Instant;

use mcf_carte::{Carte, Manifeste, releve};

fn exemple(nom: &str) -> Vec<u8> {
    let chemin = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/exemples")
        .join(nom);
    std::fs::read(&chemin).unwrap_or_else(|e| panic!("{} : {e}", chemin.display()))
}

fn analyser(
    c: &Carte,
    id: mcf_carte::Id,
    sorte: releve::Sorte,
    px: f64,
    py: f64,
) -> Vec<releve::Proposition> {
    let r = &c.references.references[&id];
    let g = releve::Grille::preparer(&c.images_references[&id], r, &c.manifeste).expect("grille");
    g.analyser(sorte, r.vers_carte(px, py), &sorte.reglages())
        .expect("analyse")
}

#[test]
fn ile_exemple_de_bout_en_bout() {
    // 1. Cadre : 4 096 m, altitudes de −40 à 600 m (celles de la carte de relief).
    let mut m = Manifeste::nouveau("ile_exemple", 4096.0);
    (m.altitude_min_m, m.altitude_max_m) = (-40.0, 600.0);
    let mut c = Carte::nouvelle(m).expect("carte");

    // 2. Calques : 1 024 px sur toute la largeur = 4 m/px, déjà calés (même cadrage, coin sur celui de la carte).
    let couleurs = c
        .importer_reference("couleurs", &exemple("ile_exemple_couleurs.png"))
        .expect("couleurs");
    let relief = c
        .importer_reference("relief", &exemple("ile_exemple_relief.png"))
        .expect("relief");
    assert_eq!(c.references.references[&couleurs].m_par_px, 4.0);

    // 3. Relief depuis la carte en niveaux de gris.
    let t = Instant::now();
    let n = c
        .relief_depuis_reference(relief, -40.0, 600.0, 8.0)
        .expect("relief");
    println!("MESURE relief : {n} points en {:?}", t.elapsed());
    let r = c.references.references[&relief].clone();
    let sommet = c.relief.hauteur(r.vers_carte(460.0, 300.0));
    let lac = c.relief.hauteur(r.vers_carte(333.0, 486.0));
    println!("MESURE sommet {sommet:.1} m, lac {lac:.1} m");
    assert!(sommet > 450.0, "sommet du massif : {sommet}");
    assert!((lac - 140.0).abs() < 6.0, "lac à 140 m : {lac}");

    // 4. Relevé de la mer sur la carte couleurs, puis creusement sous 0 m.
    let t = Instant::now();
    let mer = analyser(&c, couleurs, releve::Sorte::Eau, 20.0, 20.0);
    let releve::Proposition::Eau(eau) = &mer[0] else {
        panic!("{mer:?}")
    };
    let creuses = c.creuser_eau(eau, 0.0, 8.0, 40.0).expect("mer");
    println!("MESURE mer : {creuses} points creusés en {:?}", t.elapsed());
    assert!(c.relief.hauteur(r.vers_carte(20.0, 20.0)) < 0.0);

    // 5. La ville (tache urbaine, rues comblées) et l'autoroute (trait rouge de 6 px).
    let t = Instant::now();
    let ville = analyser(&c, couleurs, releve::Sorte::Zone, 615.0, 738.0);
    let autoroute = analyser(&c, couleurs, releve::Sorte::Routes, 721.0, 685.0);
    println!(
        "MESURE relevés : {} zone(s), {} tracé(s) en {:?}",
        ville.len(),
        autoroute.len(),
        t.elapsed()
    );
    let Some(releve::Proposition::Zone { polygone, .. }) = ville.first() else {
        panic!("{ville:?}")
    };
    let aire = mcf_geo::polygone::aire(polygone).abs();
    // Ellipse de 0,26 × 0,18 demi-axes sur 4 096 m : π × 532 × 368 ≈ 0,62 km² (moins la côte).
    println!("MESURE ville : {:.2} km²", aire / 1e6);
    assert!((0.4e6..0.7e6).contains(&aire), "aire de la ville : {aire}");
    let Some(releve::Proposition::Route { trace, .. }) = autoroute.first() else {
        panic!("{autoroute:?}")
    };
    let longueur: f64 = trace.windows(2).map(|w| mcf_geo::distance(w[0], w[1])).sum();
    println!("MESURE autoroute : {:.0} m relevés d'un clic", longueur);
    assert!(longueur > 1500.0, "autoroute relevée : {longueur} m");
}
