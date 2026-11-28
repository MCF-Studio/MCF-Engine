//! Embruns des vagues qui cassent (étape 26).
//!
//! Là où une vague de bord déferle (crête au-dessus de la limite de McCowan, voir [`crate::rivage::vagues_bord`]),
//! l'eau est projetée en gerbes de gouttelettes que le vent emporte. Une gerbe est rendue comme quelques
//! « bouffées » : des nuages de gouttelettes qui montent, s'étalent, retombent et s'effacent en une
//! demi-seconde à une seconde.
//!
//! - Simulation sur le CPU ([`Embruns`]) : à chaque image, des points tirés au hasard autour de la caméra sont
//!   testés avec la même fonction de vagues que la flottaison ; une crête qui déferle y fait naître des bouffées.
//!   Balistique simple : gravité réduite (gouttelettes fines), traînée vers la vitesse du vent.
//! - Rendu : un quad face caméra par bouffée, dans un maillage réécrit à chaque image (au plus
//!   [`MAX_EMBRUNS`] quads), mélange prémultiplié comme les éclaboussures.
//!
//! Par temps calme les vagues de bord sont trop petites : pas d'embruns. Il faut la caméra marquée
//! [`EmbrunsVagues`] et une carte du rivage ([`crate::rivage::CarteRivage`]).

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
use crate::etat::EtatMeteo;
use crate::houle::Ocean;
use crate::mer_du_vent::MerDuVent;
use crate::rivage::{CarteRivage, ParametresRivage, ParamsBord, deplacement_total, params_bord, vagues_bord};

/// Nombre maximal de bouffées vivantes.
pub const MAX_EMBRUNS: usize = 2048;
/// Essais de naissance par image, au plus (borne le coût CPU quand une image dure longtemps).
const ESSAIS_MAX_PAR_IMAGE: u32 = 256;

/// Marque la caméra autour de laquelle naissent les embruns.
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component, Default)]
pub struct EmbrunsVagues;

/// Réglages des embruns.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Reflect)]
#[reflect(Resource)]
pub struct ReglagesEmbruns {
    /// Affiche les embruns.
    pub active: bool,
    /// Rayon autour de la caméra où ils naissent (m).
    pub rayon: f32,
    /// Points testés par seconde.
    pub essais_par_s: f32,
    /// Taille d'une bouffée à sa naissance, pour une vague de 1 m d'amplitude (m).
    pub taille: f32,
    /// Opacité générale.
    pub opacite: f32,
    /// Part de la lumière reçue que les gouttelettes renvoient.
    pub albedo: f32,
}

impl Default for ReglagesEmbruns {
    fn default() -> Self {
        Self {
            active: true,
            rayon: 60.0,
            essais_par_s: 6000.0,
            taille: 0.42,
            opacite: 0.85,
            albedo: 0.9,
        }
    }
}

/// Une bouffée de gouttelettes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bouffee {
    /// Centre (m).
    pub position: Vec3,
    /// Vitesse (m/s).
    pub vitesse: Vec3,
    /// Âge (s).
    pub age: f32,
    /// Durée de vie (s).
    pub vie: f32,
    /// Demi-largeur à la naissance (m).
    pub taille: f32,
    /// Force de la gerbe (0..1) : règle l'opacité.
    pub force: f32,
    /// Graine du motif, `[0, 1)`.
    pub graine: f32,
}

/// Force des embruns selon l'amplitude des vagues de bord : nulle sous 30 cm, pleine à partir de 1 m.
pub fn force_selon_amplitude(amplitude: f32) -> f32 {
    let t = ((amplitude - 0.3) / 0.7).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Simulation des embruns.
#[derive(Resource, Debug, Clone)]
pub struct Embruns {
    bouffees: Vec<Bouffee>,
    alea: u32,
    reste: f32,
}

impl Default for Embruns {
    fn default() -> Self {
        Self {
            bouffees: Vec::with_capacity(MAX_EMBRUNS),
            alea: 0x9E37_79B9,
            reste: 0.0,
        }
    }
}

impl Embruns {
    /// Bouffées vivantes.
    pub fn bouffees(&self) -> &[Bouffee] {
        &self.bouffees
    }

    /// Supprime toutes les bouffées.
    pub fn vider(&mut self) {
        self.bouffees.clear();
    }

    /// Nombre pseudo-aléatoire dans `[0, 1)` (xorshift : reproductible, sans dépendance).
    fn hasard(&mut self) -> f32 {
        let mut x = self.alea;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.alea = x;
        (x >> 8) as f32 / 16_777_216.0
    }

    /// Fait vieillir et avancer les bouffées : gravité réduite, traînée vers la vitesse du vent. Celles qui sont
    /// mortes ou retombées sous `niveau` sont retirées.
    pub fn avancer(&mut self, dt: f32, vent: Vec3, niveau: f32) {
        let dt = if dt.is_finite() { dt.clamp(0.0, 0.1) } else { 0.0 };
        let vent = if vent.is_finite() { vent } else { Vec3::ZERO };
        // Gouttelettes fines : elles tombent bien moins vite qu'un corps en chute libre.
        const PESANTEUR: f32 = 7.0;
        const TRAINEE: f32 = 1.4;
        let suivi = 1.0 - ops::exp(-TRAINEE * dt);
        for b in &mut self.bouffees {
            b.age += dt;
            b.vitesse.y -= PESANTEUR * dt;
            b.vitesse += (vent - b.vitesse) * Vec3::new(suivi, suivi * 0.3, suivi);
            b.position += b.vitesse * dt;
        }
        self.bouffees
            .retain(|b| b.age < b.vie && b.position.y > niveau - 0.5 && b.position.is_finite());
    }

    /// Teste un point : si une crête y déferle, fait naître des bouffées. Renvoie leur nombre.
    #[allow(clippy::too_many_arguments)]
    pub fn semer_en(
        &mut self,
        p: Vec2,
        ocean: &Ocean,
        mer: Option<&MerDuVent>,
        carte: &CarteRivage,
        bord: &ParamsBord,
        vent: Vec3,
        reglages: &ReglagesEmbruns,
    ) -> u32 {
        let force_vagues = force_selon_amplitude(bord.amplitude);
        if force_vagues <= 0.0 || self.bouffees.len() >= MAX_EMBRUNS {
            return 0;
        }
        let Some(e) = carte.echantillonner(p) else {
            return 0;
        };
        let profondeur = ocean.niveau - e.fond;
        if profondeur <= 0.05 {
            return 0;
        }
        let crete = vagues_bord(p, &e, profondeur, bord, ocean.temps()).crete;
        // Seul le sommet de la crête projette de l'eau, et d'autant plus souvent que la vague est forte.
        if crete < 0.7 || self.hasard() > crete * force_vagues.max(0.35) {
            return 0;
        }
        let d = deplacement_total(ocean, mer, carte, bord, p);
        let origine = Vec3::new(p.x + d.x, ocean.niveau + d.y, p.y + d.z);
        if !origine.is_finite() {
            return 0;
        }
        // La crête avance vers la terre à la célérité de la vague.
        let celerite = bord.omega * bord.longueur / std::f32::consts::TAU;
        let avant = Vec3::new(e.normale.x, 0.0, e.normale.y);
        let nombre = 3 + (self.hasard() * 4.0 * (0.5 + force_vagues)) as u32;
        let mut nes = 0;
        for _ in 0..nombre {
            if self.bouffees.len() >= MAX_EMBRUNS {
                break;
            }
            let (a, b, c, g) = (self.hasard(), self.hasard(), self.hasard(), self.hasard());
            let cote = Vec3::new(-avant.z, 0.0, avant.x) * (a * 2.0 - 1.0) * 0.9;
            let vitesse = avant * celerite * (0.35 + 0.4 * b)
                + cote
                // Jets de 0,2 à 1 m de haut, même par beau temps : le rouleau y fait déjà 1 m.
                + Vec3::Y * (1.6 + 2.6 * c) * (0.6 + 0.4 * force_vagues)
                + vent * 0.15;
            self.bouffees.push(Bouffee {
                position: origine + cote * 0.4 + Vec3::Y * 0.05,
                vitesse,
                age: 0.0,
                vie: 0.45 + 0.55 * g,
                taille: reglages.taille.max(0.01) * bord.amplitude.clamp(0.8, 1.4) * (0.7 + 0.6 * a),
                force: (crete * (0.7 + 0.3 * force_vagues)).clamp(0.0, 1.0),
                graine: g,
            });
            nes += 1;
        }
        nes
    }

    /// Tire des points au hasard dans le disque de rayon `reglages.rayon` autour de `centre` et y sème.
    #[allow(clippy::too_many_arguments)]
    pub fn semer(
        &mut self,
        centre: Vec2,
        dt: f32,
        ocean: &Ocean,
        mer: Option<&MerDuVent>,
        carte: &CarteRivage,
        bord: &ParamsBord,
        vent: Vec3,
        reglages: &ReglagesEmbruns,
    ) {
        if !centre.is_finite() || !dt.is_finite() || dt <= 0.0 {
            return;
        }
        self.reste += reglages.essais_par_s.clamp(0.0, 100_000.0) * dt.min(0.1);
        let essais = (self.reste as u32).min(ESSAIS_MAX_PAR_IMAGE);
        self.reste = (self.reste - essais as f32).min(1.0);
        let rayon = reglages.rayon.clamp(1.0, 1000.0);
        for _ in 0..essais {
            let (r, a) = (
                ops::sqrt(self.hasard()) * rayon,
                self.hasard() * std::f32::consts::TAU,
            );
            let (s, c) = ops::sin_cos(a);
            let mut p = centre + Vec2::new(c, s) * r;
            // Les vagues ne cassent que dans une bande étroite le long du rivage : on y ramène le point tiré, le
            // long de la normale, plutôt que de gaspiller les essais au large ou à terre.
            if let Some(e) = carte.echantillonner(p)
                && e.normale != Vec2::ZERO
                && e.distance.abs() < 80.0
            {
                p += e.normale * (-(1.0 + 13.0 * self.hasard()) - e.distance);
            }
            self.semer_en(p, ocean, mer, carte, bord, vent, reglages);
        }
    }
}

/// Uniformes du dessin (même ordre que `Uniformes` dans `embruns.wesl`).
#[derive(ShaderType, Debug, Clone, Copy, Default, PartialEq)]
pub struct UniformesEmbruns {
    couleur: Vec3,
    opacite: f32,
}

/// Matériau des embruns.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub struct MateriauEmbruns {
    /// Uniformes mis à jour chaque image.
    #[uniform(0)]
    pub uniformes: UniformesEmbruns,
}

impl Material for MateriauEmbruns {
    fn vertex_shader() -> ShaderRef {
        chemin_shader()
    }

    fn fragment_shader() -> ShaderRef {
        chemin_shader()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
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
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(2),
        ])?];
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

fn chemin_shader() -> ShaderRef {
    ShaderRef::Path(
        bevy::asset::AssetPath::from_path_buf(embedded_path!("embruns.wesl")).with_source("embedded"),
    )
}

/// Entité qui porte le maillage des embruns.
#[derive(Resource, Debug, Clone)]
struct PorteurEmbruns {
    maillage: Handle<Mesh>,
    materiau: Handle<MateriauEmbruns>,
}

/// Attributs du maillage pour les bouffées données : centre, coin du quad, (âge normalisé, demi-largeur, graine,
/// force). Les quads inutilisés sont dégénérés (demi-largeur nulle).
pub fn attributs_maillage(bouffees: &[Bouffee]) -> (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[f32; 4]>) {
    const COINS: [[f32; 2]; 4] = [[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]];
    let mut positions = Vec::with_capacity(MAX_EMBRUNS * 4);
    let mut coins = Vec::with_capacity(MAX_EMBRUNS * 4);
    let mut donnees = Vec::with_capacity(MAX_EMBRUNS * 4);
    for i in 0..MAX_EMBRUNS {
        let (p, d) = match bouffees.get(i) {
            Some(b) => (
                b.position.to_array(),
                [
                    (b.age / b.vie.max(1e-3)).clamp(0.0, 1.0),
                    b.taille,
                    b.graine,
                    b.force,
                ],
            ),
            None => ([0.0; 3], [1.0, 0.0, 0.0, 0.0]),
        };
        for coin in COINS {
            positions.push(p);
            coins.push(coin);
            donnees.push(d);
        }
    }
    (positions, coins, donnees)
}

fn maillage_embruns() -> Mesh {
    let (positions, coins, donnees) = attributs_maillage(&[]);
    let mut indices = Vec::with_capacity(MAX_EMBRUNS * 6);
    for i in 0..MAX_EMBRUNS as u32 {
        let b = i * 4;
        indices.extend_from_slice(&[b, b + 1, b + 2, b + 1, b + 3, b + 2]);
    }
    // Réécrit à chaque image : il doit rester dans le monde principal.
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, coins)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, donnees)
        .with_inserted_indices(Indices::U32(indices))
}

pub(crate) fn construire(app: &mut App) {
    embedded_asset!(app, "embruns.wesl");
    app.add_plugins(MaterialPlugin::<MateriauEmbruns>::default())
        .register_type::<EmbrunsVagues>()
        .register_type::<ReglagesEmbruns>()
        .init_resource::<ReglagesEmbruns>()
        .init_resource::<Embruns>()
        .add_systems(Startup, creer_embruns)
        .add_systems(Update, mettre_a_jour_embruns.after(MiseAJourMeteo));
}

fn creer_embruns(
    mut commandes: Commands,
    mut maillages: ResMut<Assets<Mesh>>,
    mut materiaux: ResMut<Assets<MateriauEmbruns>>,
) {
    let porteur = PorteurEmbruns {
        maillage: maillages.add(maillage_embruns()),
        materiau: materiaux.add(MateriauEmbruns::default()),
    };
    commandes.spawn((
        Name::new("Embruns"),
        Mesh3d(porteur.maillage.clone()),
        MeshMaterial3d(porteur.materiau.clone()),
        Transform::IDENTITY,
        // Les bouffées se déplacent : la boîte englobante du maillage n'a pas de sens.
        NoFrustumCulling,
        NotShadowCaster,
    ));
    commandes.insert_resource(porteur);
}

/// Luminance (cd/m²) des gouttelettes : elles diffusent la lumière du ciel, plus une part du soleil direct quand
/// les nuages le laissent passer (une bouffée est à moitié transparente). `eclairement` : éclairement (lux) de la
/// lumière directionnelle la plus forte, s'il y en a une. Ordres de grandeur du ciel : 5 000 lux sous une
/// couverture totale, 25 000 lux par ciel dégagé.
pub fn luminance_embruns(meteo: &EtatMeteo, reglages: &ReglagesEmbruns, eclairement: Option<f32>) -> f32 {
    let soleil = meteo.intensite_soleil.clamp(0.0, 1.0);
    let degage = 1.0 - meteo.couverture_nuageuse.clamp(0.0, 1.0);
    let e = eclairement
        .filter(|e| e.is_finite())
        .unwrap_or(100_000.0)
        .max(0.0);
    let ciel = 5_000.0 + 20_000.0 * degage;
    let direct = e * soleil * degage * ops::sqrt(degage);
    reglages.albedo.clamp(0.0, 1.0) * (ciel + 0.3 * direct) / std::f32::consts::PI
}

#[allow(clippy::too_many_arguments)]
fn mettre_a_jour_embruns(
    temps: Res<Time>,
    meteo: Res<EtatMeteo>,
    reglages: Res<ReglagesEmbruns>,
    ocean: Res<Ocean>,
    mer: Option<Res<MerDuVent>>,
    carte: Res<CarteRivage>,
    parametres: Res<ParametresRivage>,
    cameras: Query<&GlobalTransform, With<EmbrunsVagues>>,
    lumieres: Query<&DirectionalLight>,
    porteur: Option<Res<PorteurEmbruns>>,
    mut embruns: ResMut<Embruns>,
    mut maillages: ResMut<Assets<Mesh>>,
    mut materiaux: ResMut<Assets<MateriauEmbruns>>,
    mut vide: Local<bool>,
) {
    let Some(porteur) = porteur else {
        return;
    };
    let dt = temps.delta_secs();
    let vent = meteo.vent();
    let camera = cameras.iter().next().map(GlobalTransform::translation);
    match camera {
        Some(c) if reglages.active => {
            let bord = params_bord(&meteo, &parametres);
            embruns.avancer(dt, vent, ocean.niveau);
            embruns.semer(c.xz(), dt, &ocean, mer.as_deref(), &carte, &bord, vent, &reglages);
        }
        _ => embruns.vider(),
    }
    // Rien à dessiner, et le maillage est déjà vide : on ne le réécrit pas.
    if embruns.bouffees().is_empty() && *vide {
        return;
    }
    *vide = embruns.bouffees().is_empty();
    if let Some(mut maillage) = maillages.get_mut(&porteur.maillage) {
        let (positions, coins, donnees) = attributs_maillage(embruns.bouffees());
        maillage.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        maillage.insert_attribute(Mesh::ATTRIBUTE_UV_0, coins);
        maillage.insert_attribute(Mesh::ATTRIBUTE_COLOR, donnees);
    }
    let eclairement = lumieres.iter().map(|l| l.illuminance).reduce(f32::max);
    let uniformes = UniformesEmbruns {
        couleur: Vec3::splat(luminance_embruns(&meteo, &reglages, eclairement)),
        opacite: reglages.opacite.clamp(0.0, 1.0),
    };
    if materiaux
        .get(&porteur.materiau)
        .is_some_and(|m| m.uniformes != uniformes)
        && let Some(mut m) = materiaux.get_mut(&porteur.materiau)
    {
        m.uniformes = uniformes;
    }
}
