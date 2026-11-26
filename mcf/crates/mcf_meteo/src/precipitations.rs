//! Pluie et neige sur le GPU (étape 3), portées de SkyForge.
//!
//! Particules sans état, calculées entièrement dans le vertex shader (`precipitations.wesl`) : un seul maillage
//! de quads, aucune simulation par image, aucun tampon à relire. Les gouttes vivent dans une boîte toroïdale
//! centrée sur la caméra qui affiche le rendu ; le CPU ne fournit que quelques uniformes (vent, temps, quantité).
//!
//! Les entités marquées [`AbriPrecipitations`] (toits, ponts, véhicules) arrêtent la pluie et la neige : voir
//! [`crate::abris`].

use bevy::asset::{RenderAssetUsages, embedded_asset, embedded_path};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

use crate::MiseAJourMeteo;
use crate::abris::{self, AbriPrecipitations, CarteAbris};
use crate::etat::EtatMeteo;
use crate::vent::ChampVent;

/// Période de bouclage des particules (s). Toutes les vitesses de chute sont des multiples de
/// `hauteur de boîte / PERIODE_S`, donc chaque goutte revient exactement à sa place au bout de la période.
pub const PERIODE_S: f32 = 600.0;

/// Réglages de la pluie et de la neige (qualité et aspect).
#[derive(Resource, Reflect, Debug, Clone, PartialEq)]
#[reflect(Resource, Default)]
pub struct ReglagesPrecipitations {
    /// Nombre maximal de particules (fixé à la création du maillage).
    pub max_particules: u32,
    /// Demi-largeur horizontale de la boîte autour de la caméra (m).
    pub demi_largeur: f32,
    /// Hauteur de la boîte (m).
    pub hauteur: f32,
    /// Durée d'exposition simulée (s) : longueur des traînées de pluie.
    pub trainee: f32,
    /// Multiplicateur d'opacité.
    pub luminosite: f32,
    /// Luminance des gouttes par ciel dégagé (cd/m², multipliée par l'exposition de la caméra).
    pub luminance: f32,
}

impl Default for ReglagesPrecipitations {
    fn default() -> Self {
        Self {
            max_particules: 49_152,
            demi_largeur: 35.0,
            hauteur: 30.0,
            trainee: 0.018,
            luminosite: 1.0,
            luminance: 2500.0,
        }
    }
}

/// Uniformes envoyés au shader (même ordre que `Uniformes` dans `precipitations.wesl`).
#[derive(ShaderType, Debug, Clone, Copy, Default, PartialEq)]
pub struct UniformesPrecipitations {
    decalage_vent: Vec3,
    temps: f32,
    boite: Vec3,
    actives: f32,
    vent: Vec3,
    facteur_neige: f32,
    couleur: Vec3,
    trainee: f32,
    luminosite: f32,
    quantum_vitesse: f32,
    periode: f32,
    remplissage: f32,
    /// Carte des abris : origine x, origine z, côté d'une case, nombre de cases par côté.
    abris: Vec4,
}

/// Matériau des précipitations : tout le calcul est dans le shader.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub struct MateriauPrecipitations {
    /// Uniformes mis à jour chaque image.
    #[uniform(0)]
    pub uniformes: UniformesPrecipitations,
    /// Carte des abris (R32Float, lue sans filtrage).
    #[texture(1, filterable = false)]
    pub carte_abris: Handle<Image>,
}

impl Material for MateriauPrecipitations {
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
        // Seule la position (coin + indice de particule) est lue.
        descriptor.vertex.buffers = vec![
            layout
                .0
                .get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?,
        ];
        // Traînées vues de face ou de dos : pas d'élimination des faces arrière.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

fn chemin_shader() -> ShaderRef {
    ShaderRef::Path(
        bevy::asset::AssetPath::from_path_buf(embedded_path!("precipitations.wesl")).with_source("embedded"),
    )
}

/// État du transport des particules par le vent, bouclé sur la boîte.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct EtatPrecipitations {
    decalage_vent: Vec3,
    temps: f32,
    entite: Option<Entity>,
}

/// Construit le maillage : un quad par particule, attribut position = (coin.x, coin.y, indice).
pub fn maillage_particules(nombre: u32) -> Mesh {
    let n = nombre as usize;
    let mut positions = Vec::with_capacity(n * 4);
    let mut indices = Vec::with_capacity(n * 6);
    for i in 0..nombre {
        // L'indice est stocké en f32 : exact jusqu'à 2^24 particules.
        let k = i as f32;
        positions.extend_from_slice(&[[-1.0, 0.0, k], [1.0, 0.0, k], [-1.0, 1.0, k], [1.0, 1.0, k]]);
        let b = i * 4;
        indices.extend_from_slice(&[b, b + 1, b + 2, b + 1, b + 3, b + 2]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_indices(Indices::U32(indices))
}

/// Plugin de rendu de la pluie et de la neige. Nécessite [`crate::McfMeteoPlugin`] et le rendu 3D de Bevy.
pub struct McfPrecipitationsPlugin;

impl Plugin for McfPrecipitationsPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "precipitations.wesl");
        app.add_plugins(MaterialPlugin::<MateriauPrecipitations>::default())
            .register_type::<ReglagesPrecipitations>()
            .register_type::<AbriPrecipitations>()
            .init_resource::<ReglagesPrecipitations>()
            .init_resource::<EtatPrecipitations>()
            .add_systems(Startup, creer_precipitations)
            .add_systems(
                Update,
                (abris::mettre_a_jour_carte_abris, mettre_a_jour_precipitations)
                    .chain()
                    .after(MiseAJourMeteo),
            );
    }

    fn finish(&self, app: &mut App) {
        abris::creer_carte_abris(app.world_mut());
    }
}

fn creer_precipitations(
    mut commandes: Commands,
    reglages: Res<ReglagesPrecipitations>,
    mut etat: ResMut<EtatPrecipitations>,
    mut maillages: ResMut<Assets<Mesh>>,
    mut materiaux: ResMut<Assets<MateriauPrecipitations>>,
    carte: Res<CarteAbris>,
) {
    let entite = commandes
        .spawn((
            Name::new("Précipitations"),
            Mesh3d(maillages.add(maillage_particules(reglages.max_particules))),
            MeshMaterial3d(materiaux.add(MateriauPrecipitations {
                uniformes: UniformesPrecipitations::default(),
                carte_abris: carte.image.clone(),
            })),
            Transform::IDENTITY,
            // Les particules suivent la caméra dans le shader : la boîte englobante du maillage n'a pas de sens.
            NoFrustumCulling,
            NotShadowCaster,
        ))
        .id();
    etat.entite = Some(entite);
}

/// Calcule les uniformes de l'image (fonction pure, testable sans GPU).
pub fn calculer_uniformes(
    meteo: &EtatMeteo,
    vent_local: Vec3,
    reglages: &ReglagesPrecipitations,
    decalage_vent: Vec3,
    temps: f32,
) -> UniformesPrecipitations {
    let boite = Vec3::new(
        reglages.demi_largeur * 2.0,
        reglages.hauteur,
        reglages.demi_largeur * 2.0,
    );
    let actives = (meteo.precipitations.clamp(0.0, 1.0) * reglages.max_particules as f32).floor();
    UniformesPrecipitations {
        decalage_vent,
        temps,
        boite,
        actives,
        vent: vent_local,
        facteur_neige: meteo.facteur_neige(),
        couleur: Vec3::splat(reglages.luminance * (0.35 + 0.65 * meteo.intensite_soleil)),
        trainee: reglages.trainee,
        luminosite: reglages.luminosite,
        quantum_vitesse: reglages.hauteur / PERIODE_S,
        periode: PERIODE_S,
        remplissage: 0.0,
        abris: Vec4::new(0.0, 0.0, 1.0, 0.0),
    }
}

impl EtatPrecipitations {
    /// Avance le transport : déplacement par le vent bouclé sur la boîte, temps bouclé sur la période.
    pub fn avancer(&mut self, vent: Vec3, boite: Vec3, dt: f32) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let horizontal = Vec3::new(vent.x, 0.0, vent.z) * dt;
        self.decalage_vent = (self.decalage_vent + horizontal).rem_euclid(boite.max(Vec3::ONE));
        self.temps = (self.temps + dt).rem_euclid(PERIODE_S);
    }

    /// Déplacement cumulé par le vent (m).
    pub fn decalage_vent(&self) -> Vec3 {
        self.decalage_vent
    }

    /// Temps bouclé (s).
    pub fn temps(&self) -> f32 {
        self.temps
    }
}

fn mettre_a_jour_precipitations(
    meteo: Res<EtatMeteo>,
    champ: Res<ChampVent>,
    reglages: Res<ReglagesPrecipitations>,
    mut etat: ResMut<EtatPrecipitations>,
    temps: Res<Time>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    porteurs: Query<&MeshMaterial3d<MateriauPrecipitations>>,
    mut materiaux: ResMut<Assets<MateriauPrecipitations>>,
    carte: Option<Res<CarteAbris>>,
) {
    // Vent mesuré à la première caméra 3D (la pluie l'entoure) ; à l'origine s'il n'y en a pas.
    let centre = cameras.iter().next().map_or(Vec3::ZERO, |c| c.translation());
    let vent_local = champ.echantillonner(&meteo, centre).vitesse;
    let u0 = calculer_uniformes(&meteo, vent_local, &reglages, Vec3::ZERO, 0.0);
    etat.avancer(vent_local, u0.boite, temps.delta_secs());

    let Some(porteur) = etat.entite.and_then(|e| porteurs.get(e).ok()) else {
        return;
    };
    let uniformes = UniformesPrecipitations {
        decalage_vent: etat.decalage_vent,
        temps: etat.temps,
        abris: carte.map_or(u0.abris, |c| {
            Vec4::new(
                c.grille.origine.x,
                c.grille.origine.y,
                c.grille.texel,
                c.grille.taille as f32,
            )
        }),
        ..u0
    };
    // Ne touche l'asset que s'il change (sinon Bevy le renverrait au GPU pour rien).
    if materiaux
        .get(&porteur.0)
        .is_some_and(|m| m.uniformes != uniformes)
        && let Some(mut m) = materiaux.get_mut(&porteur.0)
    {
        m.uniformes = uniformes;
    }
}
