//! Matériaux texturés de la vue 3D (Poly Haven, CC0 ; voir `assets/textures/SOURCES.md`).
//!
//! Les textures sont plaquées en coordonnées **monde** (mètres) : `uv` des maillages = position en mètres, la taille
//! d'une répétition est portée par le matériau (`uv_transform`). Les couleurs de sommet multiplient la texture : elles
//! servent de teinte (famille de bâtiment, altitude, pente).
//!
//! Les PNG n'ont pas de mipmaps : sans elles, une texture répétée scintille et moire au loin. Elles sont calculées
//! une fois au chargement (`mipmaps`), sur le processeur.

use bevy::{
    image::{ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor},
    math::Affine2,
    prelude::*,
    render::render_resource::TextureFormat,
};

/// Matériaux 3D partagés, par surface.
#[derive(Resource, Clone)]
pub struct Surfaces {
    /// Terrain et îlots plantés.
    pub herbe: Handle<StandardMaterial>,
    /// Chemins et accotements en terre.
    pub terre: Handle<StandardMaterial>,
    /// Chaussées revêtues.
    pub asphalte: Handle<StandardMaterial>,
    /// Trottoirs, places.
    pub trottoir: Handle<StandardMaterial>,
    /// Murs.
    pub facade: Handle<StandardMaterial>,
    /// Toits plats.
    pub toit: Handle<StandardMaterial>,
}

/// Images en attente de mipmaps, avec le matériau à rafraîchir ensuite.
#[derive(Resource, Default)]
struct EnAttente(Vec<(Handle<Image>, Handle<StandardMaterial>)>);

pub struct TexturesPlugin;

impl Plugin for TexturesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EnAttente>()
            .add_systems(PreStartup, charger)
            .add_systems(Update, completer_mipmaps);
    }
}

/// `(rôle = dossier, taille d'une répétition en m)`.
const ROLES: [(&str, f32); 6] = [
    ("herbe", 3.0),
    ("terre", 3.0),
    ("asphalte", 4.0),
    ("trottoir", 2.5),
    ("facade", 3.0),
    ("toit", 4.0),
];

fn charger(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut attente: ResMut<EnAttente>,
) {
    let mut faire = |role: &str, repetition_m: f32| {
        let image = |carte: &str, srgb: bool| -> Handle<Image> {
            assets
                .load_builder()
                .with_settings(move |s: &mut ImageLoaderSettings| {
                    s.is_srgb = srgb;
                    s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                        address_mode_u: ImageAddressMode::Repeat,
                        address_mode_v: ImageAddressMode::Repeat,
                        mag_filter: ImageFilterMode::Linear,
                        min_filter: ImageFilterMode::Linear,
                        mipmap_filter: ImageFilterMode::Linear,
                        anisotropy_clamp: 8,
                        ..default()
                    });
                })
                .load(format!("textures/{role}/{carte}.png"))
        };
        let (diff, nor, arm) = (image("diff", true), image("nor_gl", false), image("arm", false));
        let mat = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(diff.clone()),
            normal_map_texture: Some(nor.clone()),
            // ARM = occlusion (R), rugosité (G), métal (B) : la convention glTF que suit Bevy.
            metallic_roughness_texture: Some(arm.clone()),
            occlusion_texture: Some(arm.clone()),
            perceptual_roughness: 1.0,
            metallic: 0.0,
            uv_transform: Affine2::from_scale(Vec2::splat(1.0 / repetition_m)),
            ..default()
        });
        for i in [diff, nor, arm] {
            attente.0.push((i, mat.clone()));
        }
        mat
    };
    let [herbe, terre, asphalte, trottoir, facade, toit] = ROLES.map(|(r, t)| faire(r, t));
    commands.insert_resource(Surfaces {
        herbe,
        terre,
        asphalte,
        trottoir,
        facade,
        toit,
    });
}

/// Ajoute les mipmaps aux images chargées, puis marque leur matériau modifié pour qu'il soit relié à la nouvelle image.
fn completer_mipmaps(
    mut attente: ResMut<EnAttente>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if attente.0.is_empty() {
        return;
    }
    let mut a_rafraichir = Vec::new();
    attente.0.retain(|(h, mat)| {
        let Some(mut image) = images.get_mut(h) else {
            return true;
        };
        let srgb = match image.texture_descriptor.format {
            TextureFormat::Rgba8UnormSrgb => true,
            TextureFormat::Rgba8Unorm => false,
            autre => {
                warn!(
                    "texture {:?} : format {autre:?} inattendu, pas de mipmaps",
                    h.path()
                );
                return false;
            }
        };
        let (l, h_px) = (image.width(), image.height());
        if image.texture_descriptor.mip_level_count == 1
            && let Some(data) = image.data.as_ref()
            && let Some((chaine, niveaux)) = mipmaps(data, l, h_px, srgb)
        {
            image.data = Some(chaine);
            image.texture_descriptor.mip_level_count = niveaux;
            a_rafraichir.push(mat.clone());
        }
        false
    });
    for mat in a_rafraichir {
        // Accès mutable = événement « modifié » : le groupe de liaison du matériau est refait.
        let _ = materials.get_mut(&mat);
    }
}

/// Chaîne de mipmaps d'une image RGBA 8 bits `largeur × hauteur` (niveau 0 compris, niveaux concaténés), moyenne
/// 2 × 2 (en lumière linéaire si `srgb`). `None` si les données ne correspondent pas à la taille.
pub fn mipmaps(rgba: &[u8], largeur: u32, hauteur: u32, srgb: bool) -> Option<(Vec<u8>, u32)> {
    let (mut l, mut h) = (largeur as usize, hauteur as usize);
    if l == 0 || h == 0 || rgba.len() != l * h * 4 {
        return None;
    }
    let vers_lin: Vec<f32> = (0..256)
        .map(|v| {
            let c = v as f32 / 255.0;
            if srgb {
                Color::srgb(c, c, c).to_linear().red
            } else {
                c
            }
        })
        .collect();
    let depuis_lin = |x: f32| -> u8 {
        let c = if srgb {
            Srgba::from(LinearRgba::rgb(x, x, x)).red
        } else {
            x
        };
        (c.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    let mut sortie = rgba.to_vec();
    let mut debut = 0;
    let mut niveaux = 1;
    while l > 1 || h > 1 {
        let (nl, nh) = ((l / 2).max(1), (h / 2).max(1));
        let mut niveau = Vec::with_capacity(nl * nh * 4);
        for y in 0..nh {
            for x in 0..nl {
                let (x0, y0) = ((2 * x).min(l - 1), (2 * y).min(h - 1));
                let (x1, y1) = ((2 * x + 1).min(l - 1), (2 * y + 1).min(h - 1));
                for c in 0..4 {
                    let px = |xx: usize, yy: usize| sortie[debut + (yy * l + xx) * 4 + c];
                    let v = [px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1)];
                    // L'alpha n'est jamais en sRGB.
                    let m = if c == 3 {
                        let s: u32 = v.iter().map(|&b| u32::from(b)).sum();
                        ((s + 2) / 4) as u8
                    } else {
                        depuis_lin(v.iter().map(|&b| vers_lin[usize::from(b)]).sum::<f32>() / 4.0)
                    };
                    niveau.push(m);
                }
            }
        }
        debut = sortie.len();
        sortie.extend_from_slice(&niveau);
        (l, h) = (nl, nh);
        niveaux += 1;
    }
    Some((sortie, niveaux))
}

#[cfg(test)]
mod tests {
    use super::mipmaps;

    #[test]
    fn chaine_complete_jusqua_un_pixel() {
        let (l, h) = (8u32, 4u32);
        let data = vec![200u8; (l * h * 4) as usize];
        let (chaine, niveaux) = mipmaps(&data, l, h, true).expect("taille cohérente");
        // 8×4, 4×2, 2×1, 1×1.
        assert_eq!(niveaux, 4);
        assert_eq!(chaine.len(), (32 + 8 + 2 + 1) * 4);
        // Une image uniforme reste uniforme (aller-retour sRGB ↔ linéaire exact à l'arrondi près).
        assert!(chaine.iter().all(|&v| v.abs_diff(200) <= 1), "{chaine:?}");
    }

    #[test]
    fn moyenne_en_lumiere_lineaire() {
        // Damier noir / blanc 2×2 : la moyenne linéaire 0,5 vaut ~188 en sRGB (et non 128).
        let n = [0, 0, 0, 255];
        let b = [255, 255, 255, 255];
        let data: Vec<u8> = [n, b, b, n].concat();
        let (chaine, _) = mipmaps(&data, 2, 2, true).expect("2×2");
        assert!((187..=189).contains(&chaine[16]), "{}", chaine[16]);
        let (chaine, _) = mipmaps(&data, 2, 2, false).expect("2×2");
        assert!((127..=128).contains(&chaine[16]), "{}", chaine[16]);
    }

    #[test]
    fn taille_incoherente_refusee() {
        assert!(mipmaps(&[0; 12], 2, 2, false).is_none());
    }
}
