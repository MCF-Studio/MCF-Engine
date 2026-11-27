//! Océan FFT (étape 5b), porté de SkyForge : 3 cascades 256 × 256 du spectre JONSWAP, évoluées et transformées sur
//! le GPU à chaque image, avec l'écume des crêtes qui se replient (jacobien).
//!
//! Répartition du travail avec [`crate::houle`] : la houle de Gerstner (calculée aussi sur le CPU, pour la
//! flottaison) porte le pic du spectre ; les cascades FFT ajoutent la mer du vent, des vagues un peu plus courtes
//! que le pic jusqu'aux rides de quelques centimètres. La hauteur donnée par `Ocean::hauteur` ignore donc
//! seulement ces vagues plus petites.
//!
//! Passes (dans le graphe de rendu, avant les caméras) :
//! 1. spectre initial h0(k) (seulement quand le vent ou le fetch change) ;
//! 2. spectre à l'instant t, 8 champs réels empaquetés dans 2 textures ;
//! 3. IFFT des lignes puis 4. des colonnes (mémoire partagée) ;
//! 5. assemblage : déplacement + écume, dérivées (lues par le matériau de l'océan).

use std::borrow::Cow;

use bevy::asset::{RenderAssetUsages, load_embedded_asset};
use bevy::core_pipeline::schedule::camera_driver;
use bevy::image::{
    ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor, TextureFormatPixelInfo,
};
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    texture_2d_array, texture_storage_2d_array, uniform_buffer,
};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, RenderGraph, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::load_shader_library;

use crate::MiseAJourMeteo;
use crate::etat::EtatMeteo;
use crate::houle::Ocean;
use crate::mer_du_vent::{ParamsSpectre, SuiviSpectre};

pub use crate::mer_du_vent::{CASCADES, N_FFT, ReglagesFft, bandes};

/// Paramètres envoyés aux shaders (même ordre que `ParamsFft` dans `ocean_commun.wesl`).
#[derive(Resource, ShaderType, Clone, Copy, Debug, Default, PartialEq, ExtractResource)]
#[extract_app(RenderApp)]
pub struct ParamsFft {
    /// Taille du motif de chaque cascade (m).
    pub tailles: Vec4,
    /// Début de la bande de nombres d'onde de chaque cascade.
    pub k_min: Vec4,
    /// Fin de la bande.
    pub k_max: Vec4,
    /// Direction du vent (plan XZ).
    pub vent: Vec2,
    /// Vitesse du vent à 10 m (m/s).
    pub u10: f32,
    /// Fetch (m).
    pub fetch: f32,
    /// Déplacement horizontal.
    pub choppiness: f32,
    /// Décroissance de l'écume (1/s).
    pub decroissance_ecume: f32,
    /// Jacobien sous lequel l'écume apparaît.
    pub seuil_ecume: f32,
    /// Temps bouclé (s).
    pub temps: f32,
    /// Pas de temps (s).
    pub dt: f32,
    r0: f32,
    r1: f32,
    r2: f32,
}

/// Compteurs extraits vers le rendu : quand recalculer le spectre initial, et la parité de l'écume.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, ExtractResource)]
#[extract_app(RenderApp)]
pub struct EtatFft {
    /// Augmente à chaque changement notable du vent ou du fetch.
    pub generation_spectre: u32,
    /// Numéro d'image (parité = sens du ping-pong de l'écume).
    pub image: u64,
    suivi: SuiviSpectre,
}

/// Textures de la simulation (tableaux 2D de 3 couches, une par cascade).
#[derive(Resource, Clone, Debug, ExtractResource)]
#[extract_app(RenderApp)]
pub struct TexturesOcean {
    h0: Handle<Image>,
    a: Handle<Image>,
    b: Handle<Image>,
    a2: Handle<Image>,
    b2: Handle<Image>,
    /// Déplacement (xyz, m) et écume (w), échantillonné par le matériau.
    pub deplacement: Handle<Image>,
    /// Pentes (dDy/dx, dDy/dz) et termes du jacobien, échantillonnés par le matériau.
    pub derivees: Handle<Image>,
    ecume: [Handle<Image>; 2],
}

fn texture_tableau(format: TextureFormat, echantillonnage: bool) -> Image {
    let taille = format.pixel_size().unwrap_or(16);
    let mut img = Image::new(
        Extent3d {
            width: N_FFT,
            height: N_FFT,
            depth_or_array_layers: CASCADES,
        },
        TextureDimension::D2,
        vec![0; (N_FFT * N_FFT * CASCADES) as usize * taille],
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.usage = TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::STORAGE_BINDING
        | TextureUsages::TEXTURE_BINDING;
    img.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    if echantillonnage {
        img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            ..default()
        });
    }
    img
}

impl TexturesOcean {
    fn creer(images: &mut Assets<Image>) -> Self {
        let mut f32x4 = || images.add(texture_tableau(TextureFormat::Rgba32Float, false));
        let (h0, a, b, a2, b2) = (f32x4(), f32x4(), f32x4(), f32x4(), f32x4());
        let mut f16x4 = |e| images.add(texture_tableau(TextureFormat::Rgba16Float, e));
        Self {
            h0,
            a,
            b,
            a2,
            b2,
            deplacement: f16x4(true),
            derivees: f16x4(true),
            ecume: [f16x4(false), f16x4(false)],
        }
    }
}

/// Paramètres de l'image (fonction pure).
pub fn calculer_params(etat: &EtatMeteo, ocean: &Ocean, reglages: &ReglagesFft, dt: f32) -> ParamsFft {
    let s = ParamsSpectre::depuis(etat, reglages);
    ParamsFft {
        tailles: Vec4::new(s.tailles[0], s.tailles[1], s.tailles[2], 0.0),
        k_min: Vec4::new(s.k_min[0], s.k_min[1], s.k_min[2], 0.0),
        k_max: Vec4::new(s.k_max[0], s.k_max[1], s.k_max[2], 0.0),
        vent: s.vent,
        u10: s.u10,
        fetch: s.fetch,
        choppiness: reglages.choppiness,
        decroissance_ecume: reglages.decroissance_ecume,
        seuil_ecume: 0.75 * ((etat.vent_vitesse - 3.0) / 15.0).clamp(0.0, 1.0),
        temps: ocean.temps(),
        dt: if dt.is_finite() { dt.max(0.0) } else { 0.0 },
        ..default()
    }
}

impl EtatFft {
    /// Avance d'une image ; renvoie vrai si le spectre initial doit être recalculé (même règle que la réplique
    /// CPU, [`crate::mer_du_vent::MerDuVent`]).
    pub fn avancer(&mut self, params: &ParamsFft) -> bool {
        self.image = self.image.wrapping_add(1);
        let change = self.suivi.avancer(params.u10, params.vent, params.fetch);
        self.generation_spectre = self.suivi.generation;
        change
    }
}

/// Plugin de la simulation FFT. Ajouté par [`crate::McfOceanPlugin`].
pub(crate) struct McfOceanFftPlugin;

impl Plugin for McfOceanFftPlugin {
    fn build(&self, app: &mut App) {
        load_shader_library!(app, "ocean_commun.wesl");
        bevy::asset::embedded_asset!(app, "ocean_spectre.wesl");
        bevy::asset::embedded_asset!(app, "ocean_ifft.wesl");
        bevy::asset::embedded_asset!(app, "ocean_assemblage.wesl");
        app.register_type::<ReglagesFft>()
            .init_resource::<ReglagesFft>()
            .init_resource::<ParamsFft>()
            .init_resource::<EtatFft>()
            .add_plugins((
                ExtractResourcePlugin::<ParamsFft>::default(),
                ExtractResourcePlugin::<EtatFft>::default(),
                ExtractResourcePlugin::<TexturesOcean>::default(),
            ))
            .add_systems(Update, mettre_a_jour_params.after(MiseAJourMeteo));

        let Some(rendu) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        rendu
            .init_resource::<EtatRenduFft>()
            .add_systems(RenderStartup, creer_pipelines)
            .add_systems(Render, preparer_groupes.in_set(RenderSystems::PrepareBindGroups))
            .add_systems(RenderGraph, simuler.before(camera_driver));
    }

    fn finish(&self, app: &mut App) {
        if !app.world().contains_resource::<TexturesOcean>() {
            let textures = TexturesOcean::creer(&mut app.world_mut().resource_mut::<Assets<Image>>());
            app.insert_resource(textures);
        }
    }
}

fn mettre_a_jour_params(
    etat: Res<EtatMeteo>,
    ocean: Res<Ocean>,
    reglages: Res<ReglagesFft>,
    temps: Res<Time>,
    mut params: ResMut<ParamsFft>,
    mut etat_fft: ResMut<EtatFft>,
) {
    *params = calculer_params(&etat, &ocean, &reglages, temps.delta_secs());
    etat_fft.avancer(&params);
}

// ---------------------------------------------------------------------------------------------------------------
// Monde du rendu

#[derive(Resource)]
struct PipelinesFft {
    disposition_init: BindGroupLayoutDescriptor,
    disposition_evolution: BindGroupLayoutDescriptor,
    disposition_ifft: BindGroupLayoutDescriptor,
    disposition_assemblage: BindGroupLayoutDescriptor,
    initialiser: CachedComputePipelineId,
    evoluer: CachedComputePipelineId,
    lignes: CachedComputePipelineId,
    colonnes: CachedComputePipelineId,
    assembler: CachedComputePipelineId,
}

#[derive(Resource, Default)]
struct EtatRenduFft {
    generation_faite: u32,
    initialiser: bool,
    groupes: Option<GroupesFft>,
}

struct GroupesFft {
    init: BindGroup,
    evolution: BindGroup,
    lignes: BindGroup,
    colonnes: BindGroup,
    assemblage: BindGroup,
}

fn creer_pipelines(mut commandes: Commands, serveur: Res<AssetServer>, cache: Res<PipelineCache>) {
    let non_filtrable = TextureSampleType::Float { filterable: false };
    let ecriture = |format| texture_storage_2d_array(format, StorageTextureAccess::WriteOnly);
    let disposition_init = BindGroupLayoutDescriptor::new(
        "ocean_fft_init",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::COMPUTE,
            (
                (0, uniform_buffer::<ParamsFft>(false)),
                (1, ecriture(TextureFormat::Rgba32Float)),
            ),
        ),
    );
    let disposition_evolution = BindGroupLayoutDescriptor::new(
        "ocean_fft_evolution",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::COMPUTE,
            (
                (0, uniform_buffer::<ParamsFft>(false)),
                (2, texture_2d_array(non_filtrable)),
                (3, ecriture(TextureFormat::Rgba32Float)),
                (4, ecriture(TextureFormat::Rgba32Float)),
            ),
        ),
    );
    let disposition_ifft = BindGroupLayoutDescriptor::new(
        "ocean_fft_ifft",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer::<ParamsFft>(false),
                texture_2d_array(non_filtrable),
                texture_2d_array(non_filtrable),
                ecriture(TextureFormat::Rgba32Float),
                ecriture(TextureFormat::Rgba32Float),
            ),
        ),
    );
    let disposition_assemblage = BindGroupLayoutDescriptor::new(
        "ocean_fft_assemblage",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer::<ParamsFft>(false),
                texture_2d_array(non_filtrable),
                texture_2d_array(non_filtrable),
                texture_2d_array(non_filtrable),
                ecriture(TextureFormat::Rgba16Float),
                ecriture(TextureFormat::Rgba16Float),
                ecriture(TextureFormat::Rgba16Float),
            ),
        ),
    );
    let spectre = load_embedded_asset!(serveur.as_ref(), "ocean_spectre.wesl");
    let ifft = load_embedded_asset!(serveur.as_ref(), "ocean_ifft.wesl");
    let assemblage = load_embedded_asset!(serveur.as_ref(), "ocean_assemblage.wesl");
    let pipeline =
        |disposition: &BindGroupLayoutDescriptor, shader: &Handle<Shader>, entree: &'static str| {
            cache.queue_compute_pipeline(ComputePipelineDescriptor {
                label: Some(Cow::Borrowed(entree)),
                layout: vec![disposition.clone()],
                shader: shader.clone(),
                entry_point: Some(Cow::Borrowed(entree)),
                ..default()
            })
        };
    let pipelines = PipelinesFft {
        initialiser: pipeline(&disposition_init, &spectre, "initialiser"),
        evoluer: pipeline(&disposition_evolution, &spectre, "evoluer"),
        lignes: pipeline(&disposition_ifft, &ifft, "lignes"),
        colonnes: pipeline(&disposition_ifft, &ifft, "colonnes"),
        assembler: pipeline(&disposition_assemblage, &assemblage, "assembler"),
        disposition_init,
        disposition_evolution,
        disposition_ifft,
        disposition_assemblage,
    };
    commandes.insert_resource(pipelines);
}

fn preparer_groupes(
    pipelines: Res<PipelinesFft>,
    cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    textures: Option<Res<TexturesOcean>>,
    params: Option<Res<ParamsFft>>,
    etat: Option<Res<EtatFft>>,
    peripherique: Res<RenderDevice>,
    file: Res<RenderQueue>,
    mut rendu: ResMut<EtatRenduFft>,
) {
    rendu.groupes = None;
    let (Some(textures), Some(params), Some(etat)) = (textures, params, etat) else {
        return;
    };
    let vue = |h: &Handle<Image>| images.get(h).map(|i| &i.texture_view);
    let (Some(h0), Some(a), Some(b), Some(a2), Some(b2), Some(depl), Some(deriv), Some(e0), Some(e1)) = (
        vue(&textures.h0),
        vue(&textures.a),
        vue(&textures.b),
        vue(&textures.a2),
        vue(&textures.b2),
        vue(&textures.deplacement),
        vue(&textures.derivees),
        vue(&textures.ecume[0]),
        vue(&textures.ecume[1]),
    ) else {
        return;
    };
    let mut tampon = UniformBuffer::from(*params);
    tampon.write_buffer(&peripherique, &file);
    let Some(liaison) = tampon.binding() else {
        return;
    };
    // Ping-pong de l'écume : on lit celle de l'image précédente, on écrit l'autre.
    let (ecume_lue, ecume_ecrite) = if etat.image % 2 == 0 { (e0, e1) } else { (e1, e0) };
    let disposition = |d: &BindGroupLayoutDescriptor| cache.get_bind_group_layout(d);
    let groupes = GroupesFft {
        init: peripherique.create_bind_group(
            "ocean_fft_init",
            &disposition(&pipelines.disposition_init),
            &BindGroupEntries::with_indices(((0, liaison.clone()), (1, h0))),
        ),
        evolution: peripherique.create_bind_group(
            "ocean_fft_evolution",
            &disposition(&pipelines.disposition_evolution),
            &BindGroupEntries::with_indices(((0, liaison.clone()), (2, h0), (3, a), (4, b))),
        ),
        lignes: peripherique.create_bind_group(
            "ocean_fft_lignes",
            &disposition(&pipelines.disposition_ifft),
            &BindGroupEntries::sequential((liaison.clone(), a, b, a2, b2)),
        ),
        colonnes: peripherique.create_bind_group(
            "ocean_fft_colonnes",
            &disposition(&pipelines.disposition_ifft),
            &BindGroupEntries::sequential((liaison.clone(), a2, b2, a, b)),
        ),
        assemblage: peripherique.create_bind_group(
            "ocean_fft_assemblage",
            &disposition(&pipelines.disposition_assemblage),
            &BindGroupEntries::sequential((liaison, a, b, ecume_lue, depl, deriv, ecume_ecrite)),
        ),
    };
    rendu.groupes = Some(groupes);
    // Le spectre n'est marqué comme calculé que si toutes les passes vont réellement tourner cette image
    // (`simuler` ne fait rien tant qu'un pipeline compile encore).
    let prets = [
        pipelines.initialiser,
        pipelines.evoluer,
        pipelines.lignes,
        pipelines.colonnes,
        pipelines.assembler,
    ]
    .iter()
    .all(|&id| cache.get_compute_pipeline(id).is_some());
    rendu.initialiser = prets && rendu.generation_faite != etat.generation_spectre;
    if rendu.initialiser {
        rendu.generation_faite = etat.generation_spectre;
    }
}

fn simuler(
    mut contexte: RenderContext,
    pipelines: Res<PipelinesFft>,
    cache: Res<PipelineCache>,
    rendu: Res<EtatRenduFft>,
) {
    let Some(groupes) = &rendu.groupes else {
        return;
    };
    let (Some(init), Some(evoluer), Some(lignes), Some(colonnes), Some(assembler)) = (
        cache.get_compute_pipeline(pipelines.initialiser),
        cache.get_compute_pipeline(pipelines.evoluer),
        cache.get_compute_pipeline(pipelines.lignes),
        cache.get_compute_pipeline(pipelines.colonnes),
        cache.get_compute_pipeline(pipelines.assembler),
    ) else {
        return;
    };
    let tuiles = N_FFT.div_ceil(8);
    // Temps GPU de la passe dans les diagnostics de rendu (`render/ocean_fft/elapsed_gpu`, banc d'essai).
    let diagnostics = contexte.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let mut passe = contexte
        .command_encoder()
        .begin_compute_pass(&ComputePassDescriptor {
            label: Some("ocean_fft"),
            ..default()
        });
    let span = diagnostics.pass_span(&mut passe, "ocean_fft");
    if rendu.initialiser {
        passe.set_pipeline(init);
        passe.set_bind_group(0, &groupes.init, &[]);
        passe.dispatch_workgroups(tuiles, tuiles, CASCADES);
    }
    passe.set_pipeline(evoluer);
    passe.set_bind_group(0, &groupes.evolution, &[]);
    passe.dispatch_workgroups(tuiles, tuiles, CASCADES);
    passe.set_pipeline(lignes);
    passe.set_bind_group(0, &groupes.lignes, &[]);
    passe.dispatch_workgroups(N_FFT, 1, CASCADES);
    passe.set_pipeline(colonnes);
    passe.set_bind_group(0, &groupes.colonnes, &[]);
    passe.dispatch_workgroups(N_FFT, 1, CASCADES);
    passe.set_pipeline(assembler);
    passe.set_bind_group(0, &groupes.assemblage, &[]);
    passe.dispatch_workgroups(tuiles, tuiles, CASCADES);
    span.end(&mut passe);
}
