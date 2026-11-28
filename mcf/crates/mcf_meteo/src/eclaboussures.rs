//! Éclaboussures de la pluie (étape 20) : une petite couronne d'eau là où chaque goutte touche ce que la caméra
//! voit — sol, objets, mer — sans rien savoir de la scène.
//!
//! Entre la passe opaque et la passe transparente, une passe de calcul descend la verticale de chaque éclaboussure
//! et la compare au tampon de profondeur de la vue (`eclaboussures_calcul.wesl`) : le point où elle passe
//! derrière la géométrie est l'impact. Les points sont écrits dans une petite texture, lue par un matériau
//! transparent ordinaire qui dessine les couronnes (`eclaboussures.wesl`), testées en profondeur comme le reste.
//!
//! À poser sur la caméra : [`EclaboussuresPluie`]. Une seule caméra est servie (la texture est commune).
//! Limites : seuls les impacts visibles à l'écran existent, caméra en perspective seulement, pas d'éclaboussures
//! pour la neige.

use std::borrow::Cow;

use bevy::asset::{RenderAssetUsages, embedded_asset, embedded_path, load_embedded_asset};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::core_pipeline::core_3d::{main_opaque_pass_3d, main_transparent_pass_3d};
use bevy::core_pipeline::schedule::Core3d;
use bevy::light::NotShadowCaster;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    texture_2d, texture_depth_2d, texture_depth_2d_multisampled, texture_storage_2d, uniform_buffer,
};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::texture::GpuImage;
use bevy::render::view::{Msaa, ViewDepthStencilTexture, ViewUniform, ViewUniformOffset, ViewUniforms};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::ShaderRef;

use crate::MiseAJourMeteo;
use crate::abris::CarteAbris;
use crate::etat::EtatMeteo;
use crate::precipitations::{
    EtatPrecipitations, ReglagesPrecipitations, luminance_precipitations, maillage_particules,
};

/// Côté de la texture des impacts : `COTE × COTE` éclaboussures au plus.
pub const COTE_IMPACTS: u32 = 96;
/// Nombre maximal d'éclaboussures vivantes.
pub const MAX_ECLABOUSSURES: u32 = COTE_IMPACTS * COTE_IMPACTS;
/// Durée de vie d'une éclaboussure (s). Divise [`PERIODE_S`] : le temps bouclé ne fait pas sauter les cycles.
pub const VIE_S: f32 = 0.3;

/// À poser sur une `Camera3d` pour y voir les éclaboussures de la pluie : rend son tampon de profondeur lisible
/// par la passe qui cherche les points d'impact.
#[derive(Component, Reflect, Clone, Copy, Debug, Default, PartialEq, Eq, ExtractComponent)]
#[extract_app(RenderApp)]
#[reflect(Component, Default)]
pub struct EclaboussuresPluie;

/// Réglages des éclaboussures.
#[derive(Resource, Reflect, Debug, Clone, PartialEq)]
#[reflect(Resource, Default)]
pub struct ReglagesEclaboussures {
    /// Éclaboussures actives.
    pub active: bool,
    /// Demi-côté de la zone autour de la caméra où les éclaboussures existent (m).
    pub demi_largeur: f32,
    /// Demi-largeur d'une couronne à pleine ouverture (m).
    pub taille: f32,
    /// Multiplicateur d'opacité.
    pub luminosite: f32,
}

impl Default for ReglagesEclaboussures {
    fn default() -> Self {
        Self {
            active: true,
            demi_largeur: 12.0,
            taille: 0.045,
            luminosite: 1.0,
        }
    }
}

/// Paramètres de la passe de calcul (même ordre que `Params` dans `eclaboussures_calcul.wesl`).
#[derive(Resource, ShaderType, Clone, Copy, Debug, Default, PartialEq, ExtractResource)]
#[extract_app(RenderApp)]
pub struct ParamsEclaboussures {
    /// Temps bouclé (s).
    pub temps: f32,
    /// Nombre d'éclaboussures vivantes (0 : rien à calculer).
    pub actives: f32,
    /// Demi-côté de la zone (m).
    pub demi_largeur: f32,
    /// Durée de vie (s).
    pub vie: f32,
    /// Carte des abris : origine x, origine z, côté d'une case, cases par côté (0 : pas de carte).
    pub abris: Vec4,
}

/// Nombre d'éclaboussures vivantes : proportionnel à la pluie, aucune pour la part de neige.
pub fn eclaboussures_actives(meteo: &EtatMeteo, reglages: &ReglagesEclaboussures) -> f32 {
    if !reglages.active {
        return 0.0;
    }
    (meteo.pluie().clamp(0.0, 1.0) * MAX_ECLABOUSSURES as f32).floor()
}

/// Paramètres de l'image (fonction pure).
pub fn calculer_params(
    meteo: &EtatMeteo,
    reglages: &ReglagesEclaboussures,
    temps: f32,
) -> ParamsEclaboussures {
    ParamsEclaboussures {
        temps,
        actives: eclaboussures_actives(meteo, reglages),
        demi_largeur: reglages.demi_largeur.max(1.0),
        vie: VIE_S,
        abris: Vec4::new(0.0, 0.0, 1.0, 0.0),
    }
}

/// Texture des points d'impact (xyz = point, w = âge normalisé ou négatif), écrite par le GPU à chaque image.
#[derive(Resource, Clone, Debug, ExtractResource)]
#[extract_app(RenderApp)]
pub struct TextureImpacts(pub Handle<Image>);

/// Carte des abris, pour la passe de calcul (pas d'impact sous un toit déclaré).
#[derive(Resource, Clone, Debug, ExtractResource)]
#[extract_app(RenderApp)]
struct AbrisImpacts(Handle<Image>);

impl FromWorld for TextureImpacts {
    fn from_world(monde: &mut World) -> Self {
        // Départ : aucun impact (w = −1).
        let texel: Vec<u8> = [0.0f32, 0.0, 0.0, -1.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let mut image = Image::new_fill(
            Extent3d {
                width: COTE_IMPACTS,
                height: COTE_IMPACTS,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &texel,
            TextureFormat::Rgba32Float,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.usage =
            TextureUsages::COPY_DST | TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING;
        Self(monde.resource_mut::<Assets<Image>>().add(image))
    }
}

/// Uniformes du dessin (même ordre que `Uniformes` dans `eclaboussures.wesl`).
#[derive(ShaderType, Debug, Clone, Copy, Default, PartialEq)]
pub struct UniformesEclaboussures {
    couleur: Vec3,
    luminosite: f32,
    taille: f32,
    demi_largeur: f32,
    actives: f32,
    remplissage: f32,
}

/// Matériau des éclaboussures.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub struct MateriauEclaboussures {
    /// Uniformes mis à jour chaque image.
    #[uniform(0)]
    pub uniformes: UniformesEclaboussures,
    /// Points d'impact (Rgba32Float, lue sans filtrage).
    #[texture(1, filterable = false)]
    pub impacts: Handle<Image>,
}

impl Material for MateriauEclaboussures {
    fn vertex_shader() -> ShaderRef {
        chemin_shader()
    }

    fn fragment_shader() -> ShaderRef {
        chemin_shader()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![
            layout
                .0
                .get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?,
        ];
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

fn chemin_shader() -> ShaderRef {
    ShaderRef::Path(
        bevy::asset::AssetPath::from_path_buf(embedded_path!("eclaboussures.wesl")).with_source("embedded"),
    )
}

/// Entité qui porte le maillage des éclaboussures.
#[derive(Resource, Debug, Clone, Copy)]
struct PorteurEclaboussures(Entity);

pub(crate) fn construire(app: &mut App) {
    embedded_asset!(app, "eclaboussures.wesl");
    embedded_asset!(app, "eclaboussures_calcul.wesl");
    app.add_plugins((
        MaterialPlugin::<MateriauEclaboussures>::default(),
        ExtractComponentPlugin::<EclaboussuresPluie>::default(),
        ExtractResourcePlugin::<ParamsEclaboussures>::default(),
        ExtractResourcePlugin::<TextureImpacts>::default(),
        ExtractResourcePlugin::<AbrisImpacts>::default(),
    ))
    .register_type::<EclaboussuresPluie>()
    .register_type::<ReglagesEclaboussures>()
    .init_resource::<ReglagesEclaboussures>()
    .init_resource::<ParamsEclaboussures>()
    .init_resource::<TextureImpacts>()
    .add_systems(Startup, creer_eclaboussures)
    .add_systems(
        Update,
        (preparer_cameras, mettre_a_jour_eclaboussures)
            .after(MiseAJourMeteo)
            .after(crate::precipitations::mettre_a_jour_precipitations),
    );
}

pub(crate) fn finir(app: &mut App) {
    let Some(rendu) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    rendu
        .add_systems(RenderStartup, creer_ressources)
        .add_systems(Render, preparer_groupes.in_set(RenderSystems::PrepareBindGroups))
        .add_systems(
            Core3d,
            calculer_impacts
                .after(main_opaque_pass_3d)
                .before(main_transparent_pass_3d),
        );
}

fn creer_eclaboussures(
    mut commandes: Commands,
    mut maillages: ResMut<Assets<Mesh>>,
    mut materiaux: ResMut<Assets<MateriauEclaboussures>>,
    impacts: Res<TextureImpacts>,
    abris: Res<CarteAbris>,
) {
    commandes.insert_resource(AbrisImpacts(abris.image.clone()));
    let entite = commandes
        .spawn((
            Name::new("Éclaboussures"),
            Mesh3d(maillages.add(maillage_particules(MAX_ECLABOUSSURES))),
            MeshMaterial3d(materiaux.add(MateriauEclaboussures {
                uniformes: UniformesEclaboussures::default(),
                impacts: impacts.0.clone(),
            })),
            Transform::IDENTITY,
            // Les éclaboussures suivent la caméra dans le shader : la boîte englobante n'a pas de sens.
            NoFrustumCulling,
            NotShadowCaster,
        ))
        .id();
    commandes.insert_resource(PorteurEclaboussures(entite));
}

/// La passe de calcul lit le tampon de profondeur : il doit être échantillonnable.
fn preparer_cameras(mut cameras: Query<&mut Camera3d, With<EclaboussuresPluie>>) {
    for mut camera in &mut cameras {
        let usages = TextureUsages::from(camera.depth_texture_usages);
        if !usages.contains(TextureUsages::TEXTURE_BINDING) {
            camera.depth_texture_usages = (usages | TextureUsages::TEXTURE_BINDING).into();
        }
    }
}

fn mettre_a_jour_eclaboussures(
    meteo: Res<EtatMeteo>,
    reglages: Res<ReglagesEclaboussures>,
    reglages_pluie: Res<ReglagesPrecipitations>,
    etat: Res<EtatPrecipitations>,
    lumieres: Query<&DirectionalLight>,
    porteur: Option<Res<PorteurEclaboussures>>,
    porteurs: Query<&MeshMaterial3d<MateriauEclaboussures>>,
    mut materiaux: ResMut<Assets<MateriauEclaboussures>>,
    mut params: ResMut<ParamsEclaboussures>,
    abris: Option<Res<CarteAbris>>,
) {
    let mut nouveaux = calculer_params(&meteo, &reglages, etat.temps());
    if let Some(c) = abris {
        nouveaux.abris = Vec4::new(
            c.grille.origine.x,
            c.grille.origine.y,
            c.grille.texel,
            c.grille.taille as f32,
        );
    }
    if *params != nouveaux {
        *params = nouveaux;
    }
    let Some(porteur) = porteur.and_then(|p| porteurs.get(p.0).ok()) else {
        return;
    };
    let eclairement = lumieres.iter().map(|l| l.illuminance).reduce(f32::max);
    let uniformes = UniformesEclaboussures {
        couleur: Vec3::splat(luminance_precipitations(&meteo, &reglages_pluie, eclairement)),
        luminosite: reglages.luminosite.max(0.0),
        taille: reglages.taille.max(0.001),
        demi_largeur: nouveaux.demi_largeur,
        actives: nouveaux.actives,
        remplissage: 0.0,
    };
    if materiaux
        .get(&porteur.0)
        .is_some_and(|m| m.uniformes != uniformes)
        && let Some(mut m) = materiaux.get_mut(&porteur.0)
    {
        m.uniformes = uniformes;
    }
}

#[derive(Clone)]
struct VarianteImpacts {
    disposition: BindGroupLayoutDescriptor,
    pipeline: CachedComputePipelineId,
}

/// Ressources de rendu : une variante de pipeline par nombre d'échantillons MSAA de la profondeur.
#[derive(Resource)]
struct RessourcesImpacts {
    shader: Handle<Shader>,
    variantes: HashMap<u32, VarianteImpacts>,
}

/// Paramètres de la passe sur le GPU, et nombre d'éclaboussures de l'image (0 : passe sautée).
#[derive(Resource, Default)]
struct TamponImpacts {
    tampon: UniformBuffer<ParamsEclaboussures>,
    actives: f32,
}

impl RessourcesImpacts {
    fn variante(&mut self, cache: &PipelineCache, echantillons: u32) -> &VarianteImpacts {
        let shader = self.shader.clone();
        self.variantes.entry(echantillons).or_insert_with(|| {
            let multi = echantillons > 1;
            let disposition = BindGroupLayoutDescriptor::new(
                if multi {
                    "eclaboussures_impacts_msaa"
                } else {
                    "eclaboussures_impacts"
                },
                &BindGroupLayoutEntries::sequential(
                    ShaderStages::COMPUTE,
                    (
                        uniform_buffer::<ViewUniform>(true),
                        uniform_buffer::<ParamsEclaboussures>(false),
                        if multi {
                            texture_depth_2d_multisampled()
                        } else {
                            texture_depth_2d()
                        },
                        texture_storage_2d(TextureFormat::Rgba32Float, StorageTextureAccess::WriteOnly),
                        texture_2d(TextureSampleType::Float { filterable: false }),
                    ),
                ),
            );
            let pipeline = cache.queue_compute_pipeline(ComputePipelineDescriptor {
                label: Some(Cow::Borrowed("eclaboussures_impacts")),
                layout: vec![disposition.clone()],
                shader,
                shader_defs: if multi {
                    vec!["MULTISAMPLED".into()]
                } else {
                    Vec::new()
                },
                entry_point: Some(Cow::Borrowed("calculer")),
                ..default()
            });
            VarianteImpacts {
                disposition,
                pipeline,
            }
        })
    }
}

fn creer_ressources(mut commandes: Commands, serveur: Res<AssetServer>) {
    commandes.insert_resource(RessourcesImpacts {
        shader: load_embedded_asset!(serveur.as_ref(), "eclaboussures_calcul.wesl"),
        variantes: HashMap::default(),
    });
    commandes.init_resource::<TamponImpacts>();
}

/// Groupe de liaison de la passe de calcul pour une vue.
#[derive(Component)]
struct GroupeImpacts {
    groupe: BindGroup,
    echantillons: u32,
}

fn preparer_groupes(
    mut commandes: Commands,
    mut ressources: ResMut<RessourcesImpacts>,
    mut tampon: ResMut<TamponImpacts>,
    cache: Res<PipelineCache>,
    peripherique: Res<RenderDevice>,
    file: Res<RenderQueue>,
    uniformes_vues: Res<ViewUniforms>,
    params: Option<Res<ParamsEclaboussures>>,
    texture: Option<Res<TextureImpacts>>,
    abris: Option<Res<AbrisImpacts>>,
    images: Res<RenderAssets<GpuImage>>,
    vues: Query<(Entity, &ViewDepthStencilTexture, &Msaa), With<EclaboussuresPluie>>,
) {
    tampon.actives = 0.0;
    let (Some(params), Some(texture), Some(abris)) = (params, texture, abris) else {
        return;
    };
    let (Some(image), Some(image_abris)) = (images.get(&texture.0), images.get(&abris.0)) else {
        return;
    };
    tampon.tampon.set(*params);
    tampon.tampon.write_buffer(&peripherique, &file);
    tampon.actives = params.actives;
    let (Some(liaison_vue), Some(liaison_params)) =
        (uniformes_vues.uniforms.binding(), tampon.tampon.binding())
    else {
        return;
    };
    for (entite, profondeur, msaa) in &vues {
        let disposition = ressources.variante(&cache, msaa.samples()).disposition.clone();
        let Some(vue_profondeur) = profondeur.attachment.depth_stencil_views().depth_only_view() else {
            continue;
        };
        let groupe = peripherique.create_bind_group(
            "eclaboussures_impacts",
            &cache.get_bind_group_layout(&disposition),
            &BindGroupEntries::sequential((
                liaison_vue.clone(),
                liaison_params.clone(),
                vue_profondeur,
                &image.texture_view,
                &image_abris.texture_view,
            )),
        );
        commandes.entity(entite).insert(GroupeImpacts {
            groupe,
            echantillons: msaa.samples(),
        });
    }
}

fn calculer_impacts(
    vue: ViewQuery<(&ViewUniformOffset, &GroupeImpacts)>,
    ressources: Option<Res<RessourcesImpacts>>,
    tampon: Option<Res<TamponImpacts>>,
    cache: Res<PipelineCache>,
    mut contexte: RenderContext,
) {
    let (Some(ressources), Some(tampon)) = (ressources, tampon) else {
        return;
    };
    // Sans pluie, rien à calculer : le dessin ignore la texture (`actives = 0`).
    if tampon.actives <= 0.0 {
        return;
    }
    let (decalage_vue, groupe) = vue.into_inner();
    let Some(pipeline) = ressources
        .variantes
        .get(&groupe.echantillons)
        .and_then(|v| cache.get_compute_pipeline(v.pipeline))
    else {
        return;
    };
    let diagnostics = contexte.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let mut passe = contexte
        .command_encoder()
        .begin_compute_pass(&ComputePassDescriptor {
            label: Some("pluie_impacts"),
            ..default()
        });
    let span = diagnostics.pass_span(&mut passe, "pluie_impacts");
    passe.set_pipeline(pipeline);
    passe.set_bind_group(0, &groupe.groupe, &[decalage_vue.offset]);
    passe.dispatch_workgroups(COTE_IMPACTS / 8, COTE_IMPACTS / 8, 1);
    span.end(&mut passe);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etat::PresetMeteo;
    use crate::precipitations::PERIODE_S;

    #[test]
    fn vie_divise_la_periode() {
        let cycles = PERIODE_S / VIE_S;
        assert!(
            (cycles - cycles.round()).abs() < 1e-3,
            "{cycles} cycles par période"
        );
    }

    #[test]
    fn eclaboussures_selon_la_pluie() {
        let reglages = ReglagesEclaboussures::default();
        let clair = PresetMeteo::Clair.etat();
        let pluie = PresetMeteo::Pluie.etat();
        let tempete = PresetMeteo::Tempete.etat();
        let neige = PresetMeteo::Neige.etat();
        assert_eq!(eclaboussures_actives(&clair, &reglages), 0.0);
        assert!(eclaboussures_actives(&pluie, &reglages) > 0.0);
        assert!(eclaboussures_actives(&tempete, &reglages) >= eclaboussures_actives(&pluie, &reglages));
        assert!(eclaboussures_actives(&tempete, &reglages) <= MAX_ECLABOUSSURES as f32);
        // La neige n'éclabousse pas.
        assert_eq!(eclaboussures_actives(&neige, &reglages), 0.0);
        // Désactivé : rien.
        let coupe = ReglagesEclaboussures {
            active: false,
            ..default()
        };
        assert_eq!(eclaboussures_actives(&tempete, &coupe), 0.0);
        assert_eq!(calculer_params(&tempete, &reglages, 12.0).vie, VIE_S);
    }
}
