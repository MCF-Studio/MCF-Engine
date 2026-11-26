//! Décor posé depuis la bibliothèque d'assets (`decor/c_<x>_<z>.json`) : mobilier urbain, arbres, signalisation,
//! barrières… Un objet = l'identifiant d'un objet de la bibliothèque, une position au sol, un cap, une échelle et un
//! décalage vertical. Rangé par cellule, comme le bâti, pour limiter les conflits Git à deux.
//!
//! Données **non fiables** : champs connus seulement, identifiant sûr (jamais un chemin : l'éditeur retrouve le
//! modèle dans le catalogue validé de la bibliothèque), position dans la carte, bornes sur l'échelle et la hauteur.
//! Un identifiant inconnu de la bibliothèque n'empêche pas d'ouvrir la carte : l'éditeur montre une marque à la place.

use std::collections::BTreeMap;

use mcf_geo::P2;
use serde::{Deserialize, Serialize};

use crate::batiments::{Transformation, normaliser_cap, tourner};
use crate::ids::Id;
use crate::manifeste::{Manifeste, nom_sur};

/// Valeur du champ `format`.
pub const FORMAT: &str = "mcfdecor";
/// Version écrite et lue.
pub const VERSION: u32 = 1;
/// Objets de décor au plus sur une carte.
pub const MAX_OBJETS: usize = 500_000;
/// Bornes de l'échelle.
pub const ECHELLE: (f64, f64) = (0.1, 10.0);
/// Décalage vertical maximal par rapport au sol (m), vers le haut comme vers le bas.
pub const DECALAGE_Y_MAX_M: f64 = 50.0;
/// Longueur maximale d'un identifiant de modèle.
pub const MAX_MODELE: usize = 64;

fn un() -> f64 {
    1.0
}

fn est_un(v: &f64) -> bool {
    *v == 1.0
}

fn est_zero(v: &f64) -> bool {
    *v == 0.0
}

/// Un objet de décor posé.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjetDecor {
    /// Identifiant de l'objet dans la bibliothèque (`mcf_carte::bibliotheque`).
    pub modele: String,
    /// Position au sol `(x, z)` (m).
    pub position: P2,
    /// Cap (degrés, [0 ; 360[) : rotation autour de +Y, même convention que les bâtiments (avant du modèle = +Z).
    pub cap_deg: f64,
    /// Échelle uniforme, dans [`ECHELLE`].
    #[serde(default = "un", skip_serializing_if = "est_un")]
    pub echelle: f64,
    /// Décalage vertical par rapport au sol (m, + vers le haut), borné à ± [`DECALAGE_Y_MAX_M`].
    #[serde(default, skip_serializing_if = "est_zero")]
    pub decalage_y_m: f64,
    /// Verrouillé : ni déplacé, ni supprimé.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub verrou: bool,
}

impl ObjetDecor {
    /// Nouvel objet, cap ramené à [0 ; 360[.
    pub fn nouveau(modele: &str, position: P2, cap_deg: f64) -> Self {
        Self {
            modele: modele.to_owned(),
            position,
            cap_deg: normaliser_cap(cap_deg),
            echelle: 1.0,
            decalage_y_m: 0.0,
            verrou: false,
        }
    }

    /// Vérifie l'objet.
    pub fn valider(&self, m: &Manifeste) -> Result<(), String> {
        if self.modele.len() > MAX_MODELE || !nom_sur(&self.modele) {
            return Err(format!("identifiant de modèle non sûr : {:?}", self.modele));
        }
        if !self.position.iter().all(|c| c.is_finite()) || !m.contient(self.position) {
            return Err("position hors de la carte".into());
        }
        if !(self.cap_deg.is_finite() && (0.0..360.0).contains(&self.cap_deg)) {
            return Err("cap hors de [0 ; 360[".into());
        }
        if !(self.echelle.is_finite() && (ECHELLE.0..=ECHELLE.1).contains(&self.echelle)) {
            return Err(format!("échelle hors de [{} ; {}]", ECHELLE.0, ECHELLE.1));
        }
        if !(self.decalage_y_m.is_finite() && self.decalage_y_m.abs() <= DECALAGE_Y_MAX_M) {
            return Err(format!("décalage vertical hors de ± {DECALAGE_Y_MAX_M} m"));
        }
        Ok(())
    }

    /// Applique un geste (déplacement, rotation autour du pivot, hauteur) ; l'inclinaison est ignorée.
    pub fn transformer(&mut self, t: &Transformation) {
        let p = tourner(self.position, t.pivot, t.cap_deg);
        self.position = [p[0] + t.delta[0], p[1] + t.delta[1]];
        self.cap_deg = normaliser_cap(self.cap_deg + t.cap_deg);
        self.decalage_y_m = (self.decalage_y_m + t.dy_m).clamp(-DECALAGE_Y_MAX_M, DECALAGE_Y_MAX_M);
    }
}

/// Contenu d'un fichier de cellule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FichierDecor {
    /// Toujours [`FORMAT`].
    pub format: String,
    /// Toujours [`VERSION`].
    pub version: u32,
    /// Objets de la cellule.
    pub objets: BTreeMap<Id, ObjetDecor>,
}

/// Range les objets par cellule (tuiles du manifeste).
pub fn par_cellule(
    m: &Manifeste,
    tous: &BTreeMap<Id, ObjetDecor>,
) -> BTreeMap<(usize, usize), BTreeMap<Id, ObjetDecor>> {
    let mut v: BTreeMap<(usize, usize), BTreeMap<Id, ObjetDecor>> = BTreeMap::new();
    for (id, o) in tous {
        v.entry(crate::batiments::cellule(m, o.position))
            .or_default()
            .insert(*id, o.clone());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objet_borne_et_transforme() {
        let m = Manifeste::nouveau("t", 512.0);
        let mut o = ObjetDecor::nouveau("bench_wood", [10.0, 0.0], 370.0);
        assert!((o.cap_deg - 10.0).abs() < 1e-9);
        assert!(o.valider(&m).is_ok());
        o.transformer(&Transformation {
            delta: [1.0, 2.0],
            cap_deg: 90.0,
            pivot: [0.0, 0.0],
            dy_m: 0.5,
            ..Default::default()
        });
        // Rotation de +90° autour de l'origine (vue de dessus, +X tourne vers −Z), puis déplacement.
        assert!(
            (o.position[0] - 1.0).abs() < 1e-9 && (o.position[1] - -8.0).abs() < 1e-9,
            "{:?}",
            o.position
        );
        assert!((o.cap_deg - 100.0).abs() < 1e-9 && (o.decalage_y_m - 0.5).abs() < 1e-9);
        for mauvais in [
            ObjetDecor::nouveau("../x", [0.0, 0.0], 0.0),
            ObjetDecor::nouveau("a", [1e6, 0.0], 0.0),
            ObjetDecor {
                echelle: 50.0,
                ..ObjetDecor::nouveau("a", [0.0, 0.0], 0.0)
            },
            ObjetDecor {
                decalage_y_m: f64::NAN,
                ..ObjetDecor::nouveau("a", [0.0, 0.0], 0.0)
            },
        ] {
            assert!(mauvais.valider(&m).is_err(), "{mauvais:?}");
        }
        // Champ inconnu refusé.
        let r: Result<ObjetDecor, _> =
            serde_json::from_str(r#"{"modele":"a","position":[0,0],"cap_deg":0,"script":"x"}"#);
        assert!(r.is_err());
    }
}
