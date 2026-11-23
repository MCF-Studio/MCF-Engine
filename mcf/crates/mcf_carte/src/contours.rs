//! Contours nommés (`contours.json`) : polygones simples qui ne sont ni des routes ni des quartiers. Premier usage :
//! l'entrée d'un souterrain qui percera le relief (ADR-008 du jeu, par exemple `Grotte_SOCK_Entree`). En v1 le
//! contour est seulement enregistré et validé ; le perçage du terrain viendra avec le masque de trous (spec §5).

use std::collections::BTreeMap;

use mcf_geo::{P2, polygone};
use serde::{Deserialize, Serialize};

use crate::ids::Id;
use crate::manifeste::Manifeste;

/// Valeur du champ `format`.
pub const FORMAT: &str = "mcfcontours";
/// Version du format.
pub const VERSION: u32 = 1;
/// Contours maximum sur une carte.
pub const MAX_CONTOURS: usize = 10_000;
/// Sommets maximum d'un contour.
pub const MAX_SOMMETS: usize = 500;
/// Aire minimale d'un contour (m²).
pub const AIRE_MIN_M2: f64 = 1.0;

/// À quoi sert un contour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageContour {
    /// Entrée de souterrain : le relief sera percé à l'intérieur (ADR-008).
    EntreeSouterrain,
}

/// Un contour nommé.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contour {
    /// Nom lisible par le jeu (`A-Z`, `a-z`, `0-9`, `_`, `-`, 64 caractères au plus), unique sur la carte.
    pub nom: String,
    /// Usage.
    pub usage: UsageContour,
    /// Polygone simple, sens trigonométrique dans `(x, z)`.
    pub polygone: Vec<P2>,
}

/// Tous les contours d'une carte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contours {
    /// Toujours `mcfcontours`.
    pub format: String,
    /// Version.
    pub version: u32,
    /// Contours par identifiant.
    pub contours: BTreeMap<Id, Contour>,
}

impl Default for Contours {
    fn default() -> Self {
        Self {
            format: FORMAT.into(),
            version: VERSION,
            contours: BTreeMap::new(),
        }
    }
}

/// Nom de contour sûr : 1 à 64 caractères `A-Z`, `a-z`, `0-9`, `_`, `-` (jamais de chemin ni d'espace).
pub fn nom_contour_sur(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl Contours {
    /// Contour portant ce nom.
    pub fn par_nom(&self, nom: &str) -> Option<(Id, &Contour)> {
        self.contours
            .iter()
            .find(|(_, c)| c.nom == nom)
            .map(|(id, c)| (*id, c))
    }

    /// Vérifie les contours (donnée non fiable).
    pub fn valider(&self, m: &Manifeste) -> Result<(), String> {
        if self.format != FORMAT || self.version != VERSION {
            return Err(format!("format attendu « {FORMAT} » en version {VERSION}"));
        }
        if self.contours.len() > MAX_CONTOURS {
            return Err("trop de contours".into());
        }
        let mut noms = std::collections::BTreeSet::new();
        for (id, c) in &self.contours {
            if !nom_contour_sur(&c.nom) {
                return Err(format!(
                    "contour {id} : nom invalide (A-Z, a-z, 0-9, _ et -, 64 caractères au plus)"
                ));
            }
            if !noms.insert(c.nom.as_str()) {
                return Err(format!("contour {id} : nom « {} » en double", c.nom));
            }
            if !(3..=MAX_SOMMETS).contains(&c.polygone.len())
                || c.polygone.iter().any(|p| !m.contient(*p))
                || !polygone::est_simple(&c.polygone)
                || polygone::aire(&c.polygone) < AIRE_MIN_M2
            {
                return Err(format!(
                    "contour {id} : polygone invalide (hors carte, croisé, trop petit ou trop long)"
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carre(x: f64) -> Vec<P2> {
        vec![[x, 0.0], [x + 10.0, 0.0], [x + 10.0, 10.0], [x, 10.0]]
    }

    #[test]
    fn validation() {
        let m = Manifeste::nouveau("t", 256.0);
        let mut c = Contours::default();
        c.contours.insert(
            Id(1),
            Contour {
                nom: "Grotte_SOCK_Entree".into(),
                usage: UsageContour::EntreeSouterrain,
                polygone: carre(0.0),
            },
        );
        assert_eq!(c.valider(&m), Ok(()));
        assert_eq!(c.par_nom("Grotte_SOCK_Entree").map(|(id, _)| id), Some(Id(1)));
        // Nom en double.
        let mut double = c.clone();
        double.contours.insert(
            Id(2),
            Contour {
                polygone: carre(20.0),
                ..c.contours[&Id(1)].clone()
            },
        );
        assert!(double.valider(&m).is_err());
        // Nom qui ressemble à un chemin.
        let mut chemin = c.clone();
        chemin.contours.get_mut(&Id(1)).expect("contour").nom = "../grotte".into();
        assert!(chemin.valider(&m).is_err());
        // Polygone croisé.
        let mut croise = c.clone();
        croise.contours.get_mut(&Id(1)).expect("contour").polygone =
            vec![[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]];
        assert!(croise.valider(&m).is_err());
        // Hors carte.
        let mut dehors = c;
        dehors.contours.get_mut(&Id(1)).expect("contour").polygone = carre(200.0);
        assert!(dehors.valider(&m).is_err());
    }
}
