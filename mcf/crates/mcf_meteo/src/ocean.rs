//! Rendu de l'océan (étapes 5a et 5b) : un maillage radial qui suit la caméra, déplacé dans le shader par la houle
//! de [`crate::houle`] et par les cascades FFT de [`crate::ocean_fft`], éclairé par le PBR de Bevy (reflets de
//! l'environnement, soleil, brouillard).
//!
//! Utilisation : ajouter [`McfOceanPlugin`] ; l'océan apparaît au niveau [`Ocean::niveau`]. Le gameplay lit la
//! hauteur de l'eau avec [`Ocean::hauteur`].

use bevy::asset::{AssetPath, RenderAssetUsages, embedded_asset, embedded_path};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use crate::MiseAJourMeteo;
use crate::etat::EtatMeteo;
use crate::houle::Ocean;
use crate::ocean_fft::{McfOceanFftPlugin, ReglagesFft, TexturesOcean};

/// Matériau de l'océan : `StandardMaterial` (couleur de l'eau profonde, réflectance) + extension.
pub type MateriauOcean = ExtendedMaterial<StandardMaterial, ExtensionOcean>;

/// Réglages du maillage et de l'aspect.
#[derive(Resource, Reflect, Debug, Clone, PartialEq)]
#[reflect(Resource, Default)]
pub struct ReglagesOcean {
    /// Quads par côté du maillage (fixé à la création).
    pub quads: u32,
    /// Rayon couvert autour de la caméra (m).
    pub rayon: f32,
    /// Part linéaire de la répartition des sommets (le reste est cubique : dense au centre).
    pub part_lineaire: f32,
    /// Intensité de l'écume.
    pub ecume: f32,
    /// Distance (m) au-delà de laquelle la plus petite cascade (rides) ne déplace plus les sommets.
    pub lod_proche: f32,
    /// Distance (m) au-delà de laquelle les cascades FFT s'effacent (il ne reste que la houle).
    pub lod_loin: f32,
}

impl Default for ReglagesOcean {
    fn default() -> Self {
        Self {
            quads: 256,
            rayon: 6000.0,
            part_lineaire: 0.02,
            ecume: 1.0,
            lod_proche: 50.0,
            lod_loin: 1500.0,
        }
    }
}

/// Uniformes de l'extension (même ordre que dans `ocean.wesl`).
#[derive(ShaderType, Debug, Clone, Copy, PartialEq, Default)]
pub struct UniformesOcean {
    vague_a: [Vec4; 4],
    vague_b: [Vec4; 4],
    niveau: f32,
    temps: f32,
    rayon: f32,
    part_lineaire: f32,
    quads: f32,
    ecume: f32,
    vent: f32,
    remplissage: f32,
    /// Taille du motif de chaque cascade (m), w = 0 si la FFT est désactivée.
    tailles: Vec4,
    lod_proche: f32,
    lod_loin: f32,
    remplissage1: f32,
    remplissage2: f32,
}

/// Extension océan du `StandardMaterial`.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct ExtensionOcean {
    /// Uniformes mis à jour chaque image.
    #[uniform(100)]
    pub uniformes: UniformesOcean,
    /// Déplacement FFT (xyz) et écume (w), une couche par cascade.
    #[texture(101, dimension = "2d_array")]
    #[sampler(102)]
    pub deplacement: Handle<Image>,
    /// Pentes et termes du jacobien FFT.
    #[texture(103, dimension = "2d_array")]
    pub derivees: Handle<Image>,
}

impl MaterialExtension for ExtensionOcean {
    fn vertex_shader() -> ShaderRef {
        chemin_shader()
    }

    fn fragment_shader() -> ShaderRef {
        chemin_shader()
    }

    // Le maillage n'a de sens qu'une fois déplacé par le vertex shader : pas de prépasse ni d'ombres portées
    // (elles utiliseraient le maillage plat).
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

fn chemin_shader() -> ShaderRef {
    ShaderRef::Path(AssetPath::from_path_buf(embedded_path!("ocean.wesl")).with_source("embedded"))
}

/// Grille `quads × quads` dans `[-1, 1]²` (plan XZ), avec normales et UV.
pub fn maillage_grille(quads: u32) -> Mesh {
    let q = quads.max(1);
    let n = q + 1;
    let mut positions = Vec::with_capacity((n * n) as usize);
    let mut uvs = Vec::with_capacity((n * n) as usize);
    for z in 0..n {
        for x in 0..n {
            let u = Vec2::new(x as f32, z as f32) / q as f32;
            positions.push([u.x * 2.0 - 1.0, 0.0, u.y * 2.0 - 1.0]);
            uvs.push([u.x, u.y]);
        }
    }
    let mut indices = Vec::with_capacity((q * q * 6) as usize);
    for z in 0..q {
        for x in 0..q {
            let i = z * n + x;
            // Sens trigonométrique vu du dessus (+Y).
            indices.extend_from_slice(&[i, i + n, i + 1, i + 1, i + n, i + n + 1]);
        }
    }
    let normales = vec![[0.0, 1.0, 0.0]; positions.len()];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normales)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// Plugin de rendu de l'océan. Nécessite [`crate::McfMeteoPlugin`] (qui fait avancer [`Ocean`]).
pub struct McfOceanPlugin;

impl Plugin for McfOceanPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "ocean.wesl");
        app.add_plugins((MaterialPlugin::<MateriauOcean>::default(), McfOceanFftPlugin))
            .register_type::<ReglagesOcean>()
            .init_resource::<ReglagesOcean>()
            .add_systems(Startup, creer_ocean)
            .add_systems(Update, mettre_a_jour_ocean.after(MiseAJourMeteo));
    }
}

/// Marque l'entité de la surface de l'océan.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct SurfaceOcean;

fn creer_ocean(
    mut commandes: Commands,
    reglages: Res<ReglagesOcean>,
    mut maillages: ResMut<Assets<Mesh>>,
    mut materiaux: ResMut<Assets<MateriauOcean>>,
    textures: Res<TexturesOcean>,
) {
    let eau = StandardMaterial {
        // Eau profonde : presque noire, c'est le reflet du ciel qui la colore.
        base_color: Color::linear_rgb(0.004, 0.018, 0.035),
        perceptual_roughness: 0.05,
        // F0 de l'eau ≈ 0,02 (réflectance 0,5 = F0 0,04).
        reflectance: 0.35,
        ..default()
    };
    commandes.spawn((
        Name::new("Océan"),
        SurfaceOcean,
        Mesh3d(maillages.add(maillage_grille(reglages.quads))),
        MeshMaterial3d(materiaux.add(MateriauOcean {
            base: eau,
            extension: ExtensionOcean {
                uniformes: UniformesOcean::default(),
                deplacement: textures.deplacement.clone(),
                derivees: textures.derivees.clone(),
            },
        })),
        Transform::IDENTITY,
        NoFrustumCulling,
        NotShadowCaster,
    ));
}

/// Uniformes de l'image (fonction pure).
pub fn calculer_uniformes(
    ocean: &Ocean,
    etat: &EtatMeteo,
    reglages: &ReglagesOcean,
    fft: &ReglagesFft,
) -> UniformesOcean {
    let (vague_a, vague_b) = ocean.uniformes_vagues();
    UniformesOcean {
        vague_a,
        vague_b,
        niveau: ocean.niveau,
        temps: ocean.temps(),
        rayon: reglages.rayon,
        part_lineaire: reglages.part_lineaire.clamp(0.001, 1.0),
        quads: reglages.quads.max(1) as f32,
        ecume: reglages.ecume,
        vent: etat.vent_vitesse,
        remplissage: 0.0,
        tailles: Vec4::new(fft.tailles[0], fft.tailles[1], fft.tailles[2], 1.0),
        lod_proche: reglages.lod_proche,
        lod_loin: reglages.lod_loin,
        remplissage1: 0.0,
        remplissage2: 0.0,
    }
}

fn mettre_a_jour_ocean(
    ocean: Res<Ocean>,
    etat: Res<EtatMeteo>,
    reglages: Res<ReglagesOcean>,
    fft: Res<ReglagesFft>,
    surfaces: Query<&MeshMaterial3d<MateriauOcean>, With<SurfaceOcean>>,
    mut materiaux: ResMut<Assets<MateriauOcean>>,
) {
    let u = calculer_uniformes(&ocean, &etat, &reglages, &fft);
    for m in &surfaces {
        if let Some(mut mat) = materiaux.get_mut(&m.0) {
            mat.extension.uniformes = u;
        }
    }
}
