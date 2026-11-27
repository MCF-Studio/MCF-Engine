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
use crate::houle::Ocean;
use crate::ocean::TextureRivage;
use crate::rivage::{CarteRivage, ParametresRivage, hauteur_mouillee, params_bord};
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
    /// Niveau de la mer (m).
    pub niveau_mer: f32,
    /// Hauteur au-dessus de la mer jusqu'où les vagues mouillent le sable (m), voir
    /// [`crate::rivage::hauteur_mouillee`].
    pub hauteur_mouillee: f32,
    /// Carte des abris : origine x, origine z, côté d'une case, cases par côté (0 = pas de carte).
    pub abris: Vec4,
    /// Grille du rivage : origine x, origine z, côté (m), 1 si active (0 = pas de sable mouillé par les vagues).
    pub rivage: Vec4,
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
            niveau_mer: 0.0,
            hauteur_mouillee: 0.0,
            abris: Vec4::new(0.0, 0.0, 1.0, 0.0),
            rivage: Vec4::ZERO,
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
    /// Grille du rivage (fond, distance signée, normale), recopiée de l'océan s'il y en a un.
    #[texture(102, filterable = false)]
    pub carte_rivage: Handle<Image>,
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
            carte_rivage: Handle::default(),
        },
    }
}

/// Écart en dessous duquel on ne renvoie pas le matériau au GPU (1/256 : invisible à l'œil).
const SEUIL: f32 = 1.0 / 256.0;

/// Sable mouillé par les vagues de bord : grille du rivage, niveau de la mer et hauteur atteinte par l'eau.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RiveMouillee {
    /// Grille du rivage : origine x, origine z, côté (m), 1 si active.
    pub grille: Vec4,
    /// Niveau de la mer (m).
    pub niveau: f32,
    /// Hauteur au-dessus de la mer jusqu'où le sable est mouillé (m).
    pub hauteur: f32,
}

/// Recopie les valeurs globales dans les uniformes d'un matériau. Renvoie vrai si quelque chose a changé de façon
/// visible.
pub fn appliquer(
    u: &mut UniformesSurface,
    surfaces: &SurfacesMeteo,
    pluie: f32,
    abris: Vec4,
    rive: RiveMouillee,
) -> bool {
    let proche = |a: f32, b: f32| (a - b).abs() < SEUIL;
    // Hauteurs en mètres : 1 cm près.
    let proche_m = |a: f32, b: f32| (a - b).abs() < 0.01;
    if proche(u.mouillage, surfaces.mouillage)
        && proche(u.flaques, surfaces.flaques)
        && proche(u.neige, surfaces.neige)
        && proche(u.pluie, pluie)
        && u.abris == abris
        && u.rivage == rive.grille
        && proche_m(u.niveau_mer, rive.niveau)
        && proche_m(u.hauteur_mouillee, rive.hauteur)
    {
        return false;
    }
    u.mouillage = surfaces.mouillage;
    u.flaques = surfaces.flaques;
    u.neige = surfaces.neige;
    u.pluie = pluie;
    u.abris = abris;
    u.rivage = rive.grille;
    u.niveau_mer = rive.niveau;
    u.hauteur_mouillee = rive.hauteur;
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

#[allow(clippy::too_many_arguments)]
fn mettre_a_jour_materiaux(
    surfaces: Res<SurfacesMeteo>,
    etat: Res<EtatMeteo>,
    carte: Option<Res<CarteAbris>>,
    ocean: Option<Res<Ocean>>,
    rivage: Option<Res<CarteRivage>>,
    parametres_rivage: Option<Res<ParametresRivage>>,
    texture_rivage: Option<Res<TextureRivage>>,
    mut materiaux: ResMut<Assets<MateriauMeteo>>,
) {
    let rive = match (&ocean, &rivage, &parametres_rivage) {
        (Some(ocean), Some(rivage), Some(parametres)) => RiveMouillee {
            grille: rivage
                .grille()
                .map(|g| Vec4::new(g.origine().x, g.origine().y, g.taille_m(), 1.0))
                .unwrap_or(Vec4::ZERO),
            niveau: ocean.niveau,
            hauteur: hauteur_mouillee(&params_bord(&etat, parametres)),
        },
        _ => RiveMouillee::default(),
    };
    let image_rivage = texture_rivage.map(|t| t.0.clone()).unwrap_or_default();
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
            appliquer(&mut u, &surfaces, pluie, abris, rive) || m.extension.carte_rivage != image_rivage
        })
        .map(|(id, _)| id)
        .collect();
    for id in a_changer {
        if let Some(mut m) = materiaux.get_mut(id) {
            appliquer(&mut m.extension.uniformes, &surfaces, pluie, abris, rive);
            m.extension.carte_rivage = image_rivage.clone();
        }
    }
}
