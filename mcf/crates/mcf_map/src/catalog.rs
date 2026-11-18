use std::collections::BTreeMap;

/// Ce que la validation a besoin de savoir du catalogue des éléments posables.
pub trait Catalog {
    /// Version du catalogue (augmente à chaque pack d'assets).
    fn version(&self) -> i64;
    /// Réglages modifiables de la fiche `id`, ou `None` si l'identifiant est inconnu.
    fn editable_props(&self, id: &str) -> Option<&[String]>;
}

/// Catalogue en mémoire : identifiant → réglages modifiables.
#[derive(Debug, Clone, Default)]
pub struct SimpleCatalog {
    /// Version du catalogue.
    pub version: i64,
    /// Fiches : identifiant → liste blanche des réglages.
    pub entries: BTreeMap<String, Vec<String>>,
}

impl SimpleCatalog {
    /// Catalogue vide de version `version`.
    pub fn new(version: i64) -> Self {
        Self {
            version,
            entries: BTreeMap::new(),
        }
    }

    /// Ajoute une fiche.
    pub fn with(mut self, id: &str, editable_props: &[&str]) -> Self {
        self.entries.insert(
            id.to_owned(),
            editable_props.iter().map(|s| (*s).to_owned()).collect(),
        );
        self
    }
}

impl Catalog for SimpleCatalog {
    fn version(&self) -> i64 {
        self.version
    }

    fn editable_props(&self, id: &str) -> Option<&[String]> {
        self.entries.get(id).map(Vec::as_slice)
    }
}
