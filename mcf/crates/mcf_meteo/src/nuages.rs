//! Nuages volumétriques (étape 7), portés de SkyForge : couche de nuages traversable, ray marching en demi
//! résolution sur le GPU, éclairée par le soleil de la scène en unités physiques.
//!
//! Fonctionnement :
//! 1. au démarrage, trois textures de bruit périodique sont cuites sur le GPU (forme 128³ Perlin-Worley,
//!    détail 32³ Worley, carte météo 512² couverture / type) ;
//! 2. à chaque image, pour chaque caméra portant [`NuagesVolumetriques`], une passe de calcul marche dans la
//!    couche [bas, haut] en demi résolution (pas fins près de la caméra pour voler dedans, grands pas au loin),
//!    s'arrête devant la géométrie opaque (tampon de profondeur) et écrit lumière diffusée + transmittance ;
//! 3. le résultat est composé sur l'image HDR au début de la passe transparente : après le ciel de
//!    l'atmosphère (les nuages le masquent), avant la pluie et les objets transparents (ils restent devant).
//!
//! Éclairage (unités physiques, comme le reste du rendu Bevy) : soleil = éclairement du premier
//! `DirectionalLight` (lux) × phase de Henyey-Greenstein double lobe × diffusion multiple (3 octaves,
//! Wrenninge 2013) × effet « powder », plus une ambiance du ciel proportionnelle au soleil. Le tout est
//! multiplié par l'exposition de la vue. Au loin, les nuages deviennent transparents (le ciel de
//! l'atmosphère reprend la main) au lieu de se fondre dans un ciel analytique comme dans SkyForge.

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::asset::load_embedded_asset;
use bevy::camera::Hdr;
use bevy::core_pipeline::FullscreenShader;
use bevy::core_pipeline::core_3d::{
    Transparent3d, TransparentSortingInfo3d, main_opaque_pass_3d, main_transparent_pass_3d,
};
use bevy::core_pipeline::schedule::{Core3d, camera_driver};
use bevy::ecs::query::ROQueryItem;
use bevy::ecs::system::SystemParamItem;
use bevy::ecs::system::lifetimeless::Read;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_phase::{
    AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand, RenderCommandResult,
    SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
};
use bevy::render::render_resource::binding_types::{
    sampler, texture_2d, texture_3d, texture_depth_2d, texture_storage_2d, texture_storage_3d, uniform_buffer,
};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, RenderGraph, RenderQueue, ViewQuery};
use bevy::render::sync_world::MainEntity;
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{
    ExtractedView, Msaa, ViewDepthStencilTexture, ViewUniform, ViewUniformOffset, ViewUniforms,
};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

use crate::MiseAJourMeteo;
use crate::etat::EtatMeteo;

/// Taille de la texture de forme (cellules par côté).
pub const TAILLE_FORME: u32 = 128;
/// Taille de la texture de détail.
pub const TAILLE_DETAIL: u32 = 32;
/// Taille de la carte météo des nuages.
pub const TAILLE_CARTE: u32 = 512;
/// Période du motif de la carte météo (m).
pub const PERIODE_CARTE_M: f32 = 30_000.0;
/// Le décalage des nuages par le vent est bouclé sur cette distance (m) : multiple commun des périodes de la
/// carte (30 km), de la forme (9 km) et du détail (900 m, aussi × 1,3), pour que le bouclage ne se voie pas.
pub const BOUCLE_DECALAGE_M: f32 = 90_000.0;
/// Le temps des nuages (montée des volutes) est bouclé sur cette durée (s), choisie pour que le motif de détail
/// retombe exactement sur lui-même : 2 m/s × 1,3 × 4500 s = 13 périodes de 900 m.
pub const BOUCLE_TEMPS_S: f32 = 4500.0;

/// À poser sur une `Camera3d` pour y dessiner les nuages volumétriques. Impose le rendu HDR ; le MSAA est coupé
/// sur cette caméra (la passe lit le tampon de profondeur non multi-échantillonné).
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq, ExtractComponent)]
#[extract_app(RenderApp)]
#[reflect(Component, Default)]
#[require(Hdr)]
pub struct NuagesVolumetriques {
    /// Résolution de la marche par rapport à l'écran (0,5 = demi résolution).
    pub echelle_resolution: f32,
}

impl Default for NuagesVolumetriques {
    fn default() -> Self {
        Self {
            echelle_resolution: 0.5,
        }
    }
}

/// Réglages des nuages (valeurs par défaut de SkyForge).
#[derive(Resource, Reflect, Debug, Clone, PartialEq)]
#[reflect(Resource, Default)]
pub struct ReglagesNuages {
    /// Base de la couche (m).
    pub bas: f32,
    /// Sommet de la couche (m).
    pub haut: f32,
    /// Taille du motif de forme (m).
    pub taille_forme: f32,
    /// Taille du motif de détail (m).
    pub taille_detail: f32,
    /// Érosion des bords par le détail.
    pub force_detail: f32,
    /// Coefficient d'extinction pour une densité 1 (1/m).
    pub extinction: f32,
    /// Henyey-Greenstein vers l'avant.
    pub g_avant: f32,
    /// Henyey-Greenstein vers l'arrière.
    pub g_arriere: f32,
    /// Intensité de l'effet « powder ».
    pub poudre: f32,
    /// Intensité de la lumière ambiante du ciel.
    pub ambiance: f32,
    /// Pas minimal près de la caméra (m).
    pub pas_proche: f32,
    /// Nombre de pas de la marche (base, ×3 au maximum en comptant les pas grossiers).
    pub pas: u32,
    /// Pas vers le soleil.
    pub pas_lumiere: u32,
    /// Distance maximale de la marche (m).
    pub distance_max: f32,
    /// Distance à laquelle les nuages ont disparu dans le lointain (m).
    pub distance_fondu: f32,
    /// Influence du vent sur la dérive (le vent d'altitude est ×2,5 plus fort que celui du sol).
    pub influence_vent: f32,
}

impl Default for ReglagesNuages {
    fn default() -> Self {
        Self {
            bas: 1500.0,
            haut: 4200.0,
            taille_forme: 9000.0,
            taille_detail: 900.0,
            force_detail: 0.35,
            extinction: 0.045,
            g_avant: 0.75,
            g_arriere: -0.25,
            poudre: 0.6,
            ambiance: 0.7,
            pas_proche: 6.0,
            pas: 64,
            pas_lumiere: 6,
            distance_max: 60_000.0,
            distance_fondu: 50_000.0,
            influence_vent: 1.0,
        }
    }
}

/// Dérive des nuages avec le vent et temps des volutes, tous deux bouclés (précision f32 sur des heures).
#[derive(Resource, Reflect, Debug, Clone, Copy, Default, PartialEq)]
#[reflect(Resource, Default)]
pub struct DeriveNuages {
    decalage: Vec2,
    temps: f32,
}

impl DeriveNuages {
    /// Avance de `dt` secondes avec le vent de `etat`.
    pub fn avancer(&mut self, etat: &EtatMeteo, influence_vent: f32, dt: f32) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let d = etat.direction_vent();
        let vitesse = etat.vent_vitesse.max(0.0) * 2.5 * influence_vent;
        self.decalage.x = (self.decalage.x + d.x * vitesse * dt).rem_euclid(BOUCLE_DECALAGE_M);
        self.decalage.y = (self.decalage.y + d.z * vitesse * dt).rem_euclid(BOUCLE_DECALAGE_M);
        self.temps = (self.temps + dt).rem_euclid(BOUCLE_TEMPS_S);
    }

    /// Décalage horizontal des nuages (m, plan XZ), dans `[0, BOUCLE_DECALAGE_M)`.
    pub fn decalage(&self) -> Vec2 {
        self.decalage
    }

    /// Temps des volutes (s), dans `[0, BOUCLE_TEMPS_S)`.
    pub fn temps(&self) -> f32 {
        self.temps
    }
}

/// Soleil vu par les nuages.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoleilNuages {
    /// Direction vers le soleil (normalisée).
    pub direction: Vec3,
    /// Éclairement (lux).
    pub eclairement: f32,
    /// Couleur linéaire de la lumière.
    pub couleur: Vec3,
}

impl SoleilNuages {
    /// Soleil donné par un `DirectionalLight` (la lumière part vers −Z local : le soleil est vers +Z local).
    pub fn depuis_lumiere(lumiere: &DirectionalLight, transformation: &GlobalTransform) -> Self {
        let lin = lumiere.color.to_linear();
        Self {
            direction: transformation.back().as_vec3(),
            eclairement: lumiere.illuminance.max(0.0),
            couleur: Vec3::new(lin.red, lin.green, lin.blue),
        }
    }
}

/// Paramètres envoyés aux shaders (même ordre que `ParamsNuages` dans `nuages_commun.wesl`).
#[derive(Resource, ShaderType, Clone, Copy, Debug, Default, PartialEq, ExtractResource)]
#[extract_app(RenderApp)]
pub struct ParamsNuages {
    /// Direction vers le soleil.
    pub soleil_direction: Vec3,
    /// Éclairement du soleil (lux).
    pub soleil_lux: f32,
    /// Couleur linéaire du soleil.
    pub soleil_couleur: Vec3,
    /// Intensité de l'ambiance.
    pub ambiance: f32,
    /// Décalage par le vent (m).
    pub decalage: Vec2,
    /// Temps des volutes (s).
    pub temps: f32,
    /// Couverture nuageuse (0..1).
    pub couverture: f32,
    /// Densité (multiplie l'extinction).
    pub densite: f32,
    /// Type (0 = stratus, 1 = cumulonimbus).
    pub type_nuages: f32,
    /// Précipitations (épaississent et assombrissent la base).
    pub precipitations: f32,
    /// Base de la couche (m).
    pub bas: f32,
    /// Sommet de la couche (m).
    pub haut: f32,
    /// 1 / taille du motif de forme.
    pub echelle_forme: f32,
    /// 1 / taille du motif de détail.
    pub echelle_detail: f32,
    /// Érosion des bords.
    pub force_detail: f32,
    /// Extinction (1/m).
    pub extinction: f32,
    /// Henyey-Greenstein avant.
    pub g_avant: f32,
    /// Henyey-Greenstein arrière.
    pub g_arriere: f32,
    /// Effet « powder ».
    pub poudre: f32,
    /// Pas minimal (m).
    pub pas_proche: f32,
    /// Nombre de pas.
    pub pas: u32,
    /// Pas vers le soleil.
    pub pas_lumiere: u32,
    /// Distance maximale (m).
    pub distance_max: f32,
    /// Distance de disparition (m).
    pub distance_fondu: f32,
    r0: f32,
}

/// Paramètres de l'image (fonction pure). Sans soleil, les nuages ne sont éclairés que par… rien : ils
/// apparaissent noirs et masquent le ciel, ce qui est le comportement physique d'une scène sans lumière.
pub fn calculer_params(
    etat: &EtatMeteo,
    reglages: &ReglagesNuages,
    derive: &DeriveNuages,
    soleil: Option<SoleilNuages>,
) -> ParamsNuages {
    let soleil = soleil.unwrap_or(SoleilNuages {
        direction: Vec3::Y,
        eclairement: 0.0,
        couleur: Vec3::ONE,
    });
    let bas = reglages.bas;
    let haut = reglages.haut.max(bas + 100.0);
    ParamsNuages {
        soleil_direction: soleil.direction.normalize_or(Vec3::Y),
        soleil_lux: soleil.eclairement * etat.intensite_soleil.max(0.0),
        soleil_couleur: soleil.couleur,
        ambiance: reglages.ambiance,
        decalage: derive.decalage(),
        temps: derive.temps(),
        couverture: etat.couverture_nuageuse.clamp(0.0, 1.0),
        densite: etat.densite_nuages.max(0.0),
        type_nuages: etat.type_nuages.clamp(0.0, 1.0),
        precipitations: etat.precipitations.clamp(0.0, 1.0),
        bas,
        haut,
        echelle_forme: 1.0 / reglages.taille_forme.max(1.0),
        echelle_detail: 1.0 / reglages.taille_detail.max(1.0),
        force_detail: reglages.force_detail,
        extinction: reglages.extinction.max(0.0),
        g_avant: reglages.g_avant.clamp(-0.99, 0.99),
        g_arriere: reglages.g_arriere.clamp(-0.99, 0.99),
        poudre: reglages.poudre,
        pas_proche: reglages.pas_proche.max(0.5),
        pas: reglages.pas.clamp(8, 256),
        pas_lumiere: reglages.pas_lumiere.clamp(1, 16),
        distance_max: reglages.distance_max.max(1000.0),
        distance_fondu: reglages.distance_fondu.max(1000.0),
        r0: 0.0,
    }
}

/// Taille de la texture de marche pour une vue (au moins 1 × 1).
pub fn taille_marche(vue: UVec2, echelle: f32) -> UVec2 {
    let e = if echelle.is_finite() {
        echelle.clamp(0.125, 1.0)
    } else {
        0.5
    };
    UVec2::new(
        ((vue.x as f32 * e).ceil() as u32).max(1),
        ((vue.y as f32 * e).ceil() as u32).max(1),
    )
}

/// Plugin des nuages volumétriques. Nécessite `McfMeteoPlugin` ; ajouter [`NuagesVolumetriques`] à la caméra.
pub struct McfNuagesPlugin;

impl Plugin for McfNuagesPlugin {
    fn build(&self, app: &mut App) {
        crate::charger_bruit_wesl(app);
        bevy::asset::embedded_asset!(app, "nuages_bruit.wesl");
        bevy::asset::embedded_asset!(app, "nuages_marche.wesl");
        bevy::asset::embedded_asset!(app, "nuages_composition.wesl");
        app.register_type::<NuagesVolumetriques>()
            .register_type::<ReglagesNuages>()
            .register_type::<DeriveNuages>()
            .init_resource::<ReglagesNuages>()
            .init_resource::<DeriveNuages>()
            .init_resource::<ParamsNuages>()
            .add_plugins((
                ExtractComponentPlugin::<NuagesVolumetriques>::default(),
                ExtractResourcePlugin::<ParamsNuages>::default(),
            ))
            .add_systems(
                Update,
                (preparer_cameras, mettre_a_jour_params).after(MiseAJourMeteo),
            );
    }

    fn finish(&self, app: &mut App) {
        let Some(rendu) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        rendu
            .init_resource::<TamponParamsNuages>()
            .add_render_command::<Transparent3d, DessinerNuages>()
            .add_systems(RenderStartup, creer_ressources)
            .add_systems(
                Render,
                (
                    preparer_textures.in_set(RenderSystems::PrepareResources),
                    preparer_groupes.in_set(RenderSystems::PrepareBindGroups),
                    mettre_en_file.in_set(RenderSystems::Queue),
                ),
            )
            .add_systems(RenderGraph, cuire_bruit.before(camera_driver))
            .add_systems(
                Core3d,
                marcher
                    .after(main_opaque_pass_3d)
                    .before(main_transparent_pass_3d),
            );
    }
}

/// La marche lit le tampon de profondeur : il doit être échantillonnable et non multi-échantillonné.
fn preparer_cameras(mut cameras: Query<(&mut Camera3d, &mut Msaa), With<NuagesVolumetriques>>) {
    for (mut camera, mut msaa) in &mut cameras {
        let usages = TextureUsages::from(camera.depth_texture_usages);
        if !usages.contains(TextureUsages::TEXTURE_BINDING) {
            camera.depth_texture_usages = (usages | TextureUsages::TEXTURE_BINDING).into();
        }
        if *msaa != Msaa::Off {
            *msaa = Msaa::Off;
        }
    }
}

fn mettre_a_jour_params(
    etat: Res<EtatMeteo>,
    reglages: Res<ReglagesNuages>,
    temps: Res<Time>,
    lumieres: Query<(&DirectionalLight, &GlobalTransform)>,
    mut derive: ResMut<DeriveNuages>,
    mut params: ResMut<ParamsNuages>,
) {
    derive.avancer(&etat, reglages.influence_vent, temps.delta_secs());
    let soleil = lumieres
        .iter()
        .max_by(|a, b| a.0.illuminance.total_cmp(&b.0.illuminance))
        .map(|(l, t)| SoleilNuages::depuis_lumiere(l, t));
    *params = calculer_params(&etat, &reglages, &derive, soleil);
}

// ---------------------------------------------------------------------------------------------------------------
// Monde du rendu

#[derive(Resource)]
struct RessourcesNuages {
    disposition_bruit: BindGroupLayoutDescriptor,
    disposition_marche: BindGroupLayoutDescriptor,
    disposition_composition: BindGroupLayoutDescriptor,
    cuire_forme: CachedComputePipelineId,
    cuire_detail: CachedComputePipelineId,
    cuire_carte: CachedComputePipelineId,
    marche: CachedComputePipelineId,
    composition: CachedRenderPipelineId,
    forme: TextureView,
    detail: TextureView,
    carte: TextureView,
    echantillonneur_bruit: Sampler,
    echantillonneur_nuages: Sampler,
    groupe_bruit: Option<BindGroup>,
    cuit: AtomicBool,
}

#[derive(Resource, Default)]
struct TamponParamsNuages(UniformBuffer<ParamsNuages>);

#[derive(Component)]
struct TextureNuages(CachedTexture);

#[derive(Component)]
struct GroupesNuages {
    marche: BindGroup,
    composition: BindGroup,
    taille: UVec2,
}

fn texture_bruit(
    peripherique: &RenderDevice,
    nom: &'static str,
    taille: Extent3d,
    dimension: TextureDimension,
) -> TextureView {
    let texture = peripherique.create_texture(&TextureDescriptor {
        label: Some(nom),
        size: taille,
        mip_level_count: 1,
        sample_count: 1,
        dimension,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    texture.create_view(&TextureViewDescriptor::default())
}

fn creer_ressources(
    mut commandes: Commands,
    serveur: Res<AssetServer>,
    cache: Res<PipelineCache>,
    peripherique: Res<RenderDevice>,
    plein_ecran: Res<FullscreenShader>,
) {
    let ecriture3 = texture_storage_3d(TextureFormat::Rgba8Unorm, StorageTextureAccess::WriteOnly);
    let disposition_bruit = BindGroupLayoutDescriptor::new(
        "nuages_bruit",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                ecriture3,
                ecriture3,
                texture_storage_2d(TextureFormat::Rgba8Unorm, StorageTextureAccess::WriteOnly),
            ),
        ),
    );
    let filtrable = TextureSampleType::Float { filterable: true };
    let disposition_marche = BindGroupLayoutDescriptor::new(
        "nuages_marche",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer::<ParamsNuages>(false),
                texture_depth_2d(),
                texture_3d(filtrable),
                texture_3d(filtrable),
                texture_2d(filtrable),
                sampler(SamplerBindingType::Filtering),
                texture_storage_2d(TextureFormat::Rgba16Float, StorageTextureAccess::WriteOnly),
            ),
        ),
    );
    let disposition_composition = BindGroupLayoutDescriptor::new(
        "nuages_composition",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (texture_2d(filtrable), sampler(SamplerBindingType::Filtering)),
        ),
    );

    let bruit = load_embedded_asset!(serveur.as_ref(), "nuages_bruit.wesl");
    let marche = load_embedded_asset!(serveur.as_ref(), "nuages_marche.wesl");
    let composition = load_embedded_asset!(serveur.as_ref(), "nuages_composition.wesl");
    let calcul = |disposition: &BindGroupLayoutDescriptor, shader: &Handle<Shader>, entree: &'static str| {
        cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(Cow::Borrowed(entree)),
            layout: vec![disposition.clone()],
            shader: shader.clone(),
            entry_point: Some(Cow::Borrowed(entree)),
            ..default()
        })
    };
    // Composition : couleur = nuages + transmittance × image (prémultiplié, la transmittance est dans alpha) ;
    // l'alpha de l'image est conservé.
    let melange = BlendState {
        color: BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::SrcAlpha,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent {
            src_factor: BlendFactor::Zero,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        },
    };
    let pipeline_composition = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("nuages_composition".into()),
        layout: vec![disposition_composition.clone()],
        vertex: plein_ecran.to_vertex_state(),
        primitive: PrimitiveState {
            cull_mode: None,
            ..default()
        },
        // La passe transparente a un tampon de profondeur : on le déclare sans le tester ni l'écrire
        // (la marche s'est déjà arrêtée devant la géométrie).
        depth_stencil: Some(DepthStencilState {
            format: TextureFormat::Depth32Float,
            depth_write_enabled: Some(false),
            depth_compare: Some(CompareFunction::Always),
            stencil: default(),
            bias: default(),
        }),
        multisample: MultisampleState::default(),
        fragment: Some(FragmentState {
            shader: composition,
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba16Float,
                blend: Some(melange),
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });

    let cube = |n| Extent3d {
        width: n,
        height: n,
        depth_or_array_layers: n,
    };
    let repetition = |filtre| {
        peripherique.create_sampler(&SamplerDescriptor {
            label: Some("nuages"),
            address_mode_u: AddressMode::Repeat,
            address_mode_v: AddressMode::Repeat,
            address_mode_w: AddressMode::Repeat,
            mag_filter: filtre,
            min_filter: filtre,
            ..default()
        })
    };
    let bords = peripherique.create_sampler(&SamplerDescriptor {
        label: Some("nuages_composition"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    commandes.insert_resource(RessourcesNuages {
        cuire_forme: calcul(&disposition_bruit, &bruit, "cuire_forme"),
        cuire_detail: calcul(&disposition_bruit, &bruit, "cuire_detail"),
        cuire_carte: calcul(&disposition_bruit, &bruit, "cuire_carte"),
        marche: calcul(&disposition_marche, &marche, "marcher"),
        composition: pipeline_composition,
        forme: texture_bruit(
            &peripherique,
            "nuages_forme",
            cube(TAILLE_FORME),
            TextureDimension::D3,
        ),
        detail: texture_bruit(
            &peripherique,
            "nuages_detail",
            cube(TAILLE_DETAIL),
            TextureDimension::D3,
        ),
        carte: texture_bruit(
            &peripherique,
            "nuages_carte",
            Extent3d {
                width: TAILLE_CARTE,
                height: TAILLE_CARTE,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
        ),
        echantillonneur_bruit: repetition(FilterMode::Linear),
        echantillonneur_nuages: bords,
        disposition_bruit,
        disposition_marche,
        disposition_composition,
        groupe_bruit: None,
        cuit: AtomicBool::new(false),
    });
}

fn preparer_textures(
    mut commandes: Commands,
    mut cache_textures: ResMut<TextureCache>,
    peripherique: Res<RenderDevice>,
    vues: Query<(Entity, &ExtractedCamera, &NuagesVolumetriques), With<ExtractedView>>,
) {
    for (entite, camera, nuages) in &vues {
        let Some(taille_vue) = camera.physical_viewport_size else {
            continue;
        };
        let taille = taille_marche(taille_vue, nuages.echelle_resolution);
        let texture = cache_textures.get(
            &peripherique,
            TextureDescriptor {
                label: Some("nuages_marche"),
                size: Extent3d {
                    width: taille.x,
                    height: taille.y,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba16Float,
                usage: TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        );
        commandes.entity(entite).insert(TextureNuages(texture));
    }
}

#[allow(clippy::too_many_arguments)]
fn preparer_groupes(
    mut commandes: Commands,
    mut ressources: ResMut<RessourcesNuages>,
    cache: Res<PipelineCache>,
    peripherique: Res<RenderDevice>,
    file: Res<RenderQueue>,
    uniformes_vues: Res<ViewUniforms>,
    params: Option<Res<ParamsNuages>>,
    mut tampon: ResMut<TamponParamsNuages>,
    vues: Query<(Entity, &TextureNuages, &ViewDepthStencilTexture)>,
) {
    if ressources.groupe_bruit.is_none() {
        let groupe = peripherique.create_bind_group(
            "nuages_bruit",
            &cache.get_bind_group_layout(&ressources.disposition_bruit),
            &BindGroupEntries::sequential((&ressources.forme, &ressources.detail, &ressources.carte)),
        );
        ressources.groupe_bruit = Some(groupe);
    }
    let Some(params) = params else {
        return;
    };
    tampon.0.set(*params);
    tampon.0.write_buffer(&peripherique, &file);
    let (Some(liaison_vue), Some(liaison_params)) = (uniformes_vues.uniforms.binding(), tampon.0.binding())
    else {
        return;
    };
    let disposition_marche = cache.get_bind_group_layout(&ressources.disposition_marche);
    let disposition_composition = cache.get_bind_group_layout(&ressources.disposition_composition);
    for (entite, texture, profondeur) in &vues {
        let Some(vue_profondeur) = profondeur.attachment.depth_stencil_views().depth_only_view() else {
            continue;
        };
        let marche = peripherique.create_bind_group(
            "nuages_marche",
            &disposition_marche,
            &BindGroupEntries::sequential((
                liaison_vue.clone(),
                liaison_params.clone(),
                vue_profondeur,
                &ressources.forme,
                &ressources.detail,
                &ressources.carte,
                &ressources.echantillonneur_bruit,
                &texture.0.default_view,
            )),
        );
        let composition = peripherique.create_bind_group(
            "nuages_composition",
            &disposition_composition,
            &BindGroupEntries::sequential((&texture.0.default_view, &ressources.echantillonneur_nuages)),
        );
        let taille = texture.0.texture.size();
        commandes.entity(entite).insert(GroupesNuages {
            marche,
            composition,
            taille: UVec2::new(taille.width, taille.height),
        });
    }
}

fn mettre_en_file(
    fonctions: Res<DrawFunctions<Transparent3d>>,
    ressources: Res<RessourcesNuages>,
    cache: Res<PipelineCache>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    vues: Query<(Entity, &MainEntity, &ExtractedView), With<NuagesVolumetriques>>,
) {
    let Some(fonction) = fonctions.read().get_id::<DessinerNuages>() else {
        return;
    };
    // Rien à composer tant que le bruit n'est pas cuit et que la marche ne peut pas tourner (la texture de
    // marche serait vide, donc noire et opaque).
    if !ressources.cuit.load(Ordering::Relaxed) || cache.get_compute_pipeline(ressources.marche).is_none() {
        return;
    }
    for (entite, entite_principale, vue) in &vues {
        let Some(phase) = phases.get_mut(&vue.retained_view_entity) else {
            continue;
        };
        if vue.target_format != TextureFormat::Rgba16Float {
            continue;
        }
        // Dessiné en premier dans la passe transparente : derrière la pluie et les objets transparents.
        phase.add_transient(Transparent3d {
            pipeline: ressources.composition,
            entity: (entite, *entite_principale),
            draw_function: fonction,
            distance: f32::NEG_INFINITY,
            batch_range: 0..1,
            extra_index: PhaseItemExtraIndex::None,
            indexed: false,
            sorting_info: TransparentSortingInfo3d::AlwaysOnTop,
        });
    }
}

struct CommandeNuages;

impl<P: PhaseItem> RenderCommand<P> for CommandeNuages {
    type Param = ();
    type ViewQuery = Option<Read<GroupesNuages>>;
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        groupes: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _: SystemParamItem<'w, '_, Self::Param>,
        passe: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(groupes) = groupes else {
            return RenderCommandResult::Skip;
        };
        passe.set_bind_group(0, &groupes.composition, &[]);
        passe.draw(0..3, 0..1);
        RenderCommandResult::Success
    }
}

type DessinerNuages = (SetItemPipeline, CommandeNuages);

fn cuire_bruit(
    mut contexte: RenderContext,
    ressources: Option<Res<RessourcesNuages>>,
    cache: Res<PipelineCache>,
) {
    let Some(ressources) = ressources else {
        return;
    };
    if ressources.cuit.load(Ordering::Relaxed) {
        return;
    }
    let (Some(groupe), Some(forme), Some(detail), Some(carte)) = (
        &ressources.groupe_bruit,
        cache.get_compute_pipeline(ressources.cuire_forme),
        cache.get_compute_pipeline(ressources.cuire_detail),
        cache.get_compute_pipeline(ressources.cuire_carte),
    ) else {
        return;
    };
    let diagnostics = contexte.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let mut passe = contexte
        .command_encoder()
        .begin_compute_pass(&ComputePassDescriptor {
            label: Some("nuages_bruit"),
            ..default()
        });
    let span = diagnostics.pass_span(&mut passe, "nuages_bruit");
    passe.set_bind_group(0, groupe, &[]);
    passe.set_pipeline(forme);
    passe.dispatch_workgroups(TAILLE_FORME / 4, TAILLE_FORME / 4, TAILLE_FORME / 4);
    passe.set_pipeline(detail);
    passe.dispatch_workgroups(TAILLE_DETAIL / 4, TAILLE_DETAIL / 4, TAILLE_DETAIL / 4);
    passe.set_pipeline(carte);
    passe.dispatch_workgroups(TAILLE_CARTE / 8, TAILLE_CARTE / 8, 1);
    span.end(&mut passe);
    ressources.cuit.store(true, Ordering::Relaxed);
}

fn marcher(
    vue: ViewQuery<(&ViewUniformOffset, &GroupesNuages)>,
    ressources: Option<Res<RessourcesNuages>>,
    cache: Res<PipelineCache>,
    mut contexte: RenderContext,
) {
    let Some(ressources) = ressources else {
        return;
    };
    if !ressources.cuit.load(Ordering::Relaxed) {
        return;
    }
    let Some(pipeline) = cache.get_compute_pipeline(ressources.marche) else {
        return;
    };
    let (decalage_vue, groupes) = vue.into_inner();
    let diagnostics = contexte.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let mut passe = contexte
        .command_encoder()
        .begin_compute_pass(&ComputePassDescriptor {
            label: Some("nuages_marche"),
            ..default()
        });
    let span = diagnostics.pass_span(&mut passe, "nuages_marche");
    passe.set_pipeline(pipeline);
    passe.set_bind_group(0, &groupes.marche, &[decalage_vue.offset]);
    passe.dispatch_workgroups(groupes.taille.x.div_ceil(8), groupes.taille.y.div_ceil(8), 1);
    span.end(&mut passe);
}
