//! Plugin Bevy : lit la carte au démarrage et pose chaque élément de chaque calque.
//!
//! Choix de performance :
//! - bibliothèque construite une fois : un `Handle<Mesh>` par modèle (272), une poignée de matériaux partagés
//!   (un mat pour tout ce qui est coloré par sommets, un par teinte de véhicule, un émissif pour les ampoules) ;
//! - éléments posés par `spawn_batch` (réservation en bloc des entités), avec seulement des composants `Copy` ou
//!   des `Handle` clonés : **aucune allocation par entité** (pas d'`uid` ni de nom de calque en `String` ; le
//!   numéro de calque [`MapLayer`] renvoie au manifeste) ;
//! - aucune hiérarchie : une entité par élément (plus une ampoule par lampe), pas de propagation de transformation ;
//! - disparition au-delà de la distance « fin » des budgets ([`fade_end`]) avec fondu, désactivable pour mesurer
//!   son effet.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use mcf_big_map::{BLOCK, Kind, STREET};

use crate::catalog::{MODEL_COUNT, ModelRef, fade_end};
use crate::load::{Manifest, load_map_dir};
use crate::models::{LAMP_HEAD, build_model, city_ground_mesh, ground_mesh, lamp_head_mesh, triangles};

/// Options d'affichage de la carte.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct MapViewOptions {
    /// Disparition au loin des petits éléments ([`fade_end`]).
    pub visibility_ranges: bool,
    /// Lampes allumées au démarrage (sinon : éteintes, à allumer avec [`set_lamps_lit`]).
    pub lamps_lit: bool,
    /// Ombres des 50 lampes « avec ombre » (réglage du banc).
    pub lamp_shadows: bool,
}

impl Default for MapViewOptions {
    fn default() -> Self {
        Self {
            visibility_ranges: true,
            lamps_lit: false,
            lamp_shadows: true,
        }
    }
}

/// Numéro du calque d'origine (index dans [`Manifest::files`]).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MapLayer(pub u16);

/// Ampoule d'un lampadaire (lumière + maillage émissif).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LampLight;

/// Étape de démarrage du chargement : les systèmes qui ont besoin de la carte se placent `.after(MapViewSet)`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MapViewSet;

/// Triangles d'une nature : par modèle (min / max) et au total sur la carte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindStats {
    /// Nature.
    pub kind: Kind,
    /// Éléments posés.
    pub placed: usize,
    /// Triangles du modèle le plus léger.
    pub tri_min: usize,
    /// Triangles du modèle le plus lourd.
    pub tri_max: usize,
    /// Triangles de tous les éléments posés (avant culling).
    pub tri_total: usize,
}

/// Bilan du chargement, inséré quand la carte est posée.
#[derive(Resource, Debug, Clone)]
pub struct MapReport {
    /// Manifeste de la carte.
    pub manifest: Manifest,
    /// Éléments posés.
    pub elements: usize,
    /// Entités créées (éléments + ampoules + sols).
    pub entities: usize,
    /// Maillages distincts.
    pub meshes: usize,
    /// Matériaux distincts.
    pub materials: usize,
    /// Avertissements de validation.
    pub warnings: usize,
    /// Lecture + validation + conversion.
    pub read_time: Duration,
    /// Construction des modèles + création des entités.
    pub spawn_time: Duration,
    /// Par nature, dans l'ordre de [`Kind::ALL`].
    pub kinds: Vec<KindStats>,
    /// Placement des bâtiments avec intérieur (pour le parcours caméra).
    pub interiors: Vec<Transform>,
}

/// Dossier de la carte à charger.
#[derive(Resource, Debug, Clone)]
struct MapDir(PathBuf);

/// Début de la création des entités (la fin est mesurée après application des commandes).
#[derive(Resource, Debug, Clone)]
struct Pending {
    start: Instant,
    report: MapReport,
}

/// Charge la carte du dossier `dir` au démarrage (`Startup`, ensemble [`MapViewSet`]). En cas de faute, l'erreur
/// est journalisée et l'application quitte en erreur : rien n'est affiché à moitié.
pub struct MapViewPlugin {
    /// Dossier produit par `cargo big-map`.
    pub dir: PathBuf,
    /// Options.
    pub options: MapViewOptions,
}

impl Plugin for MapViewPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(MapDir(self.dir.clone()))
            .insert_resource(self.options.clone())
            .add_systems(Startup, (load_and_spawn, finish_report).chain().in_set(MapViewSet));
    }
}

/// Allume ou éteint toutes les lampes (lumière et ampoule).
pub fn set_lamps_lit(lamps: &mut Query<&mut Visibility, With<LampLight>>, lit: bool) {
    let v = if lit { Visibility::Inherited } else { Visibility::Hidden };
    for mut vis in lamps.iter_mut() {
        vis.set_if_neq(v);
    }
}

fn load_and_spawn(
    mut commands: Commands,
    dir: Res<MapDir>,
    opts: Res<MapViewOptions>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut exit: MessageWriter<AppExit>,
) {
    let map = match load_map_dir(&dir.0) {
        Ok(m) => m,
        Err(e) => {
            error!("carte {} refusée : {e}", dir.0.display());
            exit.write(AppExit::error());
            return;
        }
    };
    let start = Instant::now();

    // Bibliothèque : un maillage par modèle, construit une fois.
    let mut tri = [0usize; MODEL_COUNT];
    let library: Vec<Handle<Mesh>> = ModelRef::all()
        .map(|m| {
            let mesh = build_model(m);
            tri[m.slot()] = triangles(&mesh);
            meshes.add(mesh)
        })
        .collect();
    let matte = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.85,
        reflectance: 0.3,
        ..default()
    });
    let head_mesh = meshes.add(lamp_head_mesh());
    let head_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::rgb(12.0, 9.0, 4.5),
        ..default()
    });
    let mut tints: Vec<([u8; 4], Handle<StandardMaterial>)> = Vec::new();
    let mut tint_material = |rgba: [u8; 4], materials: &mut Assets<StandardMaterial>| {
        if let Some((_, h)) = tints.iter().find(|(c, _)| *c == rgba) {
            return h.clone();
        }
        let h = materials.add(StandardMaterial {
            base_color: Color::srgba_u8(rgba[0], rgba[1], rgba[2], rgba[3]),
            perceptual_roughness: 0.35,
            metallic: 0.1,
            ..default()
        });
        tints.push((rgba, h.clone()));
        h
    };

    // Sols : île et ville, un maillage chacun.
    let m = &map.manifest;
    commands.spawn((Mesh3d(meshes.add(ground_mesh(m.size_m as f32))), MeshMaterial3d(matte.clone())));
    commands.spawn((
        Mesh3d(meshes.add(city_ground_mesh(m.city_m as f32, BLOCK as f32, STREET as f32))),
        MeshMaterial3d(matte.clone()),
    ));

    let ranged = |k: Kind| opts.visibility_ranges.then(|| fade_end(k)).flatten();
    let n_ranged = map.placements.iter().filter(|p| ranged(p.model.kind).is_some()).count();
    let mut plain = Vec::with_capacity(map.placements.len() - n_ranged);
    let mut faded = Vec::with_capacity(n_ranged);
    let mut heads = Vec::new();
    let mut interiors = Vec::new();
    let lamp_vis = if opts.lamps_lit { Visibility::Inherited } else { Visibility::Hidden };
    for p in &map.placements {
        let kind = p.model.kind;
        let material = match p.tint {
            Some(rgba) => tint_material(rgba, &mut materials),
            None => matte.clone(),
        };
        let base = (
            Mesh3d(library[p.model.slot()].clone()),
            MeshMaterial3d(material),
            p.transform,
            MapLayer(p.layer),
        );
        match ranged(kind) {
            // Fondu tramé sur les 10 derniers % : pas d'apparition brutale.
            Some(end) => faded.push((
                base,
                VisibilityRange {
                    start_margin: 0.0..0.0,
                    end_margin: end * 0.9..end,
                    use_aabb: false,
                },
            )),
            None => plain.push(base),
        }
        match kind {
            Kind::Lamp | Kind::ShadowLamp => heads.push((
                Mesh3d(head_mesh.clone()),
                MeshMaterial3d(head_mat.clone()),
                p.transform * Transform::from_translation(LAMP_HEAD),
                PointLight {
                    color: Color::srgb(1.0, 0.85, 0.65),
                    intensity: 60_000.0,
                    range: 18.0,
                    radius: 0.1,
                    shadow_maps_enabled: kind == Kind::ShadowLamp && opts.lamp_shadows,
                    ..default()
                },
                lamp_vis,
                NotShadowCaster,
                LampLight,
                MapLayer(p.layer),
            )),
            Kind::Interior => interiors.push(p.transform),
            _ => {}
        }
    }

    let kinds = Kind::ALL
        .iter()
        .map(|&kind| {
            let models: Vec<usize> = ModelRef::all()
                .filter(|m| m.kind == kind)
                .map(|m| tri[m.slot()])
                .collect();
            KindStats {
                kind,
                placed: map.count(kind),
                tri_min: models.iter().copied().min().unwrap_or(0),
                tri_max: models.iter().copied().max().unwrap_or(0),
                tri_total: map
                    .placements
                    .iter()
                    .filter(|p| p.model.kind == kind)
                    .map(|p| tri[p.model.slot()])
                    .sum(),
            }
        })
        .collect();
    let entities = plain.len() + faded.len() + heads.len() + 2;
    let report = MapReport {
        elements: map.placements.len(),
        entities,
        meshes: library.len() + 3,
        materials: 2 + tints.len(),
        warnings: map.warnings,
        read_time: map.read_time,
        spawn_time: Duration::ZERO,
        kinds,
        interiors,
        manifest: map.manifest,
    };
    commands.spawn_batch(plain);
    commands.spawn_batch(faded);
    commands.spawn_batch(heads);
    commands.insert_resource(Pending { start, report });
}

/// Après application des commandes : la carte est réellement posée, on fige le bilan.
fn finish_report(mut commands: Commands, pending: Option<Res<Pending>>) {
    let Some(p) = pending else { return };
    let mut report = p.report.clone();
    report.spawn_time = p.start.elapsed();
    info!(
        "carte « {} » : {} éléments, {} entités, {} maillages, {} matériaux ; lecture {:.0} ms, pose {:.0} ms",
        report.manifest.tier,
        report.elements,
        report.entities,
        report.meshes,
        report.materials,
        report.read_time.as_secs_f64() * 1000.0,
        report.spawn_time.as_secs_f64() * 1000.0
    );
    commands.remove_resource::<Pending>();
    commands.insert_resource(report);
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin;

    use super::*;
    use crate::load::tests::{TempMap, small_params};

    fn app(dir: PathBuf, options: MapViewOptions) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .add_plugins(MapViewPlugin { dir, options });
        app.update();
        app
    }

    #[test]
    fn pose_chaque_element_une_fois() {
        let tmp = TempMap::new("plugin");
        let q = small_params().quantities;
        let mut app = app(tmp.0.clone(), MapViewOptions::default());
        let world = app.world_mut();
        let report = world.resource::<MapReport>().clone();
        let lamps = q.of(Kind::Lamp) + q.of(Kind::ShadowLamp);
        assert_eq!(report.elements, q.total());
        assert_eq!(report.entities, q.total() + lamps + 2);
        let layered = world.query_filtered::<(), With<MapLayer>>().iter(world).count();
        assert_eq!(layered, q.total() + lamps);
        let heads = world.query_filtered::<&Visibility, With<LampLight>>().iter(world).collect::<Vec<_>>();
        assert_eq!(heads.len(), lamps);
        assert!(heads.iter().all(|v| **v == Visibility::Hidden), "lampes éteintes au départ");
        let shadow = world
            .query::<&PointLight>()
            .iter(world)
            .filter(|l| l.shadow_maps_enabled)
            .count();
        assert_eq!(shadow, q.of(Kind::ShadowLamp));
        let faded = world.query::<&VisibilityRange>().iter(world).count();
        let expected: usize = Kind::ALL
            .iter()
            .filter(|k| fade_end(**k).is_some())
            .map(|k| q.of(*k))
            .sum();
        assert_eq!(faded, expected);
        assert_eq!(report.interiors.len(), q.of(Kind::Interior));
        // Maillages partagés : le nombre d'assets ne dépend pas du nombre d'éléments.
        // Les modèles non posés sont libérés (leurs handles sont lâchés en fin de système) :
        // on borne le nombre de maillages vivants au lieu de l'exiger exact.
        let live = world.resource::<Assets<Mesh>>().len();
        assert!(live > 3 && live <= MODEL_COUNT + 3, "maillages vivants : {live}");
        assert_eq!(report.meshes, MODEL_COUNT + 3);
        assert!(report.materials <= 2 + 6, "une teinte de véhicule = un matériau");
        assert!(report.kinds.iter().all(|k| k.tri_min > 0 && k.tri_min <= k.tri_max));
    }

    #[test]
    fn options_sans_disparition() {
        let tmp = TempMap::new("plugin_options");
        let opts = MapViewOptions {
            visibility_ranges: false,
            lamps_lit: true,
            lamp_shadows: false,
        };
        let mut app = app(tmp.0.clone(), opts);
        let world = app.world_mut();
        assert_eq!(world.query::<&VisibilityRange>().iter(world).count(), 0);
        assert!(world.query::<&PointLight>().iter(world).all(|l| !l.shadow_maps_enabled));
        assert!(
            world
                .query_filtered::<&Visibility, With<LampLight>>()
                .iter(world)
                .all(|v| *v == Visibility::Inherited)
        );
    }

    #[test]
    fn carte_absente_quitte_en_erreur() {
        let app = app(PathBuf::from("dossier/inexistant"), MapViewOptions::default());
        assert!(app.world().get_resource::<MapReport>().is_none());
        assert!(app.should_exit().is_some_and(|e| e.is_error()));
    }
}
