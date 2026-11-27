//! Carte des abris : hauteur du plus haut toit au-dessus de chaque case, autour de la caméra. La pluie et la neige
//! ne descendent pas sous cette hauteur (pas de pluie dans les maisons, sous les ponts, dans les voitures).
//!
//! Équivalent de la carte d'occlusion vue du ciel de SkyForge, mais construite sur le CPU à partir des boîtes
//! englobantes des entités marquées [`AbriPrecipitations`] : c'est opt-in, pour qu'un grand terrain ou un arbre
//! ne bloque pas la pluie sur toute sa boîte. Mise à jour seulement quand la caméra change de case ou qu'un abri
//! bouge.

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Hauteur écrite dans les cases sans abri (bien plus bas que n'importe quel sol).
pub const SANS_ABRI: f32 = -1.0e6;

/// Marque une entité (avec un maillage) qui abrite de la pluie et de la neige : toit, pont, auvent, véhicule…
#[derive(Component, Reflect, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[reflect(Component, Default)]
pub struct AbriPrecipitations;

/// Grille de hauteurs d'abri centrée sur la caméra.
#[derive(Debug, Clone, PartialEq)]
pub struct GrilleAbris {
    /// Nombre de cases par côté.
    pub taille: u32,
    /// Côté d'une case (m).
    pub texel: f32,
    /// Coin −X −Z de la grille (m), aligné sur les cases.
    pub origine: Vec2,
    /// Hauteurs, ligne par ligne (Z), [`SANS_ABRI`] là où il n'y a rien.
    pub hauteurs: Vec<f32>,
}

impl GrilleAbris {
    /// Grille vide.
    pub fn nouvelle(taille: u32, texel: f32) -> Self {
        let taille = taille.max(1);
        Self {
            taille,
            texel: texel.max(0.01),
            origine: Vec2::ZERO,
            hauteurs: vec![SANS_ABRI; (taille * taille) as usize],
        }
    }

    /// Origine alignée sur les cases pour une grille centrée sur `centre` (x, z).
    pub fn origine_pour(&self, centre: Vec2) -> Vec2 {
        let demi = self.taille as f32 * self.texel * 0.5;
        ((centre - Vec2::splat(demi)) / self.texel).floor() * self.texel
    }

    /// Remplit la grille à partir de boîtes englobantes monde `(min, max)`.
    pub fn remplir(&mut self, origine: Vec2, boites: impl IntoIterator<Item = (Vec3, Vec3)>) {
        self.origine = origine;
        self.hauteurs.fill(SANS_ABRI);
        let n = self.taille as i32;
        for (min, max) in boites {
            if !(min.is_finite() && max.is_finite()) {
                continue;
            }
            let a = ((Vec2::new(min.x, min.z) - origine) / self.texel)
                .floor()
                .as_ivec2();
            let b = ((Vec2::new(max.x, max.z) - origine) / self.texel)
                .floor()
                .as_ivec2();
            let (x0, x1) = (a.x.max(0), b.x.min(n - 1));
            let (z0, z1) = (a.y.max(0), b.y.min(n - 1));
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let h = &mut self.hauteurs[(z * n + x) as usize];
                    *h = h.max(max.y);
                }
            }
        }
    }

    /// Hauteur d'abri au point (x, z), [`SANS_ABRI`] hors de la grille.
    pub fn hauteur(&self, x: f32, z: f32) -> f32 {
        let c = ((Vec2::new(x, z) - self.origine) / self.texel).floor().as_ivec2();
        let n = self.taille as i32;
        if c.x < 0 || c.y < 0 || c.x >= n || c.y >= n {
            return SANS_ABRI;
        }
        self.hauteurs[(c.y * n + c.x) as usize]
    }

    /// Image R32Float pour le GPU.
    pub fn image(&self) -> Image {
        let mut img = Image::new(
            Extent3d {
                width: self.taille,
                height: self.taille,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            self.octets(),
            TextureFormat::R32Float,
            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
        );
        img.sampler = bevy::image::ImageSampler::nearest();
        img
    }

    fn octets(&self) -> Vec<u8> {
        self.hauteurs.iter().flat_map(|h| h.to_le_bytes()).collect()
    }
}

/// Boîte englobante monde d'une boîte locale transformée.
pub fn boite_monde(aabb: &Aabb, transform: &GlobalTransform) -> (Vec3, Vec3) {
    let c = Vec3::from(aabb.center);
    let e = Vec3::from(aabb.half_extents);
    let m = transform.affine();
    let centre = m.transform_point3(c);
    // Demi-étendue monde = |matrice| × demi-étendue locale.
    let mat = m.matrix3;
    let etendue = Vec3::new(
        Vec3::from(mat.row(0)).abs().dot(e),
        Vec3::from(mat.row(1)).abs().dot(e),
        Vec3::from(mat.row(2)).abs().dot(e),
    );
    (centre - etendue, centre + etendue)
}

/// Ressource : la grille et son image GPU.
#[derive(Resource, Debug, Clone)]
pub struct CarteAbris {
    /// Grille CPU (consultable par le gameplay : « suis-je à l'abri ? »).
    pub grille: GrilleAbris,
    /// Image envoyée aux shaders.
    pub image: Handle<Image>,
}

impl CarteAbris {
    /// Vrai si le point est sous un abri.
    pub fn a_l_abri(&self, p: Vec3) -> bool {
        p.y < self.grille.hauteur(p.x, p.z)
    }
}

/// Crée la carte (appelé à la fin de la construction du plugin, pour qu'elle existe dès le premier `Startup`).
pub(crate) fn creer_carte_abris(monde: &mut World) {
    if monde.contains_resource::<CarteAbris>() {
        return;
    }
    // 128 cases de 0,75 m = 96 m : couvre la boîte de pluie (70 m) avec de la marge.
    let grille = GrilleAbris::nouvelle(128, 0.75);
    let image = monde.resource_mut::<Assets<Image>>().add(grille.image());
    monde.insert_resource(CarteAbris { grille, image });
}

pub(crate) fn mettre_a_jour_carte_abris(
    carte: Option<ResMut<CarteAbris>>,
    mut images: ResMut<Assets<Image>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    abris: Query<(&Aabb, &GlobalTransform), With<AbriPrecipitations>>,
    changes: Query<
        (),
        (
            With<AbriPrecipitations>,
            Or<(Changed<GlobalTransform>, Changed<Aabb>)>,
        ),
    >,
    mut retires: RemovedComponents<AbriPrecipitations>,
) {
    let Some(mut carte) = carte else {
        return;
    };
    let centre = cameras.iter().next().map_or(Vec3::ZERO, |c| c.translation());
    let origine = carte.grille.origine_pour(Vec2::new(centre.x, centre.z));
    let retrait = retires.read().count() > 0;
    if origine == carte.grille.origine && changes.is_empty() && !retrait {
        return;
    }
    carte
        .grille
        .remplir(origine, abris.iter().map(|(a, t)| boite_monde(a, t)));
    let octets = carte.grille.octets();
    if let Some(mut img) = images.get_mut(&carte.image) {
        img.data = Some(octets);
    }
}
