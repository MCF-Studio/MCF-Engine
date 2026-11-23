//! Quartiers (`quartiers.json`) : affectation des îlots à une famille et une densité, zones libres hors réseau.
//!
//! Une affectation retient un **point repère** (là où l'auteur a cliqué) plutôt qu'un identifiant d'îlot : l'îlot
//! qui contient ce point hérite de l'affectation, même après une retouche du réseau.

use std::collections::BTreeMap;

use mcf_geo::P2;
use serde::{Deserialize, Serialize};

use crate::gabarits::{Densite, Famille};
use crate::ids::Id;
use crate::manifeste::Manifeste;

/// Valeur du champ `format`.
pub const FORMAT: &str = "mcfquartiers";
/// Version du format.
pub const VERSION: u32 = 1;
/// Affectations et zones maximum.
pub const MAX_ELEMENTS: usize = 20_000;
/// Sommets maximum d'une zone libre.
pub const MAX_SOMMETS: usize = 500;

/// Un îlot affecté.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Affectation {
    /// Point repère dans l'îlot.
    pub repere: P2,
    /// Famille.
    pub famille: Famille,
    /// Densité.
    pub densite: Densite,
}

/// Une zone dessinée librement (port, zone industrielle sans rues internes, champs…).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZoneLibre {
    /// Contour.
    pub polygone: Vec<P2>,
    /// Famille.
    pub famille: Famille,
    /// Densité.
    pub densite: Densite,
}

/// Tous les quartiers d'une carte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quartiers {
    /// Toujours `mcfquartiers`.
    pub format: String,
    /// Version.
    pub version: u32,
    /// Îlots affectés.
    pub affectations: BTreeMap<Id, Affectation>,
    /// Zones libres.
    pub zones: BTreeMap<Id, ZoneLibre>,
}

impl Default for Quartiers {
    fn default() -> Self {
        Self {
            format: FORMAT.into(),
            version: VERSION,
            affectations: BTreeMap::new(),
            zones: BTreeMap::new(),
        }
    }
}

impl Quartiers {
    /// Vérifie les quartiers.
    pub fn valider(&self, m: &Manifeste) -> Result<(), String> {
        if self.format != FORMAT || self.version != VERSION {
            return Err(format!("format attendu « {FORMAT} » en version {VERSION}"));
        }
        if self.affectations.len() + self.zones.len() > MAX_ELEMENTS {
            return Err("trop de quartiers".into());
        }
        for (id, a) in &self.affectations {
            if !m.contient(a.repere) {
                return Err(format!("affectation {id} hors de la carte"));
            }
        }
        for (id, z) in &self.zones {
            if z.polygone.len() > MAX_SOMMETS
                || z.polygone.iter().any(|p| !m.contient(*p))
                || !mcf_geo::polygone::est_simple(&z.polygone)
            {
                return Err(format!(
                    "zone {id} : contour invalide (hors carte, croisé ou trop long)"
                ));
            }
        }
        Ok(())
    }
}
