//! Familles de quartier, densités et gabarits de bâti (spec §6.2). Valeurs par défaut réglables par carte.

use serde::{Deserialize, Serialize};

/// Famille d'un quartier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Famille {
    /// Logements.
    Residentiel,
    /// Commerces, bureaux.
    Commerce,
    /// Entrepôts, ateliers, port.
    Activite,
    /// Équipements publics.
    Services,
    /// Parcs, stades, berges (pas de bâti généré).
    Nature,
    /// Plans d'eau (pas de bâti).
    Eau,
}

impl Famille {
    /// Toutes les familles.
    pub const TOUTES: [Self; 6] = [
        Self::Residentiel,
        Self::Commerce,
        Self::Activite,
        Self::Services,
        Self::Nature,
        Self::Eau,
    ];

    /// Nom affiché.
    pub const fn nom(self) -> &'static str {
        match self {
            Self::Residentiel => "Résidentiel",
            Self::Commerce => "Commerce",
            Self::Activite => "Activité",
            Self::Services => "Services publics",
            Self::Nature => "Nature / loisirs",
            Self::Eau => "Eau",
        }
    }

    /// Teinte du plan stylisé (sRGB).
    pub const fn couleur_plan(self) -> [u8; 3] {
        match self {
            Self::Residentiel => [214, 232, 196],
            Self::Commerce => [190, 210, 238],
            Self::Activite => [214, 200, 232],
            Self::Services => [240, 204, 200],
            Self::Nature => [170, 214, 150],
            Self::Eau => [120, 170, 220],
        }
    }

    /// Des bâtiments peuvent-ils y pousser ?
    pub const fn constructible(self) -> bool {
        !matches!(self, Self::Nature | Self::Eau)
    }
}

/// Densité d'un quartier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Densite {
    /// Pavillonnaire, zone commerciale.
    Faible,
    /// Petits collectifs, rues commerçantes.
    Moyenne,
    /// Barres, bureaux.
    Forte,
    /// Downtown : tours.
    Centre,
}

impl Densite {
    /// Toutes les densités.
    pub const TOUTES: [Self; 4] = [Self::Faible, Self::Moyenne, Self::Forte, Self::Centre];

    /// Nom affiché.
    pub const fn nom(self) -> &'static str {
        match self {
            Self::Faible => "Faible",
            Self::Moyenne => "Moyenne",
            Self::Forte => "Forte",
            Self::Centre => "Centre",
        }
    }
}

/// Règles de lotissement d'une famille à une densité.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gabarit {
    /// Famille.
    pub famille: Famille,
    /// Densité.
    pub densite: Densite,
    /// Nombre de niveaux (rez-de-chaussée compris), min et max.
    pub niveaux: [u32; 2],
    /// Hauteur d'un niveau (m).
    pub hauteur_niveau_m: f64,
    /// Largeur de façade sur rue (m), min et max.
    pub facade_m: [f64; 2],
    /// Profondeur (m), min et max.
    pub profondeur_m: [f64; 2],
    /// Retrait par rapport au bord de la rue (m), min et max.
    pub retrait_m: [f64; 2],
    /// Bâtiments accolés ?
    pub mitoyen: bool,
}

/// Limites de validation.
pub const MAX_NIVEAUX: u32 = 120;

impl Gabarit {
    fn valider(&self) -> Result<(), String> {
        let ok_intervalle = |v: [f64; 2], max: f64| v[0].is_finite() && v[1].is_finite() && 0.0 <= v[0] && v[0] <= v[1] && v[1] <= max;
        if !(1 <= self.niveaux[0] && self.niveaux[0] <= self.niveaux[1] && self.niveaux[1] <= MAX_NIVEAUX) {
            return Err("niveaux hors bornes".into());
        }
        if !(2.0..=12.0).contains(&self.hauteur_niveau_m) {
            return Err("hauteur de niveau hors bornes (2 à 12 m)".into());
        }
        if !ok_intervalle(self.facade_m, 300.0) || self.facade_m[0] < 3.0 {
            return Err("façade hors bornes (3 à 300 m)".into());
        }
        if !ok_intervalle(self.profondeur_m, 300.0) || self.profondeur_m[0] < 3.0 {
            return Err("profondeur hors bornes (3 à 300 m)".into());
        }
        if !ok_intervalle(self.retrait_m, 100.0) {
            return Err("retrait hors bornes (0 à 100 m)".into());
        }
        Ok(())
    }
}

/// Table des gabarits : une entrée par famille constructible et par densité.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TableGabarits(pub Vec<Gabarit>);

#[allow(clippy::too_many_arguments)]
const fn g(
    famille: Famille,
    densite: Densite,
    niveaux: [u32; 2],
    hauteur_niveau_m: f64,
    facade_m: [f64; 2],
    profondeur_m: [f64; 2],
    retrait_m: [f64; 2],
    mitoyen: bool,
) -> Gabarit {
    Gabarit {
        famille,
        densite,
        niveaux,
        hauteur_niveau_m,
        facade_m,
        profondeur_m,
        retrait_m,
        mitoyen,
    }
}

impl TableGabarits {
    /// Valeurs par défaut de la spec (§6.2). Les niveaux comptent le rez-de-chaussée (R+1 = 2 niveaux).
    pub fn defaut() -> Self {
        use Densite::*;
        use Famille::*;
        let mut v = vec![
            g(Residentiel, Faible, [1, 2], 3.0, [10.0, 16.0], [9.0, 12.0], [4.0, 6.0], false),
            g(Residentiel, Moyenne, [3, 6], 3.0, [12.0, 30.0], [12.0, 15.0], [0.0, 3.0], true),
            g(Residentiel, Forte, [6, 15], 3.0, [30.0, 80.0], [12.0, 16.0], [5.0, 10.0], false),
            g(Residentiel, Centre, [16, 46], 3.0, [25.0, 40.0], [20.0, 30.0], [0.0, 0.0], true),
            g(Commerce, Faible, [1, 2], 5.0, [40.0, 120.0], [30.0, 80.0], [15.0, 30.0], false),
            g(Commerce, Moyenne, [3, 7], 3.5, [10.0, 25.0], [12.0, 18.0], [0.0, 0.0], true),
            g(Commerce, Forte, [7, 21], 3.5, [20.0, 50.0], [20.0, 40.0], [0.0, 0.0], true),
            g(Commerce, Centre, [21, 61], 3.5, [30.0, 50.0], [30.0, 50.0], [0.0, 5.0], false),
        ];
        for d in Densite::TOUTES {
            v.push(g(Activite, d, [1, 1], 8.0, [30.0, 120.0], [25.0, 80.0], [8.0, 15.0], false));
            v.push(g(Services, d, [2, 5], 3.5, [30.0, 60.0], [20.0, 40.0], [5.0, 10.0], false));
        }
        Self(v)
    }

    /// Gabarit d'une famille constructible à une densité.
    pub fn get(&self, famille: Famille, densite: Densite) -> Option<&Gabarit> {
        self.0.iter().find(|g| g.famille == famille && g.densite == densite)
    }

    /// Complète et cohérente ?
    pub fn valider(&self) -> Result<(), String> {
        for f in Famille::TOUTES.into_iter().filter(|f| f.constructible()) {
            for d in Densite::TOUTES {
                let n = self.0.iter().filter(|g| g.famille == f && g.densite == d).count();
                if n != 1 {
                    return Err(format!("gabarit {}·{} : {n} entrée(s), 1 attendue", f.nom(), d.nom()));
                }
            }
        }
        for gab in &self.0 {
            if !gab.famille.constructible() {
                return Err(format!("gabarit pour une famille non constructible ({})", gab.famille.nom()));
            }
            gab.valider()
                .map_err(|e| format!("gabarit {}·{} : {e}", gab.famille.nom(), gab.densite.nom()))?;
        }
        Ok(())
    }
}

impl Default for TableGabarits {
    fn default() -> Self {
        Self::defaut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_par_defaut_complete() {
        let t = TableGabarits::defaut();
        assert_eq!(t.valider(), Ok(()));
        assert!(t.get(Famille::Nature, Densite::Faible).is_none());
        let mut t2 = t.clone();
        t2.0.pop();
        assert!(t2.valider().is_err());
    }
}
