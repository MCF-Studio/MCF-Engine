//! Rendu de l'océan (étapes 5a et 5b) : un maillage radial qui suit la caméra, déplacé dans le shader par la houle
//! de [`crate::houle`] et par les cascades FFT de [`crate::ocean_fft`], éclairé par le PBR de Bevy (reflets de
//! l'environnement, soleil, brouillard).
//!
//! Utilisation : ajouter [`McfOceanPlugin`] ; l'océan apparaît au niveau [`Ocean::niveau`]. Le gameplay lit la
//! hauteur de l'eau avec [`Ocean::hauteur`], ou [`crate::rivage::hauteur_eau`] près des côtes.
//!
//! Vagues de bord (étape 6) : si le jeu fournit une [`SourceFond`], la grille du rivage suit la caméra et est
//! envoyée au shader (fond, distance signée au rivage, normale) ; la houle s'atténue en eau peu profonde, les
//! vagues de bord déferlent, l'eau laisse voir le fond près du bord et l'écume borde le rivage.

use bevy::asset::{AssetPath, RenderAssetUsages, embedded_asset, embedded_path};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;

use crate::MiseAJourMeteo;
use crate::etat::EtatMeteo;
use crate::houle::Ocean;
use crate::ocean_fft::{McfOceanFftPlugin, ReglagesFft, TexturesOcean};
use crate::rivage::{CarteRivage, ParametresRivage, SourceFond, amplitude_houle, params_bord};

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
    /// Couleur du fond vu à travers l'eau peu profonde (sable clair par défaut).
    pub couleur_fond: Color,
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
            couleur_fond: Color::linear_rgb(0.70, 0.62, 0.45),
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
    /// Amplitude crête totale de la houle (m), pour la limiter en eau peu profonde.
    amplitude_houle: f32,
    /// Taille du motif de chaque cascade (m), w = 0 si la FFT est désactivée.
    tailles: Vec4,
    lod_proche: f32,
    lod_loin: f32,
    remplissage1: f32,
    remplissage2: f32,
    /// Grille du rivage : origine x, origine z, côté (m), 1 si active.
    rivage: Vec4,
    /// Vagues de bord : amplitude (m), longueur d'onde (m), jet de rive (m), pulsation (rad/s).
    bord: Vec4,
    /// Couleur linéaire du fond (rgb).
    couleur_fond: Vec4,
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
    /// Grille du rivage : (fond, distance signée, normale x, normale z), lue sans filtrage.
    #[texture(104, sample_type = "float", filterable = false)]
    pub rivage: Handle<Image>,
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
        crate::charger_bruit_wesl(app);
        app.add_plugins((MaterialPlugin::<MateriauOcean>::default(), McfOceanFftPlugin))
            .register_type::<ReglagesOcean>()
            .init_resource::<ReglagesOcean>()
            .add_systems(Startup, creer_ocean)
            .add_systems(
                Update,
                (mettre_a_jour_rivage, mettre_a_jour_ocean)
                    .chain()
                    .after(MiseAJourMeteo),
            );
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
    mut images: ResMut<Assets<Image>>,
) {
    let rivage = images.add(image_rivage(1, vec![[0.0, -LOIN, 0.0, 0.0]]));
    commandes.insert_resource(TextureRivage(rivage.clone()));
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
                rivage,
            },
        })),
        Transform::IDENTITY,
        NoFrustumCulling,
        NotShadowCaster,
    ));
}

const LOIN: f32 = crate::rivage::LOIN_DU_RIVAGE;

/// Texture de la grille du rivage (une case pour « pas de rivage »).
#[derive(Resource, Debug, Clone)]
pub struct TextureRivage(pub Handle<Image>);

fn image_rivage(n: u32, donnees: Vec<[f32; 4]>) -> Image {
    let octets: Vec<u8> = donnees
        .iter()
        .flat_map(|c| c.iter().flat_map(|v| v.to_le_bytes()))
        .collect();
    Image::new(
        Extent3d {
            width: n,
            height: n,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        octets,
        TextureFormat::Rgba32Float,
        RenderAssetUsages::default(),
    )
}

/// Suit la caméra (la première `Camera3d`) et renvoie la grille au GPU quand elle est reconstruite.
fn mettre_a_jour_rivage(
    source: Option<Res<SourceFond>>,
    parametres: Res<ParametresRivage>,
    ocean: Res<Ocean>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    texture: Option<Res<TextureRivage>>,
    mut carte: ResMut<CarteRivage>,
    mut images: ResMut<Assets<Image>>,
) {
    let centre = cameras
        .iter()
        .next()
        .map(|t| t.translation().xz())
        .unwrap_or(Vec2::ZERO);
    let forcer = source.as_ref().is_some_and(|s| s.is_changed()) || parametres.is_changed();
    if !carte.mettre_a_jour(source.as_deref(), centre, ocean.niveau, &parametres, forcer) {
        return;
    }
    let Some(texture) = texture else {
        return;
    };
    let image = match carte.grille() {
        Some(g) => image_rivage(g.n(), g.donnees_gpu()),
        None => image_rivage(1, vec![[0.0, -LOIN, 0.0, 0.0]]),
    };
    if let Some(mut i) = images.get_mut(&texture.0) {
        *i = image;
    }
}

/// Uniformes de l'image (fonction pure).
pub fn calculer_uniformes(
    ocean: &Ocean,
    etat: &EtatMeteo,
    reglages: &ReglagesOcean,
    fft: &ReglagesFft,
    carte: &CarteRivage,
    rivage: &ParametresRivage,
) -> UniformesOcean {
    let grille = carte
        .grille()
        .map(|g| Vec4::new(g.origine().x, g.origine().y, g.taille_m(), 1.0))
        .unwrap_or(Vec4::ZERO);
    let b = params_bord(etat, rivage);
    let fond = reglages.couleur_fond.to_linear();
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
        amplitude_houle: amplitude_houle(ocean),
        tailles: Vec4::new(fft.tailles[0], fft.tailles[1], fft.tailles[2], 1.0),
        lod_proche: reglages.lod_proche,
        lod_loin: reglages.lod_loin,
        remplissage1: 0.0,
        remplissage2: 0.0,
        rivage: grille,
        bord: Vec4::new(b.amplitude, b.longueur, b.jet, b.omega),
        couleur_fond: Vec4::new(fond.red, fond.green, fond.blue, 1.0),
    }
}

fn mettre_a_jour_ocean(
    ocean: Res<Ocean>,
    etat: Res<EtatMeteo>,
    reglages: Res<ReglagesOcean>,
    fft: Res<ReglagesFft>,
    carte: Res<CarteRivage>,
    rivage: Res<ParametresRivage>,
    surfaces: Query<&MeshMaterial3d<MateriauOcean>, With<SurfaceOcean>>,
    mut materiaux: ResMut<Assets<MateriauOcean>>,
) {
    let u = calculer_uniformes(&ocean, &etat, &reglages, &fft, &carte, &rivage);
    for m in &surfaces {
        if let Some(mut mat) = materiaux.get_mut(&m.0) {
            mat.extension.uniformes = u;
        }
    }
}
