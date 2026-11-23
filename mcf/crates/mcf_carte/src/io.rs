//! Dossier de projet de carte : lecture validée et bornée, écriture atomique de ce qui a changé.
//!
//! ```text
//! <dossier>/carte.mcfcarte.json   manifeste
//!          /reseau.json            réseau routier
//!          /quartiers.json         quartiers
//!          /contours.json          contours nommés (entrées de souterrain…)
//!          /batiments/c_X_Z.json   bâti par cellule
//!          /relief/t_X_Z.png       heightmap source 16 bits par tuile (bords partagés)
//!          /calques/<nom>.mcfmap   objets posés
//!          /exports/               rendus (dérivés, régénérables)
//! ```
//!
//! Menaces couvertes : fichiers piégés (taille plafonnée, champs inconnus refusés, validation complète avant
//! usage), bombes PNG (dimensions vérifiées dans l'en-tête avant décodage), sortie du dossier (noms construits
//! par le code, liens symboliques refusés), perte de données (écriture atomique, rien n'est écrit si la carte est
//! invalide).

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::batiments::{self, FichierBatiments};
use crate::carte::{Carte, Sale};
use crate::contours::Contours;
use crate::manifeste::{Manifeste, nom_sur};
use crate::quartiers::Quartiers;
use crate::relief::Relief;
use crate::reseau::Reseau;

/// Nom du manifeste.
pub const FICHIER_MANIFESTE: &str = "carte.mcfcarte.json";
/// Tailles maximales des fichiers lus.
pub const MAX_MANIFESTE: u64 = 1024 * 1024;
/// Réseau.
pub const MAX_RESEAU: u64 = 64 * 1024 * 1024;
/// Quartiers.
pub const MAX_QUARTIERS: u64 = 16 * 1024 * 1024;
/// Contours.
pub const MAX_CONTOURS: u64 = 16 * 1024 * 1024;
/// Une cellule de bâti.
pub const MAX_CELLULE: u64 = 32 * 1024 * 1024;
/// Une tuile PNG.
pub const MAX_TUILE: u64 = 16 * 1024 * 1024;
/// Calques d'objets maximum.
pub const MAX_CALQUES: usize = 256;

fn erreur(chemin: &Path, e: impl std::fmt::Display) -> String {
    format!("{} : {e}", chemin.display())
}

fn verifier_pas_lien(chemin: &Path) -> Result<(), String> {
    match fs::symlink_metadata(chemin) {
        Ok(m) if m.file_type().is_symlink() => Err(erreur(chemin, "lien symbolique refusé")),
        Ok(_) => Ok(()),
        Err(e) => Err(erreur(chemin, e)),
    }
}

fn lire_borne(chemin: &Path, max: u64) -> Result<Vec<u8>, String> {
    verifier_pas_lien(chemin)?;
    let meta = fs::metadata(chemin).map_err(|e| erreur(chemin, e))?;
    if !meta.is_file() || meta.len() > max {
        return Err(erreur(
            chemin,
            format!("fichier absent ou trop gros (max {max} octets)"),
        ));
    }
    fs::read(chemin).map_err(|e| erreur(chemin, e))
}

fn lire_json<T: DeserializeOwned>(chemin: &Path, max: u64) -> Result<T, String> {
    let octets = lire_borne(chemin, max)?;
    serde_json::from_slice(&octets).map_err(|e| erreur(chemin, format!("JSON refusé : {e}")))
}

fn ecrire_json<T: Serialize>(chemin: &Path, v: &T) -> Result<(), String> {
    let mut texte = serde_json::to_vec_pretty(v).map_err(|e| erreur(chemin, e))?;
    texte.push(b'\n');
    mcf_map::write_atomic(chemin, &texte).map_err(|e| erreur(chemin, e))
}

fn nom_tuile(x: usize, z: usize) -> String {
    format!("t_{x:03}_{z:03}.png")
}

fn nom_cellule(x: usize, z: usize) -> String {
    format!("c_{x:03}_{z:03}.json")
}

/// Échantillons par côté de tuile (bords partagés compris).
fn cote_tuile(m: &Manifeste) -> usize {
    (f64::from(m.tuile_m) / m.resolution_relief_m).round() as usize + 1
}

fn encoder_tuile(r: &Relief, m: &Manifeste, tx: usize, tz: usize) -> Result<Vec<u8>, String> {
    let n = cote_tuile(m);
    let (x0, z0) = (tx * (n - 1), tz * (n - 1));
    let (a, b) = (m.altitude_min_m, m.altitude_max_m);
    let mut donnees = Vec::with_capacity(n * n * 2);
    for iz in z0..z0 + n {
        for ix in x0..x0 + n {
            let h = f64::from(r.h[r.index(ix.min(r.nx - 1), iz.min(r.nz - 1))]);
            let code = (((h - a) / (b - a)) * 65535.0).round().clamp(0.0, 65535.0) as u16;
            donnees.extend_from_slice(&code.to_be_bytes());
        }
    }
    let mut sortie = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut sortie, n as u32, n as u32);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Sixteen);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&donnees).map_err(|e| e.to_string())?;
        w.finish().map_err(|e| e.to_string())?;
    }
    Ok(sortie)
}

fn decoder_tuile(octets: Vec<u8>, m: &Manifeste, r: &mut Relief, tx: usize, tz: usize) -> Result<(), String> {
    let n = cote_tuile(m);
    let mut dec = png::Decoder::new(Cursor::new(octets));
    dec.set_limits(png::Limits {
        bytes: n * n * 2 + 1024,
    });
    let mut lecteur = dec.read_info().map_err(|e| e.to_string())?;
    let info = lecteur.info();
    // Dimensions et format vérifiés AVANT de décoder les pixels.
    if info.width as usize != n
        || info.height as usize != n
        || info.color_type != png::ColorType::Grayscale
        || info.bit_depth != png::BitDepth::Sixteen
    {
        return Err(format!("tuile attendue : {n}×{n}, niveaux de gris 16 bits"));
    }
    let mut tampon = vec![0u8; n * n * 2];
    lecteur.next_frame(&mut tampon).map_err(|e| e.to_string())?;
    let (x0, z0) = (tx * (n - 1), tz * (n - 1));
    let (a, b) = (m.altitude_min_m, m.altitude_max_m);
    for dz in 0..n {
        for dx in 0..n {
            let (ix, iz) = (x0 + dx, z0 + dz);
            if ix < r.nx && iz < r.nz {
                let k = (dz * n + dx) * 2;
                let code = u16::from_be_bytes([tampon[k], tampon[k + 1]]);
                let i = r.index(ix, iz);
                r.h[i] = (a + f64::from(code) / 65535.0 * (b - a)) as f32;
            }
        }
    }
    Ok(())
}

impl Carte {
    /// Ouvre un projet de carte. Tout est validé avant d'être rendu ; un fichier refusé fait échouer l'ouverture.
    pub fn ouvrir(dossier: &Path) -> Result<Carte, String> {
        verifier_pas_lien(dossier)?;
        let manifeste: Manifeste = lire_json(&dossier.join(FICHIER_MANIFESTE), MAX_MANIFESTE)?;
        manifeste.valider().map_err(|e| format!("manifeste : {e}"))?;
        let reseau: Reseau = lire_json(&dossier.join("reseau.json"), MAX_RESEAU)?;
        let quartiers: Quartiers = lire_json(&dossier.join("quartiers.json"), MAX_QUARTIERS)?;
        let contours: Contours = lire_json(&dossier.join("contours.json"), MAX_CONTOURS)?;
        let mut tous = BTreeMap::new();
        let dossier_bat = dossier.join("batiments");
        if dossier_bat.exists() {
            verifier_pas_lien(&dossier_bat)?;
            for entree in fs::read_dir(&dossier_bat).map_err(|e| erreur(&dossier_bat, e))? {
                let chemin = entree.map_err(|e| erreur(&dossier_bat, e))?.path();
                let nom = chemin.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !(nom.starts_with("c_") && nom.ends_with(".json")) {
                    continue;
                }
                let f: FichierBatiments = lire_json(&chemin, MAX_CELLULE)?;
                if f.format != batiments::FORMAT || f.version != batiments::VERSION {
                    return Err(erreur(&chemin, "format de bâti non géré"));
                }
                for (id, b) in f.batiments {
                    if tous.insert(id, b).is_some() {
                        return Err(erreur(&chemin, format!("bâtiment {id} en double")));
                    }
                }
            }
        }
        let mut relief = Relief::plat(&manifeste, 0.0);
        let (tx, tz) = manifeste.tuiles();
        for z in 0..tz {
            for x in 0..tx {
                let chemin = dossier.join("relief").join(nom_tuile(x, z));
                let octets = lire_borne(&chemin, MAX_TUILE)?;
                decoder_tuile(octets, &manifeste, &mut relief, x, z).map_err(|e| erreur(&chemin, e))?;
            }
        }
        let mut calques = BTreeMap::new();
        let dossier_calques = dossier.join("calques");
        if dossier_calques.exists() {
            verifier_pas_lien(&dossier_calques)?;
            for entree in fs::read_dir(&dossier_calques).map_err(|e| erreur(&dossier_calques, e))? {
                let chemin = entree.map_err(|e| erreur(&dossier_calques, e))?.path();
                let Some(nom) = chemin
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| n.strip_suffix(".mcfmap"))
                else {
                    continue;
                };
                if !nom_sur(nom) {
                    return Err(erreur(&chemin, "nom de calque non sûr"));
                }
                verifier_pas_lien(&chemin)?;
                let charge = mcf_map::load_file(&chemin, None).map_err(|e| erreur(&chemin, e))?;
                calques.insert(nom.to_owned(), charge.map);
                if calques.len() > MAX_CALQUES {
                    return Err("trop de calques".into());
                }
            }
        }
        let carte = Carte::assembler(
            dossier.to_path_buf(),
            manifeste,
            reseau,
            quartiers,
            contours,
            tous,
            relief,
            calques,
        );
        carte.valider()?;
        Ok(carte)
    }

    /// Enregistre dans `dossier` (ou le dossier courant). Un nouveau dossier doit être vide ou inexistant.
    /// N'écrit que ce qui a changé ; n'écrit rien si la carte est invalide.
    pub fn enregistrer(&mut self, dossier: Option<&Path>) -> Result<(), String> {
        self.fin_trait();
        self.valider()?;
        let cible: PathBuf = match (dossier, &self.dossier) {
            (Some(d), Some(actuel)) if d == actuel.as_path() => actuel.clone(),
            (Some(d), _) => {
                if d.exists() {
                    verifier_pas_lien(d)?;
                    if fs::read_dir(d).map_err(|e| erreur(d, e))?.next().is_some() {
                        return Err(erreur(d, "dossier non vide : choisissez un dossier neuf"));
                    }
                }
                // Nouveau dossier : tout est à écrire.
                let (tx, tz) = self.manifeste.tuiles();
                self.sale = Sale {
                    manifeste: true,
                    reseau: true,
                    quartiers: true,
                    contours: true,
                    batiments: true,
                    relief: (0..tz).flat_map(|z| (0..tx).map(move |x| (x, z))).collect(),
                    calques: self.calques.keys().cloned().collect(),
                };
                d.to_path_buf()
            }
            (None, Some(actuel)) => actuel.clone(),
            (None, None) => return Err("aucun dossier choisi".into()),
        };
        fs::create_dir_all(&cible).map_err(|e| erreur(&cible, e))?;
        verifier_pas_lien(&cible)?;
        if self.sale.manifeste {
            ecrire_json(&cible.join(FICHIER_MANIFESTE), &self.manifeste)?;
        }
        if self.sale.reseau {
            ecrire_json(&cible.join("reseau.json"), &self.reseau)?;
        }
        if self.sale.quartiers {
            ecrire_json(&cible.join("quartiers.json"), &self.quartiers)?;
        }
        if self.sale.contours {
            ecrire_json(&cible.join("contours.json"), &self.contours)?;
        }
        if self.sale.batiments {
            let d = cible.join("batiments");
            fs::create_dir_all(&d).map_err(|e| erreur(&d, e))?;
            let cellules = batiments::par_cellule(&self.manifeste, &self.batiments);
            let mut a_garder = std::collections::BTreeSet::new();
            for ((x, z), contenu) in cellules {
                let nom = nom_cellule(x, z);
                ecrire_json(
                    &d.join(&nom),
                    &FichierBatiments {
                        format: batiments::FORMAT.into(),
                        version: batiments::VERSION,
                        batiments: contenu,
                    },
                )?;
                a_garder.insert(nom);
            }
            // Cellules devenues vides : seuls nos propres fichiers `c_*.json` sont supprimés.
            for entree in fs::read_dir(&d).map_err(|e| erreur(&d, e))?.flatten() {
                let nom = entree.file_name().to_string_lossy().into_owned();
                if nom.starts_with("c_") && nom.ends_with(".json") && !a_garder.contains(&nom) {
                    fs::remove_file(entree.path()).map_err(|e| erreur(&entree.path(), e))?;
                }
            }
        }
        if !self.sale.relief.is_empty() {
            let d = cible.join("relief");
            fs::create_dir_all(&d).map_err(|e| erreur(&d, e))?;
            for (x, z) in self.sale.relief.clone() {
                let octets = encoder_tuile(&self.relief, &self.manifeste, x, z)?;
                mcf_map::write_atomic(&d.join(nom_tuile(x, z)), &octets).map_err(|e| erreur(&d, e))?;
            }
        }
        for nom in self.sale.calques.clone() {
            if !nom_sur(&nom) {
                return Err(format!("nom de calque non sûr : {nom}"));
            }
            let chemin = cible.join("calques").join(format!("{nom}.mcfmap"));
            match self.calques.get(&nom) {
                Some(c) => mcf_map::save_file(&chemin, c).map_err(|e| erreur(&chemin, e))?,
                None => {
                    if chemin.exists() {
                        fs::remove_file(&chemin).map_err(|e| erreur(&chemin, e))?;
                    }
                }
            }
        }
        self.dossier = Some(cible);
        self.sale = Sale::default();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gabarits::{Densite, Famille};
    use crate::ids::Id;
    use crate::relief::Pinceau;
    use mcf_geo::ClasseRoute;

    fn dossier_test(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mcf_carte_test_{nom}_{}", Id::nouveau()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn aller_retour_complet() {
        let mut c = Carte::nouvelle(Manifeste::nouveau("essai", 512.0)).expect("carte");
        for k in [-100.0, 0.0, 100.0] {
            c.tracer_route(ClasseRoute::Rue, &[[-100.0, k], [100.0, k]], 2.0)
                .expect("h");
            c.tracer_route(ClasseRoute::Rue, &[[k, -100.0], [k, 100.0]], 2.0)
                .expect("v");
        }
        c.pinceau(Pinceau::Lever, [150.0, 150.0], 40.0, 12.0, 0.0);
        let a = c
            .affecter([50.0, 50.0], Famille::Commerce, Densite::Moyenne)
            .expect("affecter");
        c.lotir(a).expect("lotir");
        let d = dossier_test("aller_retour");
        c.enregistrer(Some(&d)).expect("enregistrer");
        let relue = Carte::ouvrir(&d).expect("ouvrir");
        assert_eq!(relue.reseau, c.reseau);
        assert_eq!(relue.quartiers, c.quartiers);
        assert_eq!(relue.batiments, c.batiments);
        // Précision 16 bits sur 512 m de plage : < 8 mm.
        let ecart = relue
            .relief
            .h
            .iter()
            .zip(&c.relief.h)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(ecart < 0.008, "écart de relief {ecart}");
        // Dossier non vide refusé.
        let mut autre = Carte::nouvelle(Manifeste::nouveau("autre", 256.0)).expect("carte");
        assert!(autre.enregistrer(Some(&d)).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn fichiers_pieges_refuses() {
        let mut c = Carte::nouvelle(Manifeste::nouveau("piege", 256.0)).expect("carte");
        let d = dossier_test("piege");
        c.enregistrer(Some(&d)).expect("enregistrer");
        // Champ inconnu.
        let m = fs::read_to_string(d.join(FICHIER_MANIFESTE)).expect("lire");
        fs::write(d.join(FICHIER_MANIFESTE), m.replacen("{", "{\"pirate\": 1,", 1)).expect("écrire");
        assert!(Carte::ouvrir(&d).is_err());
        fs::write(d.join(FICHIER_MANIFESTE), m).expect("écrire");
        assert!(Carte::ouvrir(&d).is_ok());
        // Tuile aux mauvaises dimensions.
        let mut petit = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut petit, 4, 4);
            enc.set_color(png::ColorType::Grayscale);
            enc.set_depth(png::BitDepth::Sixteen);
            let mut w = enc.write_header().expect("en-tête");
            w.write_image_data(&[0u8; 32]).expect("données");
        }
        fs::write(d.join("relief").join(nom_tuile(0, 0)), petit).expect("écrire");
        assert!(Carte::ouvrir(&d).is_err());
        let _ = fs::remove_dir_all(&d);
    }
}
