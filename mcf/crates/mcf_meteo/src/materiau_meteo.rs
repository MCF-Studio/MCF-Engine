//! Matériau « météo » : le `StandardMaterial` de Bevy, mouillé, avec flaques et neige selon [`SurfacesMeteo`].
//!
//! Utilisation : remplacer `MeshMaterial3d<StandardMaterial>` par `MeshMaterial3d<MateriauMeteo>` sur les
//! surfaces qui doivent réagir à la météo (sol, routes, toits, rochers), avec [`materiau_meteo`]. Les valeurs
//! globales sont recopiées dans chaque matériau seulement quand elles changent de façon visible.

use bevy::asset::{AssetPath, embedded_asset, embedded_path};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use crate::MiseAJourMeteo;
use crate::abris::CarteAbris;
use crate::etat::EtatMeteo;
use crate::surfaces::SurfacesMeteo;

/// `StandardMaterial` + extension météo.
pub type MateriauMeteo = ExtendedMaterial<StandardMaterial, ExtensionMeteo>;

/// Uniformes de l'extension (même ordre que `ExtensionMeteo` dans `materiau_meteo.wesl`).
#[derive(ShaderType, Reflect, Debug, Clone, Copy, PartialEq)]
pub struct UniformesSurface {
    /// Sol mouillé, `[0, 1]`.
    pub mouillage: f32,
    /// Remplissage des flaques, `[0, 1]`.
    pub flaques: f32,
    /// Couverture de neige, `[0, 1]`.
    pub neige: f32,
    /// Intensité de la pluie (ondulations), `[0, 1]`.
    pub pluie: f32,
    /// Porosité du matériau : 0 = métal / verre, 1 = très poreux (terre, béton brut).
    pub porosite: f32,
    /// 0 = jamais de flaques (murs, toits en pente), 1 = sol.
    pub reception_flaques: f32,
    remplissage0: f32,
    remplissage1: f32,
    /// Carte des abris : origine x, origine z, côté d'une case, cases par côté (0 = pas de carte).
    pub abris: Vec4,
}

impl Default for UniformesSurface {
    fn default() -> Self {
        Self {
            mouillage: 0.0,
            flaques: 0.0,
            neige: 0.0,
            pluie: 0.0,
            porosite: 0.5,
            reception_flaques: 1.0,
            remplissage0: 0.0,
            remplissage1: 0.0,
            abris: Vec4::new(0.0, 0.0, 1.0, 0.0),
        }
    }
}

/// Extension météo du `StandardMaterial`.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct ExtensionMeteo {
    /// Uniformes (mis à jour par le plugin).
    #[uniform(100)]
    pub uniformes: UniformesSurface,
    /// Carte des abris (R32Float, lue sans filtrage).
    #[texture(101, filterable = false)]
    pub carte_abris: Handle<Image>,
}

impl MaterialExtension for ExtensionMeteo {
    fn fragment_shader() -> ShaderRef {
        chemin_shader()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        chemin_shader()
    }
}

fn chemin_shader() -> ShaderRef {
    ShaderRef::Path(AssetPath::from_path_buf(embedded_path!("materiau_meteo.wesl")).with_source("embedded"))
}

/// Crée un matériau météo à partir d'un `StandardMaterial`.
/// `porosite` : 0 = métal / verre, 1 = terre. `reception_flaques` : 0 pour les murs et toits en pente, 1 pour le sol.
pub fn materiau_meteo(
    base: StandardMaterial,
    porosite: f32,
    reception_flaques: f32,
    carte: &CarteAbris,
) -> MateriauMeteo {
    MateriauMeteo {
        base,
        extension: ExtensionMeteo {
            uniformes: UniformesSurface {
                porosite: porosite.clamp(0.0, 1.0),
                reception_flaques: reception_flaques.clamp(0.0, 1.0),
                ..default()
            },
            carte_abris: carte.image.clone(),
        },
    }
}

/// Écart en dessous duquel on ne renvoie pas le matériau au GPU (1/256 : invisible à l'œil).
const SEUIL: f32 = 1.0 / 256.0;

/// Recopie les valeurs globales dans les uniformes d'un matériau. Renvoie vrai si quelque chose a changé de façon
/// visible.
pub fn appliquer(u: &mut UniformesSurface, surfaces: &SurfacesMeteo, pluie: f32, abris: Vec4) -> bool {
    let proche = |a: f32, b: f32| (a - b).abs() < SEUIL;
    if proche(u.mouillage, surfaces.mouillage)
        && proche(u.flaques, surfaces.flaques)
        && proche(u.neige, surfaces.neige)
        && proche(u.pluie, pluie)
        && u.abris == abris
    {
        return false;
    }
    u.mouillage = surfaces.mouillage;
    u.flaques = surfaces.flaques;
    u.neige = surfaces.neige;
    u.pluie = pluie;
    u.abris = abris;
    true
}

/// Plugin du matériau météo. Nécessite [`crate::McfMeteoPlugin`] et [`crate::McfPrecipitationsPlugin`] (carte des
/// abris).
pub struct McfMateriauMeteoPlugin;

impl Plugin for McfMateriauMeteoPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "materiau_meteo.wesl");
        app.add_plugins(MaterialPlugin::<MateriauMeteo>::default())
            .register_type::<ExtensionMeteo>()
            .add_systems(Update, mettre_a_jour_materiaux.after(MiseAJourMeteo));
    }
}

fn mettre_a_jour_materiaux(
    surfaces: Res<SurfacesMeteo>,
    etat: Res<EtatMeteo>,
    carte: Option<Res<CarteAbris>>,
    mut materiaux: ResMut<Assets<MateriauMeteo>>,
) {
    let abris = carte.as_ref().map_or(Vec4::new(0.0, 0.0, 1.0, 0.0), |c| {
        Vec4::new(
            c.grille.origine.x,
            c.grille.origine.y,
            c.grille.texel,
            c.grille.taille as f32,
        )
    });
    let pluie = etat.pluie();
    // Repère d'abord les matériaux à changer (`iter_mut` les marquerait tous comme modifiés).
    let a_changer: Vec<AssetId<MateriauMeteo>> = materiaux
        .iter()
        .filter(|(_, m)| {
            let mut u = m.extension.uniformes;
            appliquer(&mut u, &surfaces, pluie, abris)
        })
        .map(|(id, _)| id)
        .collect();
    for id in a_changer {
        if let Some(mut m) = materiaux.get_mut(id) {
            appliquer(&mut m.extension.uniformes, &surfaces, pluie, abris);
        }
    }
}
