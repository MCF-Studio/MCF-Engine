//! Nuages volumétriques (étape 7), portés de SkyForge : couche de nuages traversable, ray marching en demi
//! résolution sur le GPU, éclairée par le soleil de la scène en unités physiques.
//!
//! Fonctionnement :
//! 1. au démarrage, trois textures de bruit périodique sont cuites sur le GPU (forme 128³ Perlin-Worley,
//!    détail 32³ Worley, carte météo 512² couverture / type) ;
//! 2. à chaque image, pour chaque caméra portant [`NuagesVolumetriques`], une passe de calcul marche dans la
//!    couche [bas, haut] en demi résolution (pas fins près de la caméra pour voler dedans, grands pas au loin),
//!    s'arrête devant la géométrie opaque (tampon de profondeur) et écrit lumière diffusée + transmittance ;
//!    avec la reprojection temporelle (étape 14, `NuagesVolumetriques::reprojection`), elle ne marche qu'un
//!    pixel par bloc 2 × 2 à chaque image (ordre de Bayer, 4 images pour tout couvrir) ; une seconde passe
//!    reconstruit les autres depuis l'image précédente (position monde = rayon × distance moyenne du nuage,
//!    reprojetée par l'ancienne matrice), bornée au min / max des blocs frais voisins pour éviter les traînées ;
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
    sampler, texture_2d, texture_3d, texture_depth_2d, texture_depth_2d_multisampled, texture_storage_2d,
    texture_storage_2d_array, texture_storage_3d, uniform_buffer,
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

/// À poser sur une `Camera3d` pour y dessiner les nuages volumétriques. Impose le rendu HDR ; le MSAA de la caméra
/// est respecté (la marche lit la profondeur multi-échantillonnée si besoin).
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq, ExtractComponent)]
#[extract_app(RenderApp)]
#[reflect(Component, Default)]
#[require(Hdr)]
pub struct NuagesVolumetriques {
    /// Résolution de la marche par rapport à l'écran (0,5 = demi résolution).
    pub echelle_resolution: f32,
    /// Reprojection temporelle : un pixel sur 4 est marché à chaque image, les autres sont repris de l'image
    /// précédente (voir `nuages_reconstruction.wesl`).
    pub reprojection: bool,
    /// Nuages dans les reflets et la lumière ambiante (carte d'environnement de l'atmosphère, voir
    /// `ciel_nuages`).
    pub ciel: bool,
}

impl Default for NuagesVolumetriques {
    fn default() -> Self {
        Self {
            echelle_resolution: 0.5,
            reprojection: true,
            ciel: true,
        }
    }
}

/// Pixel marché dans chaque bloc 2 × 2 selon le numéro d'image (ordre de Bayer : les 4 pixels en 4 images, en
/// alternant les diagonales).
pub fn decalage_reprojection(image: u32) -> UVec2 {
    const ORDRE: [UVec2; 4] = [
        UVec2::new(0, 0),
        UVec2::new(1, 1),
        UVec2::new(1, 0),
        UVec2::new(0, 1),
    ];
    ORDRE[(image % 4) as usize]
}

/// Paramètres de la reprojection d'une vue (même ordre que `ParamsReprojection` dans les shaders).
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
pub struct ParamsReprojection {
    /// `clip_from_world` de l'image précédente.
    pub precedente: Mat4,
    /// Taille de la texture complète (demi résolution de l'écran).
    pub taille_cible: UVec2,
    /// Pixel marché dans chaque bloc à cette image.
    pub decalage: UVec2,
    /// Côté des blocs : 2 avec reprojection, 1 sans.
    pub pas: u32,
    /// 0 si l'image précédente n'est pas utilisable (première image, taille changée).
    pub valide: u32,
    /// Face du cube de ciel marchée à cette image (0 à 5).
    pub face_ciel: u32,
    r1: u32,
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
        bevy::asset::embedded_asset!(app, "nuages_reconstruction.wesl");
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
        crate::ombre_nuages::construire(app);
        crate::ciel_nuages::construire(app);
    }

    fn finish(&self, app: &mut App) {
        crate::ombre_nuages::finir(app);
        crate::ciel_nuages::finir(app);
        let Some(rendu) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        rendu
            .init_resource::<TamponParamsNuages>()
            .init_resource::<HistoriquesNuages>()
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

/// La marche lit le tampon de profondeur : il doit être échantillonnable (multi-échantillonné ou non : les
/// pipelines existent pour chaque niveau de MSAA, voir `VarianteNuages`).
fn preparer_cameras(mut cameras: Query<&mut Camera3d, With<NuagesVolumetriques>>) {
    for mut camera in &mut cameras {
        let usages = TextureUsages::from(camera.depth_texture_usages);
        if !usages.contains(TextureUsages::TEXTURE_BINDING) {
            camera.depth_texture_usages = (usages | TextureUsages::TEXTURE_BINDING).into();
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
pub(crate) struct RessourcesNuages {
    disposition_bruit: BindGroupLayoutDescriptor,
    disposition_composition: BindGroupLayoutDescriptor,
    disposition_reconstruction: BindGroupLayoutDescriptor,
    reconstruction: CachedComputePipelineId,
    cuire_forme: CachedComputePipelineId,
    cuire_detail: CachedComputePipelineId,
    cuire_carte: CachedComputePipelineId,
    /// Marche et composition pour chaque nombre d'échantillons MSAA rencontré (créées à la demande).
    variantes: bevy::platform::collections::HashMap<u32, VarianteNuages>,
    shader_marche: Handle<Shader>,
    shader_composition: Handle<Shader>,
    sommet_plein_ecran: VertexState,
    pub(crate) forme: TextureView,
    detail: TextureView,
    pub(crate) carte: TextureView,
    pub(crate) echantillonneur_bruit: Sampler,
    echantillonneur_nuages: Sampler,
    groupe_bruit: Option<BindGroup>,
    pub(crate) cuit: AtomicBool,
}

/// Pipelines qui dépendent du MSAA de la vue : la marche lit la profondeur (multi-échantillonnée ou non) et la
/// composition dessine dans la cible de la passe transparente (même nombre d'échantillons).
#[derive(Clone)]
struct VarianteNuages {
    disposition_marche: BindGroupLayoutDescriptor,
    marche: CachedComputePipelineId,
    marche_ciel: CachedComputePipelineId,
    composition: CachedRenderPipelineId,
}

impl RessourcesNuages {
    fn variante(&mut self, cache: &PipelineCache, echantillons: u32) -> &VarianteNuages {
        let disposition_composition = self.disposition_composition.clone();
        let (shader_marche, shader_composition, sommet) = (
            self.shader_marche.clone(),
            self.shader_composition.clone(),
            self.sommet_plein_ecran.clone(),
        );
        self.variantes.entry(echantillons).or_insert_with(|| {
            creer_variante(
                cache,
                echantillons,
                shader_marche,
                shader_composition,
                sommet,
                disposition_composition,
            )
        })
    }
}

fn creer_variante(
    cache: &PipelineCache,
    echantillons: u32,
    shader_marche: Handle<Shader>,
    shader_composition: Handle<Shader>,
    sommet: VertexState,
    disposition_composition: BindGroupLayoutDescriptor,
) -> VarianteNuages {
    let filtrable = TextureSampleType::Float { filterable: true };
    let multi = echantillons > 1;
    let profondeur = if multi {
        texture_depth_2d_multisampled()
    } else {
        texture_depth_2d()
    };
    let disposition_marche = BindGroupLayoutDescriptor::new(
        if multi {
            "nuages_marche_msaa"
        } else {
            "nuages_marche"
        },
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer::<ParamsNuages>(false),
                profondeur,
                texture_3d(filtrable),
                texture_3d(filtrable),
                texture_2d(filtrable),
                sampler(SamplerBindingType::Filtering),
                texture_storage_2d(TextureFormat::Rgba16Float, StorageTextureAccess::WriteOnly),
                uniform_buffer::<ParamsReprojection>(false),
                texture_storage_2d(TextureFormat::R32Float, StorageTextureAccess::WriteOnly),
                texture_storage_2d_array(TextureFormat::Rgba16Float, StorageTextureAccess::WriteOnly),
            ),
        ),
    );
    let definitions: Vec<bevy::shader::ShaderDefVal> = if multi {
        vec!["MULTISAMPLED".into()]
    } else {
        Vec::new()
    };
    let calcul = |entree: &'static str| {
        cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(Cow::Borrowed(entree)),
            layout: vec![disposition_marche.clone()],
            shader: shader_marche.clone(),
            shader_defs: definitions.clone(),
            entry_point: Some(Cow::Borrowed(entree)),
            ..default()
        })
    };
    let marche = calcul("marcher");
    let marche_ciel = calcul("marcher_ciel");
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
    let composition = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("nuages_composition".into()),
        layout: vec![disposition_composition],
        vertex: sommet,
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
        multisample: MultisampleState {
            count: echantillons,
            ..default()
        },
        fragment: Some(FragmentState {
            shader: shader_composition,
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba16Float,
                blend: Some(melange),
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    VarianteNuages {
        disposition_marche,
        marche,
        marche_ciel,
        composition,
    }
}

#[derive(Resource, Default)]
pub(crate) struct TamponParamsNuages(pub(crate) UniformBuffer<ParamsNuages>);

#[derive(Component)]
pub(crate) struct TexturesNuages {
    /// Pixels marchés à cette image (un par bloc).
    frais: CachedTexture,
    /// Distance moyenne des nuages de chaque bloc.
    distance: CachedTexture,
    /// Image précédente reconstruite (lue).
    historique: TextureView,
    /// Image reconstruite à cette image (écrite, puis composée).
    sortie: TextureView,
    /// Nuages du cube de ciel (tableau de 6 couches), gardés d'une image à l'autre.
    pub(crate) ciel: TextureView,
    /// Le cube de ciel est à marcher et composer à cette image.
    pub(crate) ciel_actif: bool,
    params: ParamsReprojection,
}

#[derive(Component)]
pub(crate) struct GroupesNuages {
    marche: BindGroup,
    reconstruction: BindGroup,
    composition: BindGroup,
    taille_frais: UVec2,
    taille_cible: UVec2,
    echantillons: u32,
}

/// Images reconstruites gardées d'une image à l'autre, par caméra.
struct Historique {
    textures: [TextureView; 2],
    taille: UVec2,
    courante: usize,
    image: u32,
    precedente: Option<Mat4>,
    ciel: TextureView,
}

#[derive(Resource, Default)]
struct HistoriquesNuages(bevy::platform::collections::HashMap<MainEntity, Historique>);

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
    let disposition_reconstruction = BindGroupLayoutDescriptor::new(
        "nuages_reconstruction",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer::<ParamsReprojection>(false),
                texture_2d(filtrable),
                texture_2d(TextureSampleType::Float { filterable: false }),
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
    let reconstruction = load_embedded_asset!(serveur.as_ref(), "nuages_reconstruction.wesl");
    let calcul = |disposition: &BindGroupLayoutDescriptor, shader: &Handle<Shader>, entree: &'static str| {
        cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(Cow::Borrowed(entree)),
            layout: vec![disposition.clone()],
            shader: shader.clone(),
            entry_point: Some(Cow::Borrowed(entree)),
            ..default()
        })
    };
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
        variantes: default(),
        shader_marche: marche,
        shader_composition: composition,
        sommet_plein_ecran: plein_ecran.to_vertex_state(),
        reconstruction: calcul(&disposition_reconstruction, &reconstruction, "reconstruire"),
        disposition_reconstruction,
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
        disposition_composition,
        groupe_bruit: None,
        cuit: AtomicBool::new(false),
    });
}

fn texture_nuages(
    cache_textures: &mut TextureCache,
    peripherique: &RenderDevice,
    nom: &'static str,
    taille: UVec2,
    format: TextureFormat,
) -> CachedTexture {
    cache_textures.get(
        peripherique,
        TextureDescriptor {
            label: Some(nom),
            size: Extent3d {
                width: taille.x,
                height: taille.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
    )
}

fn preparer_textures(
    mut commandes: Commands,
    mut cache_textures: ResMut<TextureCache>,
    mut historiques: ResMut<HistoriquesNuages>,
    peripherique: Res<RenderDevice>,
    file: Res<RenderQueue>,
    ciel_atmosphere: Option<Res<crate::ciel_nuages::CielAtmosphere>>,
    vues: Query<(
        Entity,
        &MainEntity,
        &ExtractedCamera,
        &ExtractedView,
        &NuagesVolumetriques,
    )>,
) {
    let mut vues_vues = Vec::new();
    for (entite, principale, camera, vue, nuages) in &vues {
        let Some(taille_vue) = camera.physical_viewport_size else {
            continue;
        };
        vues_vues.push(*principale);
        let cible = taille_marche(taille_vue, nuages.echelle_resolution);
        let pas = if nuages.reprojection { 2 } else { 1 };
        let taille_frais = UVec2::new(cible.x.div_ceil(pas), cible.y.div_ceil(pas));
        let frais = texture_nuages(
            &mut cache_textures,
            &peripherique,
            "nuages_marche",
            taille_frais,
            TextureFormat::Rgba16Float,
        );
        let distance = texture_nuages(
            &mut cache_textures,
            &peripherique,
            "nuages_distance",
            taille_frais,
            TextureFormat::R32Float,
        );
        let historique = historiques.0.entry(*principale).or_insert_with(|| Historique {
            textures: [0, 1].map(|_| creer_historique(&peripherique, cible)),
            taille: cible,
            courante: 0,
            image: 0,
            precedente: None,
            ciel: creer_ciel(&peripherique, &file),
        });
        if historique.taille != cible {
            historique.textures = [0, 1].map(|_| creer_historique(&peripherique, cible));
            historique.taille = cible;
            historique.precedente = None;
        }
        let actuelle = vue.clip_from_view * vue.world_from_view.to_matrix().inverse();
        let params = ParamsReprojection {
            precedente: historique.precedente.unwrap_or(actuelle),
            taille_cible: cible,
            decalage: if pas == 2 {
                decalage_reprojection(historique.image)
            } else {
                UVec2::ZERO
            },
            pas,
            valide: u32::from(historique.precedente.is_some()),
            face_ciel: historique.image % 6,
            ..default()
        };
        let lecture = historique.courante;
        let ecriture = 1 - lecture;
        commandes.entity(entite).insert(TexturesNuages {
            frais,
            distance,
            historique: historique.textures[lecture].clone(),
            sortie: historique.textures[ecriture].clone(),
            ciel: historique.ciel.clone(),
            ciel_actif: nuages.ciel && ciel_atmosphere.is_some(),
            params,
        });
        historique.courante = ecriture;
        historique.image = historique.image.wrapping_add(1);
        historique.precedente = Some(actuelle);
    }
    // Caméras disparues : on libère leur historique.
    historiques.0.retain(|e, _| vues_vues.contains(e));
}

/// Côté d'une face du cube de ciel des nuages.
pub const TAILLE_CIEL: u32 = 32;

/// Cube de ciel des nuages, initialisé à « pas de nuage » (transmittance 1) : les faces pas encore marchées
/// laissent passer le ciel.
fn creer_ciel(peripherique: &RenderDevice, file: &RenderQueue) -> TextureView {
    let texels = (TAILLE_CIEL * TAILLE_CIEL * 6) as usize;
    // Rgba16Float (0, 0, 0, 1) : 1,0 en demi-flottant vaut 0x3C00.
    let donnees: Vec<u8> = std::iter::repeat_n([0u8, 0, 0, 0, 0, 0, 0x00, 0x3C], texels)
        .flatten()
        .collect();
    peripherique
        .create_texture_with_data(
            file,
            &TextureDescriptor {
                label: Some("nuages_ciel"),
                size: Extent3d {
                    width: TAILLE_CIEL,
                    height: TAILLE_CIEL,
                    depth_or_array_layers: 6,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba16Float,
                usage: TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            TextureDataOrder::LayerMajor,
            &donnees,
        )
        .create_view(&TextureViewDescriptor {
            dimension: Some(TextureViewDimension::D2Array),
            ..default()
        })
}

fn creer_historique(peripherique: &RenderDevice, taille: UVec2) -> TextureView {
    peripherique
        .create_texture(&TextureDescriptor {
            label: Some("nuages_historique"),
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
        })
        .create_view(&TextureViewDescriptor::default())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn preparer_groupes(
    mut commandes: Commands,
    mut ressources: ResMut<RessourcesNuages>,
    cache: Res<PipelineCache>,
    peripherique: Res<RenderDevice>,
    file: Res<RenderQueue>,
    uniformes_vues: Res<ViewUniforms>,
    params: Option<Res<ParamsNuages>>,
    mut tampon: ResMut<TamponParamsNuages>,
    vues: Query<(Entity, &TexturesNuages, &ViewDepthStencilTexture, &Msaa)>,
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
    let disposition_composition = cache.get_bind_group_layout(&ressources.disposition_composition);
    let disposition_reconstruction = cache.get_bind_group_layout(&ressources.disposition_reconstruction);
    for (entite, textures, profondeur, msaa) in &vues {
        let Some(variante) = ressources.variantes.get(&msaa.samples()) else {
            continue;
        };
        let disposition_marche = cache.get_bind_group_layout(&variante.disposition_marche);
        let mut tampon_reprojection = UniformBuffer::from(textures.params);
        tampon_reprojection.write_buffer(&peripherique, &file);
        let Some(liaison_reprojection) = tampon_reprojection.binding() else {
            continue;
        };
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
                &textures.frais.default_view,
                liaison_reprojection.clone(),
                &textures.distance.default_view,
                &textures.ciel,
            )),
        );
        let reconstruction = peripherique.create_bind_group(
            "nuages_reconstruction",
            &disposition_reconstruction,
            &BindGroupEntries::sequential((
                liaison_vue.clone(),
                liaison_reprojection,
                &textures.frais.default_view,
                &textures.distance.default_view,
                &textures.historique,
                &ressources.echantillonneur_nuages,
                &textures.sortie,
            )),
        );
        let composition = peripherique.create_bind_group(
            "nuages_composition",
            &disposition_composition,
            &BindGroupEntries::sequential((&textures.sortie, &ressources.echantillonneur_nuages)),
        );
        let taille = textures.frais.texture.size();
        commandes.entity(entite).insert(GroupesNuages {
            marche,
            reconstruction,
            composition,
            taille_frais: UVec2::new(taille.width, taille.height),
            taille_cible: textures.params.taille_cible,
            echantillons: msaa.samples(),
        });
    }
}

fn mettre_en_file(
    fonctions: Res<DrawFunctions<Transparent3d>>,
    mut ressources: ResMut<RessourcesNuages>,
    cache: Res<PipelineCache>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    vues: Query<(Entity, &MainEntity, &ExtractedView, &Msaa), With<NuagesVolumetriques>>,
) {
    let Some(fonction) = fonctions.read().get_id::<DessinerNuages>() else {
        return;
    };
    // Rien à composer tant que le bruit n'est pas cuit et que la marche ne peut pas tourner (la texture de
    // marche serait vide, donc noire et opaque).
    if !ressources.cuit.load(Ordering::Relaxed)
        || cache.get_compute_pipeline(ressources.reconstruction).is_none()
    {
        // Les variantes sont quand même créées, pour que leurs pipelines compilent pendant la cuisson.
        for (_, _, _, msaa) in &vues {
            ressources.variante(&cache, msaa.samples());
        }
        return;
    }
    for (entite, entite_principale, vue, msaa) in &vues {
        let variante = ressources.variante(&cache, msaa.samples()).clone();
        if cache.get_compute_pipeline(variante.marche).is_none() {
            continue;
        }
        let Some(phase) = phases.get_mut(&vue.retained_view_entity) else {
            continue;
        };
        if vue.target_format != TextureFormat::Rgba16Float {
            continue;
        }
        // Dessiné en premier dans la passe transparente : derrière la pluie et les objets transparents.
        phase.add_transient(Transparent3d {
            pipeline: variante.composition,
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

pub(crate) fn cuire_bruit(
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

pub(crate) fn marcher(
    vue: ViewQuery<(&ViewUniformOffset, &GroupesNuages, &TexturesNuages)>,
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
    let (decalage_vue, groupes, textures) = vue.into_inner();
    let Some(variante) = ressources.variantes.get(&groupes.echantillons) else {
        return;
    };
    let (Some(pipeline), Some(pipeline_reconstruction)) = (
        cache.get_compute_pipeline(variante.marche),
        cache.get_compute_pipeline(ressources.reconstruction),
    ) else {
        return;
    };
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
    passe.dispatch_workgroups(
        groupes.taille_frais.x.div_ceil(8),
        groupes.taille_frais.y.div_ceil(8),
        1,
    );
    span.end(&mut passe);
    let span = diagnostics.pass_span(&mut passe, "nuages_reconstruction");
    passe.set_pipeline(pipeline_reconstruction);
    passe.set_bind_group(0, &groupes.reconstruction, &[decalage_vue.offset]);
    passe.dispatch_workgroups(
        groupes.taille_cible.x.div_ceil(8),
        groupes.taille_cible.y.div_ceil(8),
        1,
    );
    span.end(&mut passe);
    if textures.ciel_actif
        && let Some(pipeline_ciel) = cache.get_compute_pipeline(variante.marche_ciel)
    {
        let span = diagnostics.pass_span(&mut passe, "nuages_ciel");
        passe.set_pipeline(pipeline_ciel);
        passe.set_bind_group(0, &groupes.marche, &[decalage_vue.offset]);
        passe.dispatch_workgroups(TAILLE_CIEL / 8, TAILLE_CIEL / 8, 1);
        span.end(&mut passe);
    }
}
