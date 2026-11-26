//! Bâtiments modélisés dans l'éditeur (spec §6.5) : catalogue `assets/batiments/catalogue.json`, modèles `.glb`
//! affichés dans la 3D à la place des blocs, pose depuis la liste.
//!
//! Le catalogue est versionné ; les `.glb` et leurs textures sont des assets du jeu, copiés à côté et non versionnés.
//! Une fiche dont le `.glb` manque (ou sort du dossier) n'est pas « disponible » : ses bâtiments restent des blocs,
//! la carte s'ouvre quand même et la liste le signale.
//!
//! Les modèles ne font que **montrer** l'empreinte : elle reste la vérité (sélection, conflits, terrain).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use bevy::{
    camera::visibility::{RenderLayers, VisibilityRange},
    prelude::*,
    world_serialization::WorldInstanceReady,
};
use mcf_carte::{Batiment, CatalogueBatiments, Fiche, Id, ModelePose, Revisions, catalogue::MAX_OCTETS};

use super::{COUCHE_3D, Edition, Projet};

/// Dossier du catalogue, relatif au dossier `assets` de l'éditeur.
pub const DOSSIER: &str = "batiments";
/// Distance au-delà de laquelle un bâtiment modélisé laisse place à son bloc (m).
pub const PORTEE_MODELE_M: f32 = 1400.0;
/// Case du chargement par zone (m) : les modèles sont recalculés quand l'œil en change.
const PAS_ZONE_M: f32 = 200.0;
/// Fichier du catalogue dans ce dossier.
const FICHIER: &str = "catalogue.json";

/// Catalogue chargé au démarrage.
#[derive(Resource, Default)]
pub struct Catalogue {
    /// Fiches valides (vide si le catalogue est refusé).
    pub fiches: Vec<Fiche>,
    /// Fiches dont le modèle est présent.
    pub disponibles: HashSet<String>,
    /// Erreur de lecture, pour la liste.
    pub erreur: Option<String>,
}

impl Catalogue {
    /// Fiche d'identifiant `id`.
    pub fn fiche(&self, id: &str) -> Option<&Fiche> {
        self.fiches.iter().find(|f| f.id == id)
    }

    /// Le bâtiment s'affiche avec son modèle (sinon en bloc).
    pub fn modele_affiche(&self, b: &Batiment) -> bool {
        b.modele
            .as_ref()
            .is_some_and(|m| self.disponibles.contains(&m.fiche))
    }
}

pub struct ModelesPlugin;

impl Plugin for ModelesPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(charger(&dossier()))
            .init_resource::<Instances>()
            .add_systems(Update, synchroniser.after(super::outils::SystemesOutils));
    }
}

fn dossier() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(DOSSIER)
}

/// Lit le catalogue (taille plafonnée avant lecture) et repère les modèles présents dans le dossier.
fn charger(dossier: &Path) -> Catalogue {
    let chemin = dossier.join(FICHIER);
    let lire = || -> Result<CatalogueBatiments, String> {
        let meta = std::fs::symlink_metadata(&chemin).map_err(|e| format!("{} : {e}", chemin.display()))?;
        if !meta.is_file() || meta.len() > MAX_OCTETS {
            return Err(format!(
                "{} : pas un fichier ordinaire ou trop gros",
                chemin.display()
            ));
        }
        let texte = std::fs::read_to_string(&chemin).map_err(|e| format!("{} : {e}", chemin.display()))?;
        CatalogueBatiments::lire(&texte)
    };
    match lire() {
        Ok(c) => {
            let racine = dossier.canonicalize().ok();
            let disponibles = c
                .fiches
                .iter()
                .filter(|f| modele_present(dossier, racine.as_deref(), &f.fichier))
                .map(|f| f.id.clone())
                .collect();
            Catalogue {
                fiches: c.fiches,
                disponibles,
                erreur: None,
            }
        }
        Err(e) => {
            warn!("Catalogue des bâtiments refusé : {e}");
            Catalogue {
                erreur: Some(e),
                ..default()
            }
        }
    }
}

/// Le modèle existe, est un fichier ordinaire (pas un lien) et reste dans le dossier du catalogue une fois résolu.
fn modele_present(dossier: &Path, racine: Option<&Path>, fichier: &str) -> bool {
    let chemin = dossier.join(fichier);
    let ordinaire = std::fs::symlink_metadata(&chemin).is_ok_and(|m| m.is_file());
    let confine = match (racine, chemin.canonicalize()) {
        (Some(r), Ok(c)) => c.starts_with(r),
        _ => false,
    };
    ordinaire && confine
}

/// Modèles instanciés : bâtiment → (entité, fiche).
#[derive(Resource, Default)]
struct Instances(HashMap<Id, (Entity, String)>);

/// Racine d'un modèle de bâtiment.
#[derive(Component)]
struct ModeleBatiment;

/// Orientation d'un modèle posé : cap autour de +Y, puis tangage autour de son axe X, puis roulis autour de son axe
/// Z (`Ry · Rx · Rz`) ; tourner autour de l'axe X du modèle ajoute donc exactement au tangage, et de même pour le
/// roulis (anneaux du gizmo).
pub fn rotation(m: &ModelePose) -> Quat {
    let r = |deg: f64| (deg as f32).to_radians();
    Quat::from_rotation_y(r(m.cap_deg))
        * Quat::from_rotation_x(r(m.tangage_deg))
        * Quat::from_rotation_z(r(m.roulis_deg))
}

/// Place des modèles : `Transform` du bâtiment modélisé `b` (fiche `fiche`), le sol sous son centre étant à `sol`.
/// Le centre de l'encombrement du modèle tombe sur le centre de l'empreinte, au sol plus le décalage vertical.
pub fn placement(fiche: &Fiche, b: &Batiment, sol: f64) -> Option<Transform> {
    let rot = rotation(b.modele.as_ref()?);
    let c = b.centre();
    let decal = rot * Vec3::new(fiche.centre_m[0] as f32, 0.0, fiche.centre_m[1] as f32);
    let ancre = Vec3::new(c[0] as f32, (sol + b.decalage_y_m) as f32, c[1] as f32);
    Some(Transform::from_translation(ancre - decal).with_rotation(rot))
}

/// Garde les modèles en accord avec la carte (pose, retrait, déplacement) et montre l'aperçu d'un geste en cours.
/// Ne travaille que si la carte ou le geste ont changé.
#[allow(clippy::too_many_arguments)]
fn synchroniser(
    mut commands: Commands,
    assets: Res<AssetServer>,
    touches: Res<ButtonInput<KeyCode>>,
    catalogue: Res<Catalogue>,
    edition: Res<Edition>,
    mut projet: ResMut<Projet>,
    mut instances: ResMut<Instances>,
    mut transforms: Query<&mut Transform, With<ModeleBatiment>>,
    orbite: Res<super::camera::Orbite>,
    mut vu: Local<Option<(Revisions, Option<super::Glisse>, Option<(i64, i64)>)>>,
) {
    // Chargement par zone : seuls les modèles proches de l'œil de la caméra 3D existent (au-delà, le bloc du bâtiment
    // les remplace, `scene`). Recalculé quand l'œil change de case de `PAS_ZONE_M`.
    let (s, c) = bevy::math::ops::sin_cos(orbite.lacet);
    let avant = Vec3::new(-s, 0.0, -c);
    let oeil = orbite.cible - avant * orbite.distance * bevy::math::ops::cos(orbite.tangage);
    let hauteur = orbite.distance * bevy::math::ops::sin(orbite.tangage);
    let zone = (hauteur < PORTEE_MODELE_M).then(|| {
        (
            (oeil.x / PAS_ZONE_M).floor() as i64,
            (oeil.z / PAS_ZONE_M).floor() as i64,
        )
    });
    let etat = (projet.carte.revisions, edition.glisse, zone);
    if *vu == Some(etat) {
        return;
    }
    *vu = Some(etat);
    let alt = touches.pressed(KeyCode::AltLeft) || touches.pressed(KeyCode::AltRight);
    let apercu = super::gizmo::apercu(&edition, alt);
    let selection: HashSet<Id> = match &edition.selection {
        super::Selection::Batiments(ids) => ids.iter().copied().collect(),
        _ => HashSet::new(),
    };
    let poses: Vec<(Id, Batiment)> = projet
        .carte
        .batiments
        .iter()
        .filter(|(_, b)| catalogue.modele_affiche(b))
        .filter(|(_, b)| {
            let c = b.centre();
            zone.is_some()
                && Vec2::new(c[0] as f32 - oeil.x, c[1] as f32 - oeil.z).length()
                    < PORTEE_MODELE_M + PAS_ZONE_M * 1.5
        })
        .map(|(i, b)| (*i, b.clone()))
        .collect();
    let terrain = &projet.carte.derive().terrain;
    let mut gardes = HashSet::new();
    for (id, b) in poses {
        let Some(m) = &b.modele else { continue };
        let Some(fiche) = catalogue.fiche(&m.fiche) else {
            continue;
        };
        let mut vu_ici = b.clone();
        if let Some(t) = &apercu
            && selection.contains(&id)
            && !b.verrou
        {
            vu_ici.transformer(t);
        }
        let Some(t) = placement(fiche, &vu_ici, terrain.hauteur(vu_ici.centre())) else {
            continue;
        };
        gardes.insert(id);
        match instances.0.get(&id) {
            Some((e, f)) if *f == m.fiche => {
                if let Ok(mut tr) = transforms.get_mut(*e) {
                    *tr = t;
                }
            }
            autre => {
                if let Some((e, _)) = autre {
                    commands.entity(*e).despawn();
                }
                let scene =
                    assets.load(GltfAssetLabel::Scene(0).from_asset(format!("{DOSSIER}/{}", fiche.fichier)));
                let e = commands
                    .spawn((
                        ModeleBatiment,
                        WorldAssetRoot(scene),
                        t,
                        RenderLayers::layer(COUCHE_3D),
                    ))
                    .observe(preparer)
                    .id();
                instances.0.insert(id, (e, m.fiche.clone()));
            }
        }
    }
    instances.0.retain(|id, (e, _)| {
        let garder = gardes.contains(id);
        if !garder {
            commands.entity(*e).despawn();
        }
        garder
    });
}

/// Modèle instancié : couche 3D sur toute la hiérarchie (la caméra Plan ne le voit pas, le Plan garde l'aplat de
/// l'empreinte), maillages de collision `-col` (import Godot) cachés.
fn preparer(
    pret: On<WorldInstanceReady>,
    mut commands: Commands,
    enfants: Query<&Children>,
    noms: Query<&Name>,
) {
    let racine = pret.event_target();
    let portee = VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: (PORTEE_MODELE_M - 200.0)..PORTEE_MODELE_M,
        use_aabb: false,
    };
    for e in enfants.iter_descendants(racine) {
        let mut ec = commands.entity(e);
        ec.insert((RenderLayers::layer(COUCHE_3D), portee.clone()));
        if noms.get(e).is_ok_and(|n| n.as_str().contains("-col")) {
            ec.insert(Visibility::Hidden);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hauteur_et_inclinaison_du_modele() {
        let c = charger(&dossier());
        let f = c.fiche("superette").expect("fiche").clone();
        let mut b = f.batiment([0.0, 0.0], 90.0);
        b.transformer(&mcf_carte::Transformation {
            dy_m: -0.8,
            tangage_deg: 10.0,
            ..Default::default()
        });
        let t = placement(&f, &b, 5.0).expect("modélisé");
        // Le centre de l'encombrement reste ancré au sol + décalage, même incliné.
        let w = t.transform_point(Vec3::new(f.centre_m[0] as f32, 0.0, f.centre_m[1] as f32));
        assert!((w - Vec3::new(0.0, 4.2, 0.0)).length() < 1e-4, "{w}");
        // Tangage : l'axe X du modèle (après le cap) ne bouge pas, le côté −Z (l'arrière du modèle) monte.
        let x = t.rotation * Vec3::X;
        assert!((x - Quat::from_rotation_y(90f32.to_radians()) * Vec3::X).length() < 1e-5);
        assert!((t.rotation * Vec3::NEG_Z).y > 0.1);
    }

    #[test]
    fn catalogue_du_depot_valide() {
        let c = charger(&dossier());
        assert!(c.erreur.is_none(), "{:?}", c.erreur);
        assert!(c.fiche("maison_a").is_some() && c.fiche("superette").is_some());
    }

    #[test]
    fn modele_hors_du_dossier_non_disponible() {
        let d = dossier();
        let r = d.canonicalize().ok();
        assert!(!modele_present(
            &d,
            r.as_deref(),
            "../personnages/perso_homme.glb"
        ));
        assert!(!modele_present(&d, r.as_deref(), "absent/rien.glb"));
    }

    #[test]
    fn placement_pose_le_centre_de_la_fiche_sur_le_centre_de_l_empreinte() {
        let c = charger(&dossier());
        let f = c.fiche("maison_a").expect("fiche").clone();
        for cap in [0.0, 37.0, 90.0, 215.0] {
            let b = f.batiment([100.0, -50.0], cap);
            let t = placement(&f, &b, 12.0).expect("modélisé");
            // Le centre de l'encombrement du modèle (repère du modèle) tombe sur le centre de l'empreinte.
            let w = t.transform_point(Vec3::new(f.centre_m[0] as f32, 0.0, f.centre_m[1] as f32));
            assert!(
                (w - Vec3::new(100.0, 12.0, -50.0)).length() < 1e-3,
                "cap {cap} : {w}"
            );
            // Un coin du modèle tombe sur un coin de l'empreinte.
            let coin = t.transform_point(Vec3::new(
                (f.centre_m[0] + f.largeur_m / 2.0) as f32,
                0.0,
                (f.centre_m[1] + f.profondeur_m / 2.0) as f32,
            ));
            assert!(
                b.empreinte
                    .iter()
                    .any(|p| (Vec2::new(p[0] as f32, p[1] as f32) - coin.xz()).length() < 1e-3),
                "cap {cap} : coin {coin} absent de {:?}",
                b.empreinte
            );
        }
    }
}
