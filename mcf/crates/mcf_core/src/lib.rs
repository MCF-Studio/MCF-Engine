//! Briques communes au jeu, à l'éditeur et au serveur MCF.
//!
//! Aucune dépendance au rendu : le serveur dédié (headless) l'utilise aussi.

use bevy::prelude::*;
pub use mcf_map;

pub mod coords;

/// Identifiant unique et stable d'un élément posé (le `uid` du `.mcfmap`). Réimporter un calque met à jour les
/// mêmes entités au lieu d'en créer d'autres.
#[derive(Component, Reflect, Debug, Clone, PartialEq, Eq, Hash)]
#[reflect(Component)]
pub struct McfUid(pub String);

/// Fiche du catalogue dont l'entité est une instance (`props/street_lamp`).
#[derive(Component, Reflect, Debug, Clone, PartialEq, Eq, Hash)]
#[reflect(Component)]
pub struct CatalogRef(pub String);

/// Calque auquel appartient l'entité (un calque = un fichier `.mcfmap`).
#[derive(Component, Reflect, Debug, Clone, PartialEq, Eq, Hash)]
#[reflect(Component)]
pub struct LayerName(pub String);

/// Plugin commun : enregistre les types MCF (réflexion : inspecteur de l'éditeur, sérialisation des scènes).
pub struct McfCorePlugin;

impl Plugin for McfCorePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<McfUid>()
            .register_type::<CatalogRef>()
            .register_type::<LayerName>();
    }
}
