//! Nuages dans les reflets et la lumière ambiante (étape 16).
//!
//! L'atmosphère de Bevy (`AtmosphereEnvironmentMapLight`) génère chaque image une carte d'environnement (cube)
//! à partir du ciel seul : sous un ciel couvert, la mer reflétait un ciel bleu et la scène recevait la lumière
//! ambiante d'un ciel dégagé. Ce module y ajoute les nuages, sans modifier Bevy :
//! 1. la marche des nuages (`marcher_ciel`) calcule, depuis la caméra, les nuages d'une face du cube par image
//!    (32 × 32, 6 images pour le cube entier ; les nuages bougent lentement) ;
//! 2. après la passe opaque, la carte de l'atmosphère est copiée puis réécrite en nuages + transmittance × ciel ;
//! 3. Bevy filtre cette carte (reflets flous, irradiance) à l'image suivante, comme d'habitude.
//!
//! Il suffit que la caméra porte à la fois `NuagesVolumetriques` (avec `ciel: true`, par défaut) et
//! `AtmosphereEnvironmentMapLight`. Une seule carte d'environnement d'atmosphère est prise en charge.

use std::borrow::Cow;

use bevy::asset::load_embedded_asset;
use bevy::core_pipeline::core_3d::main_transparent_pass_3d;
use bevy::core_pipeline::schedule::Core3d;
use bevy::light::{AtmosphereEnvironmentMapLight, GeneratedEnvironmentMapLight};
use bevy::prelude::*;
use bevy::render::Extract;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{sampler, texture_2d_array, texture_storage_2d_array};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::{CachedTexture, GpuImage, TextureCache};
use bevy::render::{ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems};

use crate::nuages::{TexturesNuages, marcher};

/// Carte d'environnement générée par l'atmosphère (monde du rendu).
#[derive(Resource, Clone, Debug)]
pub struct CielAtmosphere(pub Handle<Image>);

pub(crate) fn construire(app: &mut App) {
    bevy::asset::embedded_asset!(app, "nuages_ciel.wesl");
}

pub(crate) fn finir(app: &mut App) {
    let Some(rendu) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    rendu
        .add_systems(ExtractSchedule, extraire_ciel)
        .add_systems(RenderStartup, creer_pipeline)
        .add_systems(Render, preparer_groupes.in_set(RenderSystems::PrepareBindGroups))
        .add_systems(Core3d, composer.after(marcher).before(main_transparent_pass_3d));
}

fn extraire_ciel(
    mut commandes: Commands,
    sondes: Extract<Query<&GeneratedEnvironmentMapLight, With<AtmosphereEnvironmentMapLight>>>,
    actuel: Option<Res<CielAtmosphere>>,
) {
    match (sondes.iter().next(), actuel) {
        (Some(sonde), Some(actuel)) if actuel.0 == sonde.environment_map => {}
        (Some(sonde), _) => commandes.insert_resource(CielAtmosphere(sonde.environment_map.clone())),
        (None, Some(_)) => commandes.remove_resource::<CielAtmosphere>(),
        (None, None) => {}
    }
}

#[derive(Resource)]
struct RessourcesCiel {
    disposition: BindGroupLayoutDescriptor,
    pipeline: CachedComputePipelineId,
    echantillonneur: Sampler,
}

#[derive(Component)]
struct GroupeCiel {
    groupe: BindGroup,
    copie: CachedTexture,
    source: Texture,
}

fn creer_pipeline(
    mut commandes: Commands,
    serveur: Res<AssetServer>,
    cache: Res<PipelineCache>,
    peripherique: Res<RenderDevice>,
) {
    let filtrable = TextureSampleType::Float { filterable: true };
    let disposition = BindGroupLayoutDescriptor::new(
        "nuages_ciel",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_2d_array(filtrable),
                texture_2d_array(filtrable),
                sampler(SamplerBindingType::Filtering),
                texture_storage_2d_array(TextureFormat::Rgba16Float, StorageTextureAccess::WriteOnly),
            ),
        ),
    );
    let pipeline = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some(Cow::Borrowed("composer_ciel")),
        layout: vec![disposition.clone()],
        shader: load_embedded_asset!(serveur.as_ref(), "nuages_ciel.wesl"),
        entry_point: Some(Cow::Borrowed("composer_ciel")),
        ..default()
    });
    let echantillonneur = peripherique.create_sampler(&SamplerDescriptor {
        label: Some("nuages_ciel"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    commandes.insert_resource(RessourcesCiel {
        disposition,
        pipeline,
        echantillonneur,
    });
}

#[allow(clippy::too_many_arguments)]
fn preparer_groupes(
    mut commandes: Commands,
    ressources: Res<RessourcesCiel>,
    ciel: Option<Res<CielAtmosphere>>,
    images: Res<RenderAssets<GpuImage>>,
    mut cache_textures: ResMut<TextureCache>,
    cache: Res<PipelineCache>,
    peripherique: Res<RenderDevice>,
    vues: Query<(Entity, &TexturesNuages)>,
) {
    let Some(image) = ciel.and_then(|c| images.get(&c.0)) else {
        return;
    };
    let taille = image.texture.size();
    if taille.depth_or_array_layers != 6 || image.texture.format() != TextureFormat::Rgba16Float {
        return;
    }
    let tableau = TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    };
    let disposition = cache.get_bind_group_layout(&ressources.disposition);
    for (entite, textures) in &vues {
        if !textures.ciel_actif {
            continue;
        }
        let copie = cache_textures.get(
            &peripherique,
            TextureDescriptor {
                label: Some("nuages_ciel_copie"),
                size: taille,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba16Float,
                usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        );
        let vue_copie = copie.texture.create_view(&tableau);
        let vue_sortie = image.texture.create_view(&tableau);
        let groupe = peripherique.create_bind_group(
            "nuages_ciel",
            &disposition,
            &BindGroupEntries::sequential((
                &vue_copie,
                &textures.ciel,
                &ressources.echantillonneur,
                &vue_sortie,
            )),
        );
        commandes.entity(entite).insert(GroupeCiel {
            groupe,
            copie,
            source: image.texture.clone(),
        });
    }
}

fn composer(
    vue: ViewQuery<(&TexturesNuages, Option<&GroupeCiel>)>,
    ressources: Option<Res<RessourcesCiel>>,
    cache: Res<PipelineCache>,
    mut contexte: RenderContext,
) {
    let (textures, groupe) = vue.into_inner();
    let (Some(ressources), Some(groupe)) = (ressources, groupe) else {
        return;
    };
    if !textures.ciel_actif {
        return;
    }
    let Some(pipeline) = cache.get_compute_pipeline(ressources.pipeline) else {
        return;
    };
    let taille = groupe.source.size();
    let diagnostics = contexte.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let encodeur = contexte.command_encoder();
    encodeur.copy_texture_to_texture(
        groupe.source.as_image_copy(),
        groupe.copie.texture.as_image_copy(),
        taille,
    );
    let mut passe = encodeur.begin_compute_pass(&ComputePassDescriptor {
        label: Some("nuages_ciel_composition"),
        ..default()
    });
    let span = diagnostics.pass_span(&mut passe, "nuages_ciel_composition");
    passe.set_pipeline(pipeline);
    passe.set_bind_group(0, &groupe.groupe, &[]);
    passe.dispatch_workgroups(taille.width.div_ceil(8), taille.height.div_ceil(8), 6);
    span.end(&mut passe);
}
