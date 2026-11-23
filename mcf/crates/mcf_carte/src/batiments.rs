//! Bâti (`batiments/c_<x>_<z>.json`) : des blocs (empreinte × niveaux), générés ou posés à la main. Rangés par
//! cellule pour limiter les conflits Git quand deux personnes travaillent sur des quartiers différents.

use std::collections::BTreeMap;

use mcf_geo::P2;
use serde::{Deserialize, Serialize};

use crate::gabarits::{Famille, MAX_NIVEAUX};
use crate::ids::Id;
use crate::manifeste::Manifeste;

/// Valeur du champ `format`.
pub const FORMAT: &str = "mcfbatiments";
/// Version du format.
pub const VERSION: u32 = 1;
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
}

impl Batiment {
    /// Hauteur totale (m).
    pub fn hauteur(&self) -> f64 {
        f64::from(self.niveaux) * self.hauteur_niveau_m
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
