//! Construit un projet de carte complet à partir de l'exemple de la convention des cartes sources
//! (`docs/CONVENTION_CARTES_SOURCE.md`) : relief depuis la carte en gris, mer creusée, ville et zone industrielle
//! relevées et loties, autoroute et route relevées. Exactement les gestes de l'auteur dans l'éditeur, sans l'éditeur.
//!
//! ```text
//! cargo run -p mcf_carte --example ile_exemple --release -- <dossier neuf ou vide>
//! cargo editor -- <dossier>
//! ```

use std::path::PathBuf;
use std::time::Instant;

use mcf_carte::{Carte, Densite, Famille, Manifeste, releve, relief_image};
use mcf_geo::ClasseRoute;

fn exemple(nom: &str) -> Vec<u8> {
    let chemin = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/exemples")
        .join(nom);
    std::fs::read(&chemin).unwrap_or_else(|e| panic!("{} : {e}", chemin.display()))
}

fn relever(c: &Carte, id: mcf_carte::Id, sorte: releve::Sorte, px: f64, py: f64) -> Vec<releve::Proposition> {
    let r = &c.references.references[&id];
    let g = releve::Grille::preparer(&c.images_references[&id], r, &c.manifeste).expect("grille");
    g.analyser(sorte, r.vers_carte(px, py), &sorte.reglages())
        .expect("analyse")
}

fn main() -> Result<(), String> {
    let dossier = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage : ile_exemple <dossier neuf ou vide>")?;
    let debut = Instant::now();

    let mut m = Manifeste::nouveau("ile_exemple", 4096.0);
    (m.altitude_min_m, m.altitude_max_m) = (-40.0, 600.0);
    let mut c = Carte::nouvelle(m)?;
    let couleurs = c.importer_reference("couleurs", &exemple("ile_exemple_couleurs.png"))?;
    let relief = c.importer_reference("relief", &exemple("ile_exemple_relief.png"))?;
    c.modifier_reference(relief, "Masquer le calque de relief", |r| {
        r.visible = false;
        Ok(())
    })?;

    let n = c.relief_depuis_reference(
        relief,
        -40.0,
        600.0,
        relief_image::lissage_conseille(-40.0, 600.0, 4.0),
        &relief_image::Niveaux::default(),
    )?;
    println!("Relief : {n} points.");
    if let Some(releve::Proposition::Eau(eau)) = relever(&c, couleurs, releve::Sorte::Eau, 20.0, 20.0).first()
    {
        println!("Mer : {} points creusés.", c.creuser_eau(eau, 0.0, 8.0, 40.0)?);
    }
    for (px, py, famille, nom) in [
        (615.0, 738.0, Famille::Residentiel, "ville"),
        (790.0, 612.0, Famille::Activite, "zone industrielle"),
    ] {
        if let Some(releve::Proposition::Zone { polygone, .. }) =
            relever(&c, couleurs, releve::Sorte::Zone, px, py).first()
        {
            let id = c.ajouter_zone(releve::borner(polygone, &c.manifeste), famille, Densite::Moyenne)?;
            println!("{nom} : {} bâtiment(s).", c.lotir(id)?);
        }
    }
    for (px, py, classe) in [
        (721.0, 685.0, ClasseRoute::Autoroute),
        (300.0, 600.0, ClasseRoute::Route),
    ] {
        for p in relever(&c, couleurs, releve::Sorte::Routes, px, py)
            .iter()
            .take(1)
        {
            if let releve::Proposition::Route { trace, .. } = p {
                let ids = c.tracer_route(classe, &releve::borner(trace, &c.manifeste), 4.0)?;
                println!("{} : {} tronçon(s).", classe.nom(), ids.len());
            }
        }
    }
    c.enregistrer(Some(&dossier))?;
    println!(
        "Projet écrit dans {} en {:?}.",
        dossier.display(),
        debut.elapsed()
    );
    Ok(())
}
