//! Ombre des nuages au sol et sur la mer (étape 15).
//!
//! Une passe de calcul écrit, à chaque image, la transmittance des nuages vue du soleil dans une texture
//! 128 × 128 centrée sur la caméra (8 km de côté par défaut, soit 62 m par texel : les ombres de nuages sont floues). Cette texture est posée sur le
//! soleil comme `DirectionalLightTexture` : Bevy multiplie alors la lumière du soleil par elle, sur **tous** les
//! matériaux (océan, terrain, `StandardMaterial`…), sans toucher à leurs shaders. La lumière du ciel (ambiance,
//! reflets) n'est pas touchée : une ombre de nuage reste éclairée par le ciel, comme en vrai.
//!
//! Projection de Bevy pour une texture de lumière directionnelle : `local = inverse(transformation du soleil) ×
//! position`, `uv = local.xy × (-0,5 ; 0,5) + 0,5`. On règle donc la translation du soleil sur la caméra
//! (calée sur les texels pour que l'ombre ne scintille pas quand on bouge) et son échelle sur la demi-taille de
//! la carte. Seule la rotation du soleil compte pour sa direction et ses cascades d'ombre : translation et
//! échelle sont libres. Le soleil ne doit pas avoir de parent.
//!
//! Conditions côté application : la feature Bevy `pbr_light_textures`, et des tableaux de textures sur le GPU
//! (pas sur Metal, voir `clustered_decals_are_usable` dans Bevy). Sans elles, la carte est calculée mais
//! ignorée par le rendu : pas d'ombre, rien d'autre ne change.

use std::borrow::Cow;
use std::sync::atomic::Ordering;

use bevy::asset::{RenderAssetUsages, load_embedded_asset};
use bevy::core_pipeline::schedule::camera_driver;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::DirectionalLightTexture;
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    sampler, texture_2d, texture_3d, texture_storage_2d, uniform_buffer,
};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, RenderGraph, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

use crate::MiseAJourMeteo;
use crate::nuages::{NuagesVolumetriques, ParamsNuages, RessourcesNuages, TamponParamsNuages, cuire_bruit};

/// Côté de la carte d'ombre (texels).
pub const TAILLE_OMBRE: u32 = 128;

/// Réglages de l'ombre des nuages.
#[derive(Resource, Reflect, Debug, Clone, PartialEq)]
#[reflect(Resource, Default)]
pub struct ReglagesOmbreNuages {
    /// Ombre active (sinon la texture est retirée du soleil).
    pub active: bool,
    /// Demi-côté de la zone couverte autour de la caméra, dans le plan du soleil (m).
    pub demi_taille: f32,
    /// Pas de la traversée de la couche de nuages.
    pub pas: u32,
}

impl Default for ReglagesOmbreNuages {
    fn default() -> Self {
        Self {
            active: true,
            demi_taille: 4000.0,
            pas: 16,
        }
    }
}

/// Texture de l'ombre des nuages (R = part de soleil qui passe), posée sur le soleil.
#[derive(Resource, Clone, Debug, ExtractResource)]
#[extract_app(RenderApp)]
pub struct OmbreNuages {
    /// Image écrite par le GPU à chaque image.
    pub image: Handle<Image>,
}

impl FromWorld for OmbreNuages {
    fn from_world(monde: &mut World) -> Self {
        let mut image = Image::new_fill(
            Extent3d {
                width: TAILLE_OMBRE,
                height: TAILLE_OMBRE,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[255, 255, 255, 255],
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.usage = TextureUsages::COPY_DST
            | TextureUsages::COPY_SRC
            | TextureUsages::STORAGE_BINDING
            | TextureUsages::TEXTURE_BINDING;
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::ClampToEdge,
            address_mode_v: ImageAddressMode::ClampToEdge,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            ..default()
        });
        let image = monde.resource_mut::<Assets<Image>>().add(image);
        Self { image }
    }
}

/// Paramètres de la passe d'ombre (même ordre que `ParamsOmbre` dans `nuages_ombre.wesl`). `pas = 0` : rien à
/// calculer.
#[derive(Resource, ShaderType, Clone, Copy, Debug, Default, PartialEq, ExtractResource)]
#[extract_app(RenderApp)]
pub struct ParamsOmbre {
    /// Centre de la carte (translation du soleil, m).
    pub centre: Vec3,
    /// Demi-côté (échelle du soleil, m).
    pub demi_taille: f32,
    /// Axe X local du soleil (monde).
    pub droite: Vec3,
    /// Pas de la traversée.
    pub pas: u32,
    /// Axe Y local du soleil (monde).
    pub haut_lumiere: Vec3,
    r0: f32,
    /// Direction vers le soleil (axe +Z local).
    pub vers_soleil: Vec3,
    r1: f32,
}

/// Translation à donner au soleil : la caméra, ramenée sur la grille des texels dans le plan du soleil (la
/// composante le long du rayon ne change pas la projection). Évite que l'ombre glisse d'un demi-texel à chaque
/// mouvement de la caméra.
pub fn centre_ombre(rotation: Quat, camera: Vec3, demi_taille: f32) -> Vec3 {
    let texel = 2.0 * demi_taille / TAILLE_OMBRE as f32;
    let local = rotation.inverse() * camera;
    let cale = Vec3::new(
        (local.x / texel).round() * texel,
        (local.y / texel).round() * texel,
        local.z,
    );
    rotation * cale
}

/// Paramètres de la passe pour un soleil d'orientation `rotation`, centré en `centre`.
pub fn params_ombre(rotation: Quat, centre: Vec3, reglages: &ReglagesOmbreNuages) -> ParamsOmbre {
    ParamsOmbre {
        centre,
        demi_taille: reglages.demi_taille,
        droite: rotation * Vec3::X,
        pas: reglages.pas,
        haut_lumiere: rotation * Vec3::Y,
        vers_soleil: rotation * Vec3::Z,
        ..default()
    }
}

/// Coordonnées de texture de Bevy pour un point du monde (même formule que `directional_light` dans
/// `pbr_lighting.wesl`) : sert aux tests et au gameplay (savoir si un point est à l'ombre d'un nuage).
pub fn uv_ombre(rotation: Quat, centre: Vec3, demi_taille: f32, point: Vec3) -> Vec2 {
    let local = rotation.inverse() * (point - centre) / demi_taille;
    Vec2::new(local.x * -0.5 + 0.5, local.y * 0.5 + 0.5)
}

pub(crate) fn construire(app: &mut App) {
    bevy::asset::embedded_asset!(app, "nuages_ombre.wesl");
    app.register_type::<ReglagesOmbreNuages>()
        .init_resource::<ReglagesOmbreNuages>()
        .init_resource::<OmbreNuages>()
        .init_resource::<ParamsOmbre>()
        .add_plugins((
            ExtractResourcePlugin::<OmbreNuages>::default(),
            ExtractResourcePlugin::<ParamsOmbre>::default(),
        ))
        .add_systems(Update, placer_ombre.after(MiseAJourMeteo));
}

pub(crate) fn finir(app: &mut App) {
    let Some(rendu) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    rendu
        .add_systems(RenderStartup, creer_pipeline)
        .add_systems(
            Render,
            preparer_groupe
                .in_set(RenderSystems::PrepareBindGroups)
                .after(crate::nuages::preparer_groupes),
        )
        .add_systems(RenderGraph, ombrer.after(cuire_bruit).before(camera_driver));
}

/// Pose la texture sur le soleil le plus fort et le centre sur la première caméra à nuages.
#[allow(clippy::type_complexity)]
fn placer_ombre(
    mut commandes: Commands,
    reglages: Res<ReglagesOmbreNuages>,
    ombre: Res<OmbreNuages>,
    mut params: ResMut<ParamsOmbre>,
    cameras: Query<&GlobalTransform, With<NuagesVolumetriques>>,
    mut soleils: Query<(
        Entity,
        &DirectionalLight,
        &mut Transform,
        Option<&DirectionalLightTexture>,
    )>,
) {
    let soleil = soleils
        .iter_mut()
        .max_by(|a, b| a.1.illuminance.total_cmp(&b.1.illuminance));
    let (Some((entite, _, mut transformation, texture)), Some(camera)) = (soleil, cameras.iter().next())
    else {
        params.pas = 0;
        return;
    };
    if !reglages.active {
        if texture.is_some_and(|t| t.image == ombre.image) {
            commandes.entity(entite).remove::<DirectionalLightTexture>();
        }
        params.pas = 0;
        return;
    }
    if texture.is_none_or(|t| t.image != ombre.image || t.tiled) {
        commandes.entity(entite).insert(DirectionalLightTexture {
            image: ombre.image.clone(),
            tiled: false,
        });
    }
    let rotation = transformation.rotation;
    let centre = centre_ombre(rotation, camera.translation(), reglages.demi_taille);
    let echelle = Vec3::splat(reglages.demi_taille);
    // N'écrire que si ça change : la propagation des transformations ne tourne pas pour rien.
    if transformation.translation != centre || transformation.scale != echelle {
        transformation.translation = centre;
        transformation.scale = echelle;
    }
    *params = params_ombre(rotation, centre, &reglages);
}

// ---------------------------------------------------------------------------------------------------------------
// Monde du rendu

#[derive(Resource)]
struct RessourcesOmbre {
    disposition: BindGroupLayoutDescriptor,
    pipeline: CachedComputePipelineId,
    tampon: UniformBuffer<ParamsOmbre>,
    groupe: Option<BindGroup>,
    pas: u32,
}

fn creer_pipeline(mut commandes: Commands, serveur: Res<AssetServer>, cache: Res<PipelineCache>) {
    let filtrable = TextureSampleType::Float { filterable: true };
    let disposition = BindGroupLayoutDescriptor::new(
        "nuages_ombre",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer::<ParamsNuages>(false),
                uniform_buffer::<ParamsOmbre>(false),
                texture_3d(filtrable),
                texture_2d(filtrable),
                sampler(SamplerBindingType::Filtering),
                texture_storage_2d(TextureFormat::Rgba8Unorm, StorageTextureAccess::WriteOnly),
            ),
        ),
    );
    let shader = load_embedded_asset!(serveur.as_ref(), "nuages_ombre.wesl");
    let pipeline = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some(Cow::Borrowed("nuages_ombre")),
        layout: vec![disposition.clone()],
        shader,
        entry_point: Some(Cow::Borrowed("ombrer")),
        ..default()
    });
    commandes.insert_resource(RessourcesOmbre {
        disposition,
        pipeline,
        tampon: UniformBuffer::default(),
        groupe: None,
        pas: 0,
    });
}

#[allow(clippy::too_many_arguments)]
fn preparer_groupe(
    mut ombre_rendu: ResMut<RessourcesOmbre>,
    nuages: Option<Res<RessourcesNuages>>,
    tampon_nuages: Res<TamponParamsNuages>,
    ombre: Option<Res<OmbreNuages>>,
    params: Option<Res<ParamsOmbre>>,
    images: Res<RenderAssets<GpuImage>>,
    cache: Res<PipelineCache>,
    peripherique: Res<RenderDevice>,
    file: Res<RenderQueue>,
) {
    ombre_rendu.groupe = None;
    ombre_rendu.pas = 0;
    let (Some(nuages), Some(ombre), Some(params)) = (nuages, ombre, params) else {
        return;
    };
    if params.pas == 0 {
        return;
    }
    let Some(image) = images.get(&ombre.image) else {
        return;
    };
    ombre_rendu.tampon.set(*params);
    ombre_rendu.tampon.write_buffer(&peripherique, &file);
    let (Some(liaison_nuages), Some(liaison_ombre)) =
        (tampon_nuages.0.binding(), ombre_rendu.tampon.binding())
    else {
        return;
    };
    let groupe = peripherique.create_bind_group(
        "nuages_ombre",
        &cache.get_bind_group_layout(&ombre_rendu.disposition),
        &BindGroupEntries::sequential((
            liaison_nuages,
            liaison_ombre,
            &nuages.forme,
            &nuages.carte,
            &nuages.echantillonneur_bruit,
            &image.texture_view,
        )),
    );
    ombre_rendu.groupe = Some(groupe);
    ombre_rendu.pas = params.pas;
}

fn ombrer(
    mut contexte: RenderContext,
    ombre: Option<Res<RessourcesOmbre>>,
    nuages: Option<Res<RessourcesNuages>>,
    cache: Res<PipelineCache>,
) {
    let (Some(ombre), Some(nuages)) = (ombre, nuages) else {
        return;
    };
    if !nuages.cuit.load(Ordering::Relaxed) || ombre.pas == 0 {
        return;
    }
    let (Some(groupe), Some(pipeline)) = (&ombre.groupe, cache.get_compute_pipeline(ombre.pipeline)) else {
        return;
    };
    let diagnostics = contexte.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let mut passe = contexte
        .command_encoder()
        .begin_compute_pass(&ComputePassDescriptor {
            label: Some("nuages_ombre"),
            ..default()
        });
    let span = diagnostics.pass_span(&mut passe, "nuages_ombre");
    passe.set_pipeline(pipeline);
    passe.set_bind_group(0, groupe, &[]);
    passe.dispatch_workgroups(TAILLE_OMBRE / 8, TAILLE_OMBRE / 8, 1);
    span.end(&mut passe);
}
