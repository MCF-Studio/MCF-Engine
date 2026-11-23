//! Correspondance identifiant du catalogue du banc (`bench_*`) → modèle à afficher. Logique pure, sans Bevy.
//!
//! Chaque modèle reçoit un **emplacement** dense (`0 .. MODEL_COUNT`) : la bibliothèque de maillages est un
//! simple tableau indexé par cet emplacement, sans table de hachage ni chaîne à l'exécution.

use mcf_big_map::{
    Kind, MODELS_BUILDING, MODELS_CHARACTER, MODELS_INTERIOR, MODELS_OBJECT, MODELS_TREE, MODELS_VEHICLE,
};

/// Nombre de modèles d'une nature.
pub const fn models_of(kind: Kind) -> usize {
    match kind {
        Kind::Building => MODELS_BUILDING,
        Kind::Interior => MODELS_INTERIOR,
        Kind::Object => MODELS_OBJECT,
        Kind::Tree => MODELS_TREE,
        Kind::Lamp | Kind::ShadowLamp => 1,
        Kind::Character => MODELS_CHARACTER,
        Kind::Vehicle => MODELS_VEHICLE,
    }
}

/// Nombre total de modèles du catalogue du banc.
pub const MODEL_COUNT: usize = {
    let mut n = 0;
    let mut i = 0;
    while i < Kind::ALL.len() {
        n += models_of(Kind::ALL[i]);
        i += 1;
    }
    n
};

/// Un modèle du catalogue du banc : nature + numéro dans la famille.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelRef {
    /// Nature.
    pub kind: Kind,
    /// Numéro du modèle dans sa famille (`< models_of(kind)`).
    pub index: u16,
}

impl ModelRef {
    /// Emplacement dense dans la bibliothèque (`< MODEL_COUNT`).
    pub fn slot(self) -> usize {
        let before: usize = Kind::ALL
            .iter()
            .take_while(|k| **k != self.kind)
            .map(|k| models_of(*k))
            .sum();
        before + usize::from(self.index)
    }

    /// Inverse de [`ModelRef::slot`].
    pub fn from_slot(slot: usize) -> Option<Self> {
        let mut rest = slot;
        for kind in Kind::ALL {
            let n = models_of(kind);
            if rest < n {
                return Some(Self {
                    kind,
                    index: u16::try_from(rest).ok()?,
                });
            }
            rest -= n;
        }
        None
    }

    /// Tous les modèles, dans l'ordre des emplacements.
    pub fn all() -> impl Iterator<Item = Self> {
        (0..MODEL_COUNT).filter_map(Self::from_slot)
    }
}

/// Préfixe des identifiants d'une famille numérotée.
fn prefix(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::Building => Some("bench_batiment_"),
        Kind::Interior => Some("bench_batiment_int_"),
        Kind::Object => Some("bench_objet_"),
        Kind::Tree => Some("bench_arbre_"),
        Kind::Character => Some("bench_personnage_"),
        Kind::Vehicle => Some("bench_vehicule_"),
        Kind::Lamp | Kind::ShadowLamp => None,
    }
}

/// Modèle d'un identifiant du catalogue, ou `None` s'il est inconnu. Strict : le numéro doit avoir exactement
/// la largeur écrite par le générateur (`bench_objet_007`, pas `bench_objet_7`) et exister dans la famille.
pub fn model_of(id: &str) -> Option<ModelRef> {
    match id {
        "bench_lampe" => {
            return Some(ModelRef {
                kind: Kind::Lamp,
                index: 0,
            });
        }
        "bench_lampe_ombre" => {
            return Some(ModelRef {
                kind: Kind::ShadowLamp,
                index: 0,
            });
        }
        _ => {}
    }
    // `bench_batiment_int_` avant `bench_batiment_` : le plus long préfixe d'abord.
    for kind in [
        Kind::Interior,
        Kind::Building,
        Kind::Object,
        Kind::Tree,
        Kind::Character,
        Kind::Vehicle,
    ] {
        let Some(digits) = prefix(kind).and_then(|p| id.strip_prefix(p)) else {
            continue;
        };
        let n = models_of(kind);
        let width = n.saturating_sub(1).to_string().len().max(2);
        if digits.len() != width || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let index: usize = digits.parse().ok()?;
        return if index < n {
            u16::try_from(index).ok().map(|index| ModelRef { kind, index })
        } else {
            None
        };
    }
    None
}

/// Distance (m) au-delà de laquelle une nature disparaît, d'après la colonne « fin » des budgets
/// (`mcf/docs/rendu-lod-objets.md`). `None` : toujours affiché (bâtiments et arbres attendent leurs imposteurs ;
/// les faire disparaître fausserait le survol).
pub fn fade_end(kind: Kind) -> Option<f32> {
    match kind {
        Kind::Object => Some(150.0),
        Kind::Lamp | Kind::ShadowLamp => Some(200.0),
        Kind::Character => Some(150.0),
        Kind::Vehicle => Some(300.0),
        Kind::Building | Kind::Interior | Kind::Tree => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use mcf_map::Catalog;

    use super::*;

    #[test]
    fn tout_le_catalogue_du_banc_a_un_modele_unique() {
        let cat = mcf_big_map::catalog();
        let mut slots = HashSet::new();
        for id in cat.entries.keys() {
            let m = model_of(id).unwrap_or_else(|| panic!("{id} sans modèle"));
            assert!(m.slot() < MODEL_COUNT);
            assert!(slots.insert(m.slot()), "{id} : emplacement en double");
            assert_eq!(ModelRef::from_slot(m.slot()), Some(m));
        }
        // Bijection : chaque emplacement correspond à une fiche du catalogue.
        assert_eq!(slots.len(), MODEL_COUNT);
        assert_eq!(MODEL_COUNT, 30 + 10 + 200 + 8 + 1 + 1 + 10 + 12);
        assert!(cat.editable_props("bench_lampe").is_some());
    }

    #[test]
    fn familles_et_numeros() {
        assert_eq!(
            model_of("bench_batiment_int_09"),
            Some(ModelRef {
                kind: Kind::Interior,
                index: 9
            })
        );
        assert_eq!(
            model_of("bench_objet_199"),
            Some(ModelRef {
                kind: Kind::Object,
                index: 199
            })
        );
        assert_eq!(
            model_of("bench_lampe_ombre").map(|m| m.kind),
            Some(Kind::ShadowLamp)
        );
    }

    #[test]
    fn identifiants_refuses() {
        for id in [
            "",
            "bench_objet_7",
            "bench_objet_0007",
            "bench_objet_200",
            "bench_arbre_08",
            "bench_batiment_int_10",
            "bench_batiment_+1",
            "bench_vehicule_1a",
            "props/street_lamp",
            "bench_lampe2",
        ] {
            assert_eq!(model_of(id), None, "{id}");
        }
        assert_eq!(ModelRef::from_slot(MODEL_COUNT), None);
    }

    #[test]
    fn ordre_des_emplacements() {
        let all: Vec<_> = ModelRef::all().collect();
        assert_eq!(all.len(), MODEL_COUNT);
        assert!(all.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(all[0].slot(), 0);
    }

    #[test]
    fn disparitions_coherentes_avec_les_budgets() {
        assert_eq!(fade_end(Kind::Building), None);
        assert_eq!(fade_end(Kind::Tree), None);
        for k in Kind::ALL {
            if let Some(d) = fade_end(k) {
                assert!((80.0..=300.0).contains(&d), "{k:?} : {d}");
            }
        }
    }
}
