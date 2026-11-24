//! Catalogue des bâtiments modélisés (spec §6.5) : une fiche par modèle `.glb`, avec son encombrement mesuré.
//!
//! Le fichier est **non fiable** comme toute donnée de carte : champs connus seulement, identifiants sûrs, chemin du
//! modèle relatif et confiné au dossier du catalogue, dimensions bornées. Aucun code n'est lu depuis le catalogue.

use std::collections::BTreeSet;

use mcf_geo::P2;
use serde::{Deserialize, Serialize};

use crate::batiments::{Batiment, ModelePose, Origine, empreinte_rectangle, normaliser_cap};
use crate::gabarits::{Famille, MAX_NIVEAUX};
use crate::manifeste::nom_sur;

/// Valeur du champ `format`.
pub const FORMAT: &str = "mcfcatalogue_batiments";
/// Version du format.
pub const VERSION: u32 = 1;
/// Fiches au plus.
pub const MAX_FICHES: usize = 2_000;
/// Taille maximale du fichier (octets).
pub const MAX_OCTETS: u64 = 1024 * 1024;
/// Bornes d'une dimension (m).
pub const DIMENSION_M: (f64, f64) = (0.5, 200.0);
/// Hauteur visée d'un niveau (m) pour déduire les niveaux d'un modèle.
const NIVEAU_M: f64 = 3.0;

/// Une fiche : un bâtiment modélisé.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fiche {
    /// Identifiant sûr (référencé par les bâtiments posés).
    pub id: String,
    /// Nom affiché.
    pub nom: String,
    /// Usage (teinte du Plan, statistiques).
    pub famille: Famille,
    /// Modèle `.glb`, relatif au dossier du catalogue (`/` comme séparateur).
    pub fichier: String,
    /// Encombrement en X du modèle (m), sans les maillages de collision.
    pub largeur_m: f64,
    /// Encombrement en Z du modèle (m).
    pub profondeur_m: f64,
    /// Hauteur (m).
    pub hauteur_m: f64,
    /// Centre de l'encombrement au sol, dans le repère du modèle (`x`, `z`, m).
    pub centre_m: P2,
}

impl Fiche {
    /// Vérifie la fiche.
    pub fn valider(&self) -> Result<(), String> {
        if !nom_sur(&self.id) {
            return Err(format!("identifiant non sûr : {:?}", self.id));
        }
        if self.nom.trim().is_empty()
            || self.nom.chars().count() > 80
            || self.nom.chars().any(char::is_control)
        {
            return Err(format!(
                "{} : nom vide, trop long ou avec des caractères de contrôle",
                self.id
            ));
        }
        if !fichier_sur(&self.fichier) {
            return Err(format!(
                "{} : fichier refusé (relatif, `.glb`, sans `..`)",
                self.id
            ));
        }
        let (min, max) = DIMENSION_M;
        for (nom, v) in [
            ("largeur", self.largeur_m),
            ("profondeur", self.profondeur_m),
            ("hauteur", self.hauteur_m),
        ] {
            if !(v.is_finite() && (min..=max).contains(&v)) {
                return Err(format!("{} : {nom} hors de [{min} ; {max}] m", self.id));
            }
        }
        if !self.centre_m.iter().all(|c| c.is_finite() && c.abs() <= max) {
            return Err(format!("{} : centre hors bornes", self.id));
        }
        Ok(())
    }

    /// Niveaux et hauteur d'un niveau qui donnent la hauteur du modèle (bornes des bâtiments respectées).
    pub fn niveaux(&self) -> (u32, f64) {
        let n = ((self.hauteur_m / NIVEAU_M).round() as u32).clamp(1, MAX_NIVEAUX);
        (n, (self.hauteur_m / f64::from(n)).clamp(2.0, 12.0))
    }

    /// Bâtiment manuel de cette fiche, centré sur `centre`, orienté de `cap_deg`.
    pub fn batiment(&self, centre: P2, cap_deg: f64) -> Batiment {
        let cap_deg = normaliser_cap(cap_deg);
        let (niveaux, hauteur_niveau_m) = self.niveaux();
        Batiment {
            empreinte: empreinte_rectangle(centre, self.largeur_m, self.profondeur_m, cap_deg),
            niveaux,
            hauteur_niveau_m,
            famille: self.famille,
            origine: Origine::Manuel,
            verrou: false,
            affectation: None,
            modele: Some(ModelePose {
                fiche: self.id.clone(),
                cap_deg,
                tangage_deg: 0.0,
                roulis_deg: 0.0,
            }),
            decalage_y_m: 0.0,
        }
    }
}

/// Chemin de modèle sûr : segments `A-Z a-z 0-9 _ - .` séparés par `/`, aucun segment vide, `.` ou `..`, extension
/// `.glb`, 200 caractères au plus. Ni chemin absolu, ni lecteur (`C:`), ni `\`.
pub fn fichier_sur(s: &str) -> bool {
    (5..=200).contains(&s.len())
        && s.ends_with(".glb")
        && s.split('/').all(|seg| {
            !seg.is_empty()
                && seg != "."
                && seg != ".."
                && seg
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
        })
}

/// Le fichier de catalogue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogueBatiments {
    /// Toujours `mcfcatalogue_batiments`.
    pub format: String,
    /// Version.
    pub version: u32,
    /// Fiches, dans l'ordre d'affichage.
    pub fiches: Vec<Fiche>,
}

impl CatalogueBatiments {
    /// Lit et valide un catalogue (texte JSON).
    pub fn lire(texte: &str) -> Result<Self, String> {
        if texte.len() as u64 > MAX_OCTETS {
            return Err("catalogue trop gros".into());
        }
        let c: Self = serde_json::from_str(texte).map_err(|e| format!("catalogue illisible : {e}"))?;
        c.valider()?;
        Ok(c)
    }

    /// Vérifie le catalogue.
    pub fn valider(&self) -> Result<(), String> {
        if self.format != FORMAT || self.version != VERSION {
            return Err(format!("format attendu « {FORMAT} » en version {VERSION}"));
        }
        if self.fiches.len() > MAX_FICHES {
            return Err("trop de fiches".into());
        }
        let mut vus = BTreeSet::new();
        for f in &self.fiches {
            f.valider()?;
            if !vus.insert(f.id.as_str()) {
                return Err(format!("fiche {} en double", f.id));
            }
        }
        Ok(())
    }

    /// Fiche d'identifiant `id`.
    pub fn fiche(&self, id: &str) -> Option<&Fiche> {
        self.fiches.iter().find(|f| f.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batiments::{DECALAGE_Y_MAX_M, INCLINAISON_MAX_DEG, Transformation, tourner};

    fn maison() -> Fiche {
        Fiche {
            id: "maison_a".into(),
            nom: "Maison A".into(),
            famille: Famille::Residentiel,
            fichier: "house_a/House_A.glb".into(),
            largeur_m: 15.1,
            profondeur_m: 9.5,
            hauteur_m: 6.22,
            centre_m: [1.65, 0.25],
        }
    }

    fn catalogue(fiches: Vec<Fiche>) -> String {
        serde_json::to_string(&CatalogueBatiments {
            format: FORMAT.into(),
            version: VERSION,
            fiches,
        })
        .expect("json")
    }

    #[test]
    fn catalogue_valide_relu() {
        let c = CatalogueBatiments::lire(&catalogue(vec![maison()])).expect("valide");
        assert_eq!(c.fiche("maison_a").map(|f| f.largeur_m), Some(15.1));
    }

    #[test]
    fn chemins_pieges_refuses() {
        for f in [
            "../secret.glb",
            "a/../../b.glb",
            "/abs.glb",
            "C:/x.glb",
            "a\\b.glb",
            "a//b.glb",
            "./a.glb",
            "maison.gltf",
            "maison.glb.exe",
            "é.glb",
        ] {
            let mut m = maison();
            m.fichier = f.into();
            assert!(
                CatalogueBatiments::lire(&catalogue(vec![m])).is_err(),
                "{f} accepté"
            );
        }
    }

    #[test]
    fn fiches_invalides_refusees() {
        let mut doubles = vec![maison(), maison()];
        assert!(CatalogueBatiments::lire(&catalogue(doubles.clone())).is_err());
        doubles.pop();
        let mut m = maison();
        m.largeur_m = f64::NAN;
        assert!(CatalogueBatiments::lire(&catalogue(vec![m])).is_err());
        let mut m = maison();
        m.hauteur_m = 500.0;
        assert!(CatalogueBatiments::lire(&catalogue(vec![m])).is_err());
        let mut m = maison();
        m.id = "Maison A".into();
        assert!(CatalogueBatiments::lire(&catalogue(vec![m])).is_err());
        // Champ inconnu.
        let texte = catalogue(vec![maison()]).replace("\"nom\"", "\"script\":\"x\",\"nom\"");
        assert!(CatalogueBatiments::lire(&texte).is_err());
    }

    #[test]
    fn batiment_de_fiche_est_un_rectangle_oriente() {
        let b = maison().batiment([10.0, 20.0], 90.0);
        assert_eq!(b.empreinte.len(), 4);
        assert!(
            mcf_geo::polygone::aire_signee(&b.empreinte) > 0.0,
            "sens trigonométrique"
        );
        assert!((mcf_geo::polygone::aire(&b.empreinte) - 15.1 * 9.5).abs() < 1e-6);
        let c = b.centre();
        assert!((c[0] - 10.0).abs() < 1e-9 && (c[1] - 20.0).abs() < 1e-9);
        // Cap 90° : la largeur (X du modèle) court le long de Z.
        let (min, max) = mcf_geo::polygone::englobant(&b.empreinte);
        assert!(((max[0] - min[0]) - 9.5).abs() < 1e-6 && ((max[1] - min[1]) - 15.1).abs() < 1e-6);
        assert_eq!(b.niveaux, 2);
        assert!((b.hauteur() - 6.22).abs() < 1e-9);
    }

    #[test]
    fn rotation_comme_quat_y() {
        // Quat::from_rotation_y(90°) envoie +X sur −Z.
        let q = tourner([1.0, 0.0], [0.0, 0.0], 90.0);
        assert!(q[0].abs() < 1e-12 && (q[1] + 1.0).abs() < 1e-12);
    }

    #[test]
    fn transformer_tourne_le_cap_et_l_empreinte() {
        let mut b = maison().batiment([0.0, 0.0], 350.0);
        b.transformer(&Transformation {
            delta: [5.0, 0.0],
            cap_deg: 20.0,
            ..Transformation::default()
        });
        let m = b.modele.as_ref().expect("modèle");
        assert!(
            (m.cap_deg - 10.0).abs() < 1e-9,
            "cap ramené dans [0 ; 360[ : {}",
            m.cap_deg
        );
        let attendu = maison().batiment([5.0, 0.0], 10.0);
        for (a, e) in b.empreinte.iter().zip(&attendu.empreinte) {
            assert!((a[0] - e[0]).abs() < 1e-9 && (a[1] - e[1]).abs() < 1e-9);
        }
    }

    #[test]
    fn hauteur_et_inclinaison_bornees() {
        let mut b = maison().batiment([0.0, 0.0], 0.0);
        let t = Transformation {
            dy_m: 80.0,
            tangage_deg: 30.0,
            roulis_deg: -70.0,
            ..Transformation::default()
        };
        b.transformer(&t);
        let m = b.modele.as_ref().expect("modèle");
        assert_eq!(b.decalage_y_m, DECALAGE_Y_MAX_M);
        assert_eq!((m.tangage_deg, m.roulis_deg), (30.0, -INCLINAISON_MAX_DEG));
        b.valider(&crate::Manifeste::nouveau("t", 512.0))
            .expect("valide une fois borné");
        // Une valeur forcée hors bornes (fichier piégé) est refusée.
        b.decalage_y_m = 51.0;
        assert!(b.valider(&crate::Manifeste::nouveau("t", 512.0)).is_err());
    }

    #[test]
    fn modele_mal_forme_refuse() {
        let m = crate::Manifeste::nouveau("t", 512.0);
        let mut b = maison().batiment([0.0, 0.0], 0.0);
        b.valider(&m).expect("valide");
        b.modele.as_mut().expect("modèle").cap_deg = 360.0;
        assert!(b.valider(&m).is_err());
        let mut b = maison().batiment([0.0, 0.0], 0.0);
        b.empreinte.pop();
        assert!(b.valider(&m).is_err());
        let mut b = maison().batiment([0.0, 0.0], 0.0);
        b.modele.as_mut().expect("modèle").fiche = "../x".into();
        assert!(b.valider(&m).is_err());
    }
}
