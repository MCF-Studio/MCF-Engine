//! Bâti (`batiments/c_<x>_<z>.json`) : des blocs (empreinte × niveaux), générés ou posés à la main, et des bâtiments
//! modélisés (fiche du catalogue + cap, spec §6.5). Rangés par cellule pour limiter les conflits Git quand deux
//! personnes travaillent sur des quartiers différents.
//!
//! Version 2 : champ facultatif `modele`. La version 1 (sans modèles) se lit toujours, telle quelle.

use std::collections::BTreeMap;

use mcf_geo::P2;
use serde::{Deserialize, Serialize};

use crate::gabarits::{Famille, MAX_NIVEAUX};
use crate::ids::Id;
use crate::manifeste::Manifeste;

/// Valeur du champ `format`.
pub const FORMAT: &str = "mcfbatiments";
/// Version écrite.
pub const VERSION: u32 = 2;
/// Versions lues (la 1 n'a pas de bâtiments modélisés : même structure, champ absent).
pub const VERSIONS_LUES: [u32; 2] = [1, 2];
/// Bâtiments maximum sur une carte.
pub const MAX_BATIMENTS: usize = 200_000;
/// Sommets maximum d'une empreinte.
pub const MAX_SOMMETS: usize = 64;

/// D'où vient un bâtiment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origine {
    /// Produit par le lotissement : peut être régénéré.
    Genere,
    /// Posé ou retouché à la main : jamais touché par une régénération.
    Manuel,
}

/// Un bâtiment en bloc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batiment {
    /// Empreinte au sol, sens trigonométrique dans `(x, z)`.
    pub empreinte: Vec<P2>,
    /// Niveaux, rez-de-chaussée compris.
    pub niveaux: u32,
    /// Hauteur d'un niveau (m).
    pub hauteur_niveau_m: f64,
    /// Usage.
    pub famille: Famille,
    /// Origine.
    pub origine: Origine,
    /// Verrouillé : ni déplacé, ni supprimé, ni régénéré.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub verrou: bool,
    /// Affectation de quartier qui l'a généré.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affectation: Option<Id>,
    /// Bâtiment modélisé : l'empreinte est alors le rectangle de la fiche, tourné du cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modele: Option<ModelePose>,
}

/// Fiche de catalogue posée et son orientation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelePose {
    /// Identifiant de la fiche (`crate::catalogue`).
    pub fiche: String,
    /// Cap (degrés, [0 ; 360[) : rotation autour de +Y, main droite (vue de dessus, +X tourne vers −Z).
    pub cap_deg: f64,
}

/// Ramène un angle en degrés dans [0 ; 360[.
pub fn normaliser_cap(deg: f64) -> f64 {
    let c = deg.rem_euclid(360.0);
    // `rem_euclid` peut rendre 360 pour un très petit négatif.
    if c >= 360.0 { 0.0 } else { c }
}

/// Tourne `p` de `deg` autour de `pivot`, comme `Quat::from_rotation_y` (x' = x cos + z sin, z' = −x sin + z cos).
pub fn tourner(p: P2, pivot: P2, deg: f64) -> P2 {
    let (s, c) = deg.to_radians().sin_cos();
    let (x, z) = (p[0] - pivot[0], p[1] - pivot[1]);
    [pivot[0] + x * c + z * s, pivot[1] - x * s + z * c]
}

/// Empreinte d'un bâtiment modélisé : rectangle `largeur` (X du modèle) × `profondeur` (Z du modèle) centré sur
/// `centre`, tourné de `cap_deg`. Sens trigonométrique dans `(x, z)`, comme toute empreinte.
pub fn empreinte_rectangle(centre: P2, largeur: f64, profondeur: f64, cap_deg: f64) -> Vec<P2> {
    let (a, b) = (largeur * 0.5, profondeur * 0.5);
    let coins = [[-a, -b], [a, -b], [a, b], [-a, b]]
        .map(|q| tourner([centre[0] + q[0], centre[1] + q[1]], centre, cap_deg));
    mcf_geo::polygone::oriente_positif(&coins)
}

impl Batiment {
    /// Hauteur totale (m).
    pub fn hauteur(&self) -> f64 {
        f64::from(self.niveaux) * self.hauteur_niveau_m
    }

    /// Tourne le bâtiment de `deg` autour de `pivot` puis le décale de `delta` (empreinte et cap).
    pub fn transformer(&mut self, delta: P2, deg: f64, pivot: P2) {
        for p in &mut self.empreinte {
            let q = tourner(*p, pivot, deg);
            *p = [q[0] + delta[0], q[1] + delta[1]];
        }
        if let Some(m) = &mut self.modele {
            m.cap_deg = normaliser_cap(m.cap_deg + deg);
        }
    }

    /// Centre de l'empreinte.
    pub fn centre(&self) -> P2 {
        mcf_geo::polygone::centroide(&self.empreinte)
    }

    /// Vérifie le bâtiment.
    pub fn valider(&self, m: &Manifeste) -> Result<(), String> {
        if self.empreinte.len() < 3 || self.empreinte.len() > MAX_SOMMETS {
            return Err("empreinte : 3 à 64 sommets".into());
        }
        if self.empreinte.iter().any(|p| !m.contient(*p)) || !mcf_geo::polygone::est_simple(&self.empreinte) {
            return Err("empreinte hors carte ou croisée".into());
        }
        if !(1..=MAX_NIVEAUX).contains(&self.niveaux) || !(2.0..=12.0).contains(&self.hauteur_niveau_m) {
            return Err("niveaux ou hauteur hors bornes".into());
        }
        if let Some(m) = &self.modele {
            if !crate::manifeste::nom_sur(&m.fiche) {
                return Err("fiche de modèle : identifiant non sûr".into());
            }
            if !(m.cap_deg.is_finite() && (0.0..360.0).contains(&m.cap_deg)) {
                return Err("cap hors de [0 ; 360[".into());
            }
            if self.empreinte.len() != 4 {
                return Err("bâtiment modélisé : l'empreinte doit être un rectangle".into());
            }
        }
        Ok(())
    }
}

/// Contenu d'un fichier de cellule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FichierBatiments {
    /// Toujours `mcfbatiments`.
    pub format: String,
    /// Version.
    pub version: u32,
    /// Bâtiments de la cellule.
    pub batiments: BTreeMap<Id, Batiment>,
}

/// Cellule d'un point (tuiles du manifeste).
pub fn cellule(m: &Manifeste, p: P2) -> (usize, usize) {
    let (a, _) = m.bornes();
    let t = f64::from(m.tuile_m);
    let (nx, nz) = m.tuiles();
    let c = |v: f64, o: f64, n: usize| (((v - o) / t).floor().max(0.0) as usize).min(n.saturating_sub(1));
    (c(p[0], a[0], nx), c(p[1], a[1], nz))
}

/// Range les bâtiments par cellule.
pub fn par_cellule(
    m: &Manifeste,
    tous: &BTreeMap<Id, Batiment>,
) -> BTreeMap<(usize, usize), BTreeMap<Id, Batiment>> {
    let mut v: BTreeMap<(usize, usize), BTreeMap<Id, Batiment>> = BTreeMap::new();
    for (id, b) in tous {
        v.entry(cellule(m, b.centre()))
            .or_default()
            .insert(*id, b.clone());
    }
    v
}
