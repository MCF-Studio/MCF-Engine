//! Calques de référence : dans la vue Plan, chaque image visible est un quadrilatère texturé, semi-transparent,
//! posé au-dessus du terrain et sous le voile des quartiers ; dans la vue 3D, la même image est drapée sur le terrain
//! final (relief + terrassements), juste au-dessus du sol. Les réglages en cours de saisie et le glisser de l'outil
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

use mcf_carte::Relief;

use super::{COUCHE_3D, COUCHE_PLAN, CibleGlisse, Edition, Projet, textures::mipmaps};

/// Points du drapé 3D par côté : une maille tous les 1/`DRAPE` de l'image.
const DRAPE: usize = 128;
/// Hauteur du drapé au-dessus du sol (m) : évite le scintillement avec le terrain.
const DRAPE_DESSUS_M: f64 = 0.35;
/// Écart vertical entre deux calques superposés dans le Plan (m) : 16 calques au plus tiennent sous le voile des
/// quartiers (+10 m au-dessus des calques).
const ETAGE_PLAN_M: f32 = 0.5;
/// Même écart pour le drapé 3D (m).
const ETAGE_3D_M: f64 = 0.08;

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
    /// Révisions du terrain final sous le drapé 3D (relief, réseau, manifeste).
    terrain: (u64, u64, u64),
    /// Textures déjà envoyées, par calque, avec la version des pixels envoyée (`Carte::versions_images`).
    textures: HashMap<Id, (Handle<Image>, u64)>,
    /// Dernier renvoi d'une image peinte (au plus 10 par seconde pendant un geste).
    dernier_envoi: Option<std::time::Instant>,
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
    mut projet: ResMut<Projet>,
    edition: Res<Edition>,
    mut affichees: ResMut<Affichees>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existants: Query<Entity, With<Calque>>,
    boutons: Res<ButtonInput<MouseButton>>,
) {
    // Calque peint : on remplace les pixels de la texture existante (pas de reconstruction, pas de clignotement).
    let attendre = boutons.pressed(MouseButton::Left)
        && affichees
            .dernier_envoi
            .is_some_and(|t| t.elapsed().as_millis() < 100);
    if !attendre {
        let mut envoye = false;
        for (id, (handle, version)) in affichees.textures.iter_mut() {
            let courante = projet.carte.versions_images.get(id).copied().unwrap_or(0);
            if courante != *version
                && let (Some(image), Some(mut gpu)) =
                    (projet.carte.images_references.get(id), images.get_mut(&*handle))
            {
                *gpu = texture(image);
                *version = courante;
                envoye = true;
            }
        }
        if envoye {
            affichees.dernier_envoi = Some(std::time::Instant::now());
        }
    }
    let voulu = references_affichees(&projet.carte, &edition);
    // Au-dessus de tout relief autorisé, sous le voile des quartiers (+20 m) et les repères (+25 m).
    let altitude = projet.carte.manifeste.altitude_max_m as f32 + 10.0;
    let r = projet.carte.revisions;
    let terrain = (r.relief, r.reseau, r.manifeste);
    if voulu == affichees.vu && altitude == affichees.altitude && terrain == affichees.terrain {
        return;
    }
    for e in &existants {
        commands.entity(e).despawn();
    }
    // Textures des calques disparus (retirés, autre carte) : libérées.
    affichees
        .textures
        .retain(|id, _| projet.carte.images_references.contains_key(id));
    let mut a_poser = Vec::new();
    for (id, r) in voulu.iter().filter(|(_, r)| r.visible) {
        let Some(image) = projet.carte.images_references.get(id) else {
            continue;
        };
        let version = projet.carte.versions_images.get(id).copied().unwrap_or(0);
        let texture = affichees
            .textures
            .entry(*id)
            .or_insert_with(|| (images.add(texture(image)), version))
            .0
            .clone();
        a_poser.push((r, texture));
    }
    let sol = &projet.carte.derive().terrain;
    // Chaque calque un peu au-dessus du précédent (ordre de la liste) : deux calques à la même hauteur se disputent
    // l'affichage d'une image à l'autre et scintillent.
    for (k, (r, texture)) in a_poser.into_iter().enumerate() {
        let etage = k as f32;
        let materiau = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 1.0, 1.0, r.opacite),
            base_color_texture: Some(texture),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        });
        commands.spawn((
            Calque,
            Mesh3d(meshes.add(quadrilatere(r, altitude + ETAGE_PLAN_M * etage))),
            MeshMaterial3d(materiau.clone()),
            Transform::IDENTITY,
            RenderLayers::layer(COUCHE_PLAN),
        ));
        commands.spawn((
            Calque,
            Mesh3d(meshes.add(drape(r, sol, DRAPE_DESSUS_M + ETAGE_3D_M * f64::from(etage)))),
            MeshMaterial3d(materiau),
            Transform::IDENTITY,
            RenderLayers::layer(COUCHE_3D),
        ));
    }
    affichees.vu = voulu;
    affichees.altitude = altitude;
    affichees.terrain = terrain;
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

/// Image drapée sur le terrain : grille de `DRAPE` × `DRAPE` mailles entre les quatre coins du calque, chaque
/// point posé `dessus` mètres au-dessus du sol (hors de la carte : hauteur du bord le plus proche, cf. `Relief::hauteur`).
fn drape(r: &Reference, sol: &Relief, dessus: f64) -> Mesh {
    let [hg, hd, bd, bg] = r.coins();
    let lerp = |a: [f64; 2], b: [f64; 2], t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    let n = DRAPE + 1;
    let mut positions = Vec::with_capacity(n * n);
    let mut uvs = Vec::with_capacity(n * n);
    for j in 0..n {
        let v = j as f64 / DRAPE as f64;
        let (gauche, droite) = (lerp(hg, bg, v), lerp(hd, bd, v));
        for i in 0..n {
            let u = i as f64 / DRAPE as f64;
            let p = lerp(gauche, droite, u);
            let y = sol.hauteur(p) + dessus;
            positions.push([p[0] as f32, y as f32, p[1] as f32]);
            uvs.push([u as f32, v as f32]);
        }
    }
    let mut indices = Vec::with_capacity(DRAPE * DRAPE * 6);
    for j in 0..DRAPE {
        for i in 0..DRAPE {
            let a = (j * n + i) as u32;
            let (b, c, d) = (a + 1, a + n as u32, a + n as u32 + 1);
            // Même sens que `quadrilatere` : face vers +Y.
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n * n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
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
