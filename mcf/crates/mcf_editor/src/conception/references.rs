//! Calques de référence dans la vue Plan : chaque image visible est un quadrilatère texturé, semi-transparent, posé
//! au-dessus du terrain et sous le voile des quartiers. Les réglages en cours de saisie et le glisser de l'outil
//! « Déplacer » sont affichés en direct, avant d'être écrits dans la carte.

use std::collections::HashMap;

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::RenderLayers,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use mcf_carte::{Carte, Id, Reference};

use super::{COUCHE_PLAN, CibleGlisse, Edition, Projet, textures::mipmaps};

pub struct ReferencesPlugin;

impl Plugin for ReferencesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Affichees>().add_systems(Update, afficher);
    }
}

/// Un quadrilatère de calque affiché.
#[derive(Component)]
struct Calque;

#[derive(Resource, Default)]
struct Affichees {
    /// Ce qui est à l'écran (pour ne reconstruire qu'en cas de changement).
    vu: Vec<(Id, Reference)>,
    /// Altitude d'affichage utilisée.
    altitude: f32,
    /// Textures déjà envoyées, par calque (une image importée ne change jamais).
    textures: HashMap<Id, Handle<Image>>,
}

/// Calques tels qu'ils doivent apparaître : ceux de la carte, avec les réglages en cours de saisie et le glisser en
/// cours appliqués.
pub fn references_affichees(carte: &Carte, edition: &Edition) -> Vec<(Id, Reference)> {
    carte
        .references
        .references
        .iter()
        .map(|(id, r)| {
            let mut r = match &edition.reference_saisie {
                Some((i, saisie)) if i == id => saisie.clone(),
                _ => r.clone(),
            };
            if let Some(g) = edition.glisse
                && g.cible == CibleGlisse::Reference(*id)
            {
                r.coin = [
                    r.coin[0] + g.actuel[0] - g.depart[0],
                    r.coin[1] + g.actuel[1] - g.depart[1],
                ];
            }
            (*id, r)
        })
        .collect()
}

fn afficher(
    mut commands: Commands,
    projet: Res<Projet>,
    edition: Res<Edition>,
    mut affichees: ResMut<Affichees>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existants: Query<Entity, With<Calque>>,
) {
    let voulu = references_affichees(&projet.carte, &edition);
    // Au-dessus de tout relief autorisé, sous le voile des quartiers (+20 m) et les repères (+25 m).
    let altitude = projet.carte.manifeste.altitude_max_m as f32 + 10.0;
    if voulu == affichees.vu && altitude == affichees.altitude {
        return;
    }
    for e in &existants {
        commands.entity(e).despawn();
    }
    // Textures des calques disparus (retirés, autre carte) : libérées.
    affichees
        .textures
        .retain(|id, _| projet.carte.images_references.contains_key(id));
    for (id, r) in voulu.iter().filter(|(_, r)| r.visible) {
        let Some(image) = projet.carte.images_references.get(id) else {
            continue;
        };
        let texture = affichees
            .textures
            .entry(*id)
            .or_insert_with(|| images.add(texture(image)))
            .clone();
        commands.spawn((
            Calque,
            Mesh3d(meshes.add(quadrilatere(r, altitude))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgba(1.0, 1.0, 1.0, r.opacite),
                base_color_texture: Some(texture),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            })),
            Transform::IDENTITY,
            RenderLayers::layer(COUCHE_PLAN),
        ));
    }
    affichees.vu = voulu;
    affichees.altitude = altitude;
}

/// Texture d'un calque, avec ses mipmaps (lisible dézoomée).
fn texture(image: &mcf_carte::ImageRgba) -> Image {
    let (l, h) = (image.largeur, image.hauteur);
    let mut img = Image::new(
        Extent3d {
            width: l,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        image.rgba.to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    if let Some((chaine, niveaux)) = mipmaps(&image.rgba, l, h, true) {
        img.data = Some(chaine);
        img.texture_descriptor.mip_level_count = niveaux;
    }
    img
}

/// Quadrilatère horizontal couvrant l'image, coordonnées de texture aux coins.
fn quadrilatere(r: &Reference, y: f32) -> Mesh {
    let c = r.coins().map(|q| [q[0] as f32, y, q[1] as f32]);
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, c.to_vec())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; 4])
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vec![[0.0f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        )
        // Haut gauche, haut droit, bas droit, bas gauche : face vers +Y vu du dessus (nord en haut).
        .with_inserted_indices(Indices::U32(vec![0, 3, 1, 1, 3, 2]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conception::Glisse;

    fn png(l: u32, h: u32) -> Vec<u8> {
        mcf_carte::references::encoder_png(&mcf_carte::ImageRgba {
            largeur: l,
            hauteur: h,
            rgba: std::sync::Arc::new(vec![128; (l * h * 4) as usize]),
        })
        .expect("png")
    }

    #[test]
    fn apercu_suit_la_saisie_et_le_glisser_sans_toucher_la_carte() {
        let mut carte = Carte::nouvelle(mcf_carte::Manifeste::nouveau("t", 512.0)).expect("carte");
        let id = carte.importer_reference("plan", &png(8, 4)).expect("import");
        let mut edition = Edition::default();
        let mut saisie = carte.references.references[&id].clone();
        saisie.opacite = 0.9;
        edition.reference_saisie = Some((id, saisie));
        edition.glisse = Some(Glisse {
            depart: [0.0, 0.0],
            actuel: [10.0, -5.0],
            cible: CibleGlisse::Reference(id),
        });
        let enregistre = carte.references.references[&id].clone();
        let vu = references_affichees(&carte, &edition);
        assert_eq!(vu.len(), 1);
        assert_eq!(vu[0].1.opacite, 0.9);
        assert_eq!(
            vu[0].1.coin,
            [enregistre.coin[0] + 10.0, enregistre.coin[1] - 5.0]
        );
        // La carte n'a pas bougé : rien n'est écrit avant « Appliquer » ou le relâchement.
        assert_eq!(carte.references.references[&id], enregistre);
    }

    #[test]
    fn quadrilatere_couvre_l_image() {
        let mut carte = Carte::nouvelle(mcf_carte::Manifeste::nouveau("t", 512.0)).expect("carte");
        let id = carte.importer_reference("plan", &png(8, 4)).expect("import");
        let r = &carte.references.references[&id];
        let m = quadrilatere(r, 5.0);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        let (a, b) = carte.manifeste.bornes();
        // Importée sur toute la largeur de la carte, coin haut gauche sur celui de la carte.
        assert_eq!(p[0], [a[0] as f32, 5.0, a[1] as f32]);
        assert_eq!(p[1], [b[0] as f32, 5.0, a[1] as f32]);
    }
}
