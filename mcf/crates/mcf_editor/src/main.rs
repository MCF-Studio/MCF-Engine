//! Première base fonctionnelle de l'éditeur MCF.
//!
//! La carte et la scène affichent le même monde : la gauche est une caméra
//! orthographique de dessus, la droite une caméra de travail 3D. Un calque
//! `.mcfmap` validé peut être chargé au lancement avec `cargo editor -- chemin`.

use std::path::{Path, PathBuf};

use bevy::{
    asset::RenderAssetUsages,
    camera::Viewport,
    input::mouse::{MouseMotion, MouseWheel},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    window::PrimaryWindow,
};
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use mcf_core::{CatalogRef, LayerName, McfCorePlugin, McfUid, mcf_map};
use mcf_geo::{ClasseRoute, NatureBande, TableProfils};

#[derive(Component, Debug, Clone)]
struct EditorObject {
    name: String,
    catalog: String,
}

/// Un objet verrouillé reste sélectionnable mais ne peut pas être déplacé.
#[derive(Component)]
struct EditorLocked;

/// Un objet masqué n'est ni rendu ni sélectionnable dans les viewports.
#[derive(Component)]
struct EditorHidden;

/// Tracé de route conservé comme donnée source. Toute sa géométrie visible (ruban 3D, plan 2D, sélection) vient de
/// `mcf_geo` : une seule source, donc aucun écart entre les vues.
#[derive(Component, Debug, Clone)]
struct RoadPreview {
    points: Vec<Vec3>,
    classe: ClasseRoute,
}

impl RoadPreview {
    fn profil(&self) -> &'static mcf_geo::ProfilRoute {
        TableProfils::DEFAUT.get(self.classe)
    }

    /// Largeur bord à bord, en mètres.
    fn emprise_m(&self) -> f32 {
        self.profil().emprise_m() as f32
    }

    /// Axe discrétisé dans le plan `(x, z)`.
    fn axe(&self) -> Vec<mcf_geo::P2> {
        let points: Vec<mcf_geo::P2> = self.points.iter().map(|p| [p.x as f64, p.z as f64]).collect();
        mcf_geo::discretiser(&points, &[], mcf_geo::ECART_CORDE_M)
    }

    /// Bandes du profil en travers (chaussée, côtés).
    fn bandes(&self) -> Vec<mcf_geo::Bande> {
        mcf_geo::bandes(&self.axe(), self.profil())
    }
}

/// Maillage de chaussée créé à partir d'un tracé de route.
#[derive(Component)]
struct RoadSurface;

#[derive(Component)]
struct RoadSurfaceOwner(Entity);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum EditorViewport {
    Map2d,
    Scene3d,
}

#[derive(Resource, Debug, Default)]
struct EditorState {
    selected: Option<Entity>,
    selected_entities: Vec<Entity>,
    source: String,
    message: String,
    grid_visible: bool,
    snap_enabled: bool,
    catalog_query: String,
    selected_catalog: Option<String>,
    save_path_input: String,
    open_path_input: String,
    selection_box_origin: Option<egui::Pos2>,
}

/// Met à jour la sélection principale et, avec Ctrl, son ensemble associé.
fn select_entities(state: &mut EditorState, entity: Option<Entity>, additive: bool) {
    if !additive {
        state.selected_entities.clear();
        if let Some(entity) = entity {
            state.selected_entities.push(entity);
        }
        state.selected = entity;
        return;
    }
    let Some(entity) = entity else {
        return;
    };
    if let Some(index) = state.selected_entities.iter().position(|item| *item == entity) {
        state.selected_entities.remove(index);
        if state.selected == Some(entity) {
            state.selected = state.selected_entities.last().copied();
        }
    } else {
        state.selected_entities.push(entity);
        state.selected = Some(entity);
    }
}

/// Le calque ouvert est conservé afin de réécrire uniquement les transformations modifiées.
#[derive(Resource, Debug, Default)]
struct MapDocument {
    path: Option<PathBuf>,
    map: Option<mcf_map::McfMap>,
    dirty: bool,
}

#[derive(Resource, Debug, Default)]
struct SaveRequest(bool);

#[derive(Debug, Clone, Copy)]
struct TransformChange {
    entity: Entity,
    before: Transform,
    after: Transform,
}

/// Historique des transformations, conservé tant que l'éditeur reste ouvert.
#[derive(Resource, Debug, Default)]
struct TransformHistory {
    undo: Vec<TransformChange>,
    redo: Vec<TransformChange>,
    undo_requested: bool,
    redo_requested: bool,
}

/// Une transformation en cours : l'historique ne reçoit qu'une entrée à la fin du geste.
#[derive(Resource, Debug, Default)]
struct DragState {
    entity: Option<Entity>,
    before: Option<Transform>,
    after: Option<Transform>,
    viewport: Option<EditorViewport>,
}

/// Termine le geste courant et ajoute une unique opération annulable si nécessaire.
fn commit_drag(drag: &mut DragState, history: &mut TransformHistory, document: &mut MapDocument) -> bool {
    let transaction = (drag.entity, drag.before, drag.after);
    *drag = DragState::default();
    let (Some(entity), Some(before), Some(after)) = transaction else {
        return false;
    };
    if before == after {
        return false;
    }
    history.undo.push(TransformChange {
        entity,
        before,
        after,
    });
    history.redo.clear();
    document.dirty = true;
    true
}

fn main() -> AppExit {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "MCF Editor — Carte 2D + scène 3D".into(),
                resolution: (1440, 840).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((McfCorePlugin, EguiPlugin::default()))
        .insert_resource(EditorState {
            grid_visible: true,
            snap_enabled: true,
            ..default()
        })
        .init_resource::<MapDocument>()
        .init_resource::<SaveRequest>()
        .init_resource::<TransformHistory>()
        .init_resource::<DragState>()
        .add_systems(Startup, setup_editor)
        .add_systems(
            Update,
            (
                update_viewports,
                move_scene_camera,
                look_scene_camera,
                zoom_map_camera,
                zoom_map_with_wheel,
                select_object_in_viewport,
                drag_selected_in_scene.after(select_object_in_viewport),
                clear_selection,
                select_all,
                draw_grid,
                draw_selection_outline,
                save_document,
                apply_transform_history,
            ),
        )
        .add_systems(EguiPrimaryContextPass, editor_ui)
        .run()
}

fn setup_editor(
    mut commands: Commands,
    mut state: ResMut<EditorState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(GlobalAmbientLight {
        brightness: 500.0,
        ..default()
    });
    spawn_ground(&mut commands, &mut meshes, &mut materials);

    let document = match std::env::args().nth(1) {
        Some(path) => match load_map(Path::new(&path), &mut commands, &mut meshes, &mut materials) {
            Ok((map, count)) => {
                let name = map.layer.clone();
                state.source = path.clone();
                state.message = format!("{count} objets chargés dans le calque « {name} ».");
                MapDocument {
                    path: Some(PathBuf::from(path)),
                    map: Some(map),
                    dirty: false,
                }
            }
            Err(error) => {
                state.source = "Scène de démonstration".into();
                state.message = format!("Carte refusée : {error}. La scène de démonstration est affichée.");
                spawn_demo_roads(&mut commands, &mut meshes, &mut materials);
                spawn_demo_city(&mut commands, &mut meshes, &mut materials);
                MapDocument::default()
            }
        },
        None => {
            state.source = "Scène de démonstration".into();
            state.message =
                "Passez un fichier .mcfmap après `cargo editor --` pour charger un calque validé.".into();
            spawn_demo_roads(&mut commands, &mut meshes, &mut materials);
            spawn_demo_city(&mut commands, &mut meshes, &mut materials);
            MapDocument::default()
        }
    };
    commands.insert_resource(document);

    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(12.0, 24.0, 16.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Camera3d::default(),
        Projection::Orthographic(OrthographicProjection {
            // Le plan de démonstration s'étend sur environ 40 m. Une échelle
            // cohérente rend la carte lisible dès l'ouverture de l'éditeur.
            scale: 42.0,
            ..OrthographicProjection::default_3d()
        }),
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.06, 0.11, 0.14)),
            ..default()
        },
        EditorViewport::Map2d,
        Transform::from_xyz(0.0, 60.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z),
    ));
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 1,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.035, 0.045, 0.065)),
            ..default()
        },
        EditorViewport::Scene3d,
        Transform::from_xyz(22.0, 16.0, 22.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
    ));
}

fn spawn_ground(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(44.0, 44.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.12, 0.22, 0.16))),
    ));
}

fn spawn_demo_roads(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    // Chaque voie possède une largeur en mètres unique, réutilisée dans les
    // deux vues : plus de trait 2D arbitraire face à un ruban 3D différent.
    spawn_road_preview(
        commands,
        meshes,
        materials,
        vec![Vec3::new(0.0, 0.05, -21.0), Vec3::new(0.0, 0.05, 21.0)],
        ClasseRoute::Rue,
    );
    spawn_road_preview(
        commands,
        meshes,
        materials,
        vec![Vec3::new(-21.0, 0.05, 0.0), Vec3::new(21.0, 0.05, 0.0)],
        ClasseRoute::Rue,
    );
}

/// Classe d'une route du calque. Le calque a été validé (`ROAD_TEMPLATES` = classes de `mcf_geo`) : le repli
/// n'est atteint que pour une donnée construite en mémoire sans validation.
fn road_class(template: &str) -> ClasseRoute {
    ClasseRoute::depuis_id(template).unwrap_or(ClasseRoute::Rue)
}

/// Crée le tracé et ses rubans 3D.
fn spawn_road_preview(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    points: Vec<Vec3>,
    classe: ClasseRoute,
) -> Entity {
    let road = RoadPreview { points, classe };
    let owner = commands.spawn(road.clone()).id();
    spawn_road_surfaces(commands, meshes, materials, owner, &road);
    owner
}

/// Couleur provisoire d'une bande, commune au plan et à la 3D (les vrais matériaux viendront avec les styles
/// « plan » et « satellite »).
fn band_color(road: &RoadPreview, nature: NatureBande) -> Color {
    let profil = road.profil();
    match (nature, profil.revetement, profil.cote) {
        (NatureBande::Chaussee, mcf_geo::Revetement::Revetu, _) => Color::srgb(0.075, 0.08, 0.095),
        (NatureBande::Chaussee, mcf_geo::Revetement::Terre, _) => Color::srgb(0.42, 0.33, 0.22),
        (_, _, mcf_geo::Cote::Trottoir) => Color::srgb(0.55, 0.55, 0.53),
        _ => Color::srgb(0.36, 0.34, 0.28),
    }
}

/// Un maillage continu par bande, construit par `mcf_geo` (plus de pavés par segment : ni trou ni chevauchement
/// dans les virages).
fn spawn_road_surfaces(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    owner: Entity,
    road: &RoadPreview,
) {
    // Le tracé v1 ne porte pas de rayons : l'axe discrétisé garde les points de `road.points`, dans le même ordre.
    // Les points sont stockés 0,12 m au-dessus du sol (voir `sync_road_points`) ; la chaussée revient à +3 cm et
    // les côtés à +5 cm pour ne pas scintiller avec le sol.
    let altitudes: Vec<f64> = road.points.iter().map(|p| p.y as f64).collect();
    for bande in road.bandes() {
        let surelevation = if bande.nature == NatureBande::Chaussee {
            -0.09
        } else {
            -0.07
        };
        let maillage = mcf_geo::maillage_bande(&bande, &altitudes, surelevation);
        if maillage.indices.is_empty() {
            continue;
        }
        let to_f32 = |v: &[[f64; 3]]| v.iter().map(|p| p.map(|c| c as f32)).collect::<Vec<[f32; 3]>>();
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, to_f32(&maillage.positions))
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, to_f32(&maillage.normales))
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_UV_0,
                maillage
                    .uvs
                    .iter()
                    .map(|uv| uv.map(|c| c as f32))
                    .collect::<Vec<[f32; 2]>>(),
            )
            .with_inserted_indices(Indices::U32(maillage.indices));
        commands.spawn((
            RoadSurface,
            RoadSurfaceOwner(owner),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(band_color(road, bande.nature))),
            Transform::IDENTITY,
        ));
    }
}

fn spawn_demo_city(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let samples = [
        (
            "Hôtel de ville",
            "batiments/hotel",
            Vec3::new(-8.0, 3.0, -8.0),
            Vec3::new(5.0, 6.0, 5.0),
        ),
        (
            "Port",
            "batiments/entrepot",
            Vec3::new(8.0, 2.0, -7.0),
            Vec3::new(6.0, 4.0, 4.0),
        ),
        (
            "Place centrale",
            "props/fontaine",
            Vec3::new(-7.0, 0.8, 7.0),
            Vec3::new(2.5, 1.6, 2.5),
        ),
        (
            "Station-service",
            "batiments/station",
            Vec3::new(8.0, 1.5, 7.0),
            Vec3::new(5.0, 3.0, 4.0),
        ),
    ];
    for (index, (name, catalog, position, scale)) in samples.into_iter().enumerate() {
        spawn_editor_object(
            commands,
            meshes,
            materials,
            name.into(),
            catalog.into(),
            format!("ed{:06x}", index + 1),
            "Démonstration".into(),
            Transform::from_translation(position).with_scale(scale),
        );
    }
}

fn load_map(
    path: &Path,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<(mcf_map::McfMap, usize), String> {
    if !is_mcfmap_path(path) {
        return Err("le fichier doit avoir l'extension .mcfmap".into());
    }
    let loaded = mcf_map::load_file(path, None).map_err(|error| error.to_string())?;
    let map = loaded.map;
    let count = spawn_loaded_map(&map, commands, meshes, materials);
    Ok((map, count))
}

/// Instancie un calque déjà validé. La validation précède toujours toute mutation de la scène.
fn spawn_loaded_map(
    map: &mcf_map::McfMap,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> usize {
    let count = map.items.len();
    let road_count = map.roads.len();
    for item in &map.items {
        spawn_editor_object(
            commands,
            meshes,
            materials,
            item.catalog.clone(),
            item.catalog.clone(),
            item.uid.clone(),
            map.layer.clone(),
            transform_from_map(&item.transform),
        );
    }
    for road in &map.roads {
        let points: Vec<Vec3> = road
            .points
            .iter()
            .map(|point| Vec3::new(point[0] as f32, point[1] as f32 + 0.12, point[2] as f32))
            .collect();
        let translation = points.first().copied().unwrap_or(Vec3::ZERO);
        let entity = spawn_road_preview(commands, meshes, materials, points, road_class(&road.template));
        commands.entity(entity).insert((
            EditorObject {
                name: format!("Route {}", road.uid),
                catalog: road.template.clone(),
            },
            McfUid(road.uid.clone()),
            CatalogRef(road.template.clone()),
            LayerName(map.layer.clone()),
            Transform::from_translation(translation),
        ));
    }
    count + road_count
}

fn is_mcfmap_path(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some("mcfmap")
}

/// Reconstitue la pose Godot/MCF (colonnes de la base) sans changer de repère.
fn transform_from_map(map_transform: &mcf_map::MapTransform) -> Transform {
    let axis_x = Vec3::from_array(map_transform.basis[0].map(|value| value as f32));
    let axis_y = Vec3::from_array(map_transform.basis[1].map(|value| value as f32));
    let axis_z = Vec3::from_array(map_transform.basis[2].map(|value| value as f32));
    let scale = Vec3::new(axis_x.length(), axis_y.length(), axis_z.length());
    let rotation = Mat3::from_cols(axis_x / scale.x, axis_y / scale.y, axis_z / scale.z);
    Transform {
        translation: Vec3::from_array(map_transform.origin.map(|value| value as f32)),
        rotation: Quat::from_mat3(&rotation),
        scale,
    }
}

/// Reconstitue exactement les colonnes de base attendues par le format `.mcfmap`.
fn transform_to_map(transform: &Transform) -> mcf_map::MapTransform {
    let rotation = Mat3::from_quat(transform.rotation);
    mcf_map::MapTransform {
        origin: transform.translation.to_array().map(f64::from),
        basis: [
            (rotation.x_axis * transform.scale.x).to_array().map(f64::from),
            (rotation.y_axis * transform.scale.y).to_array().map(f64::from),
            (rotation.z_axis * transform.scale.z).to_array().map(f64::from),
        ],
    }
}

/// Remet les points du rendu (décalés de 12 cm pour éviter le z-fighting)
/// dans les coordonnées exactes du format `.mcfmap`.
fn sync_road_points(map: &mut mcf_map::McfMap, uid: &str, points: &[Vec3]) {
    if let Some(road) = map.roads.iter_mut().find(|road| road.uid == uid) {
        road.points = points
            .iter()
            .map(|point| [point.x as f64, (point.y - 0.12) as f64, point.z as f64])
            .collect();
    }
}

/// Génère un identifiant stable compatible avec le format sans dépendre du renderer.
fn next_item_uid(map: &mcf_map::McfMap) -> String {
    let mut ordinal = map.items.len() as u64 + 1;
    loop {
        let uid = format!("ed{ordinal:06x}");
        if map.items.iter().all(|item| item.uid != uid) {
            return uid;
        }
        ordinal += 1;
    }
}

fn next_road_uid(map: &mcf_map::McfMap) -> String {
    let mut ordinal = map.roads.len() as u64 + 1;
    loop {
        let uid = format!("ad{ordinal:06x}");
        let used = map.items.iter().any(|item| item.uid == uid)
            || map.roads.iter().any(|road| road.uid == uid)
            || map.junctions.iter().any(|junction| junction.uid == uid);
        if !used {
            return uid;
        }
        ordinal += 1;
    }
}

/// Enregistre uniquement à la demande explicite (`Ctrl+S` ou bouton), après la validation du format.
fn save_document(
    keys: Res<ButtonInput<KeyCode>>,
    mut request: ResMut<SaveRequest>,
    mut document: ResMut<MapDocument>,
    mut state: ResMut<EditorState>,
    objects: Query<(&McfUid, &Transform), With<EditorObject>>,
    roads: Query<(&McfUid, &RoadPreview), With<EditorObject>>,
) {
    let shortcut = (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight))
        && keys.just_pressed(KeyCode::KeyS);
    if !shortcut && !request.0 {
        return;
    }
    request.0 = false;

    let Some(path) = document.path.clone() else {
        state.message = "Aucun calque ouvert : chargez un fichier .mcfmap avant d’enregistrer.".into();
        return;
    };
    let result = {
        let Some(map) = &mut document.map else {
            state.message = "Aucun calque ouvert : chargez un fichier .mcfmap avant d’enregistrer.".into();
            return;
        };
        for item in &mut map.items {
            if let Some((_, transform)) = objects.iter().find(|(uid, _)| uid.0 == item.uid) {
                item.transform = transform_to_map(transform);
            }
        }
        for (uid, preview) in &roads {
            sync_road_points(map, &uid.0, &preview.points);
        }
        mcf_map::save_file(&path, map)
    };
    match result {
        Ok(()) => {
            document.dirty = false;
            state.message = format!("Calque enregistré : {}.", path.display());
        }
        Err(error) => state.message = format!("Enregistrement refusé : {error}"),
    }
}

/// Applique une annulation ou une répétition sans jamais écrire un fichier de lui-même.
fn apply_transform_history(
    keys: Res<ButtonInput<KeyCode>>,
    mut history: ResMut<TransformHistory>,
    mut document: ResMut<MapDocument>,
    mut state: ResMut<EditorState>,
    mut objects: Query<&mut Transform, With<EditorObject>>,
) {
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let undo = history.undo_requested || (control && !shift && keys.just_pressed(KeyCode::KeyZ));
    let redo = history.redo_requested
        || (control && (keys.just_pressed(KeyCode::KeyY) || (shift && keys.just_pressed(KeyCode::KeyZ))));
    history.undo_requested = false;
    history.redo_requested = false;

    if undo {
        if let Some(change) = history.undo.pop()
            && let Ok(mut transform) = objects.get_mut(change.entity)
        {
            *transform = change.before;
            history.redo.push(change);
            document.dirty = true;
            state.message = "Transformation annulée.".into();
        }
    } else if redo
        && let Some(change) = history.redo.pop()
        && let Ok(mut transform) = objects.get_mut(change.entity)
    {
        *transform = change.after;
        history.undo.push(change);
        document.dirty = true;
        state.message = "Transformation rétablie.".into();
    }
}

fn spawn_editor_object(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    name: String,
    catalog: String,
    uid: String,
    layer: String,
    transform: Transform,
) -> Entity {
    let hue = catalog.bytes().fold(0_u32, |sum, byte| sum + u32::from(byte)) % 3;
    let color = match hue {
        0 => Color::srgb(0.84, 0.43, 0.18),
        1 => Color::srgb(0.22, 0.55, 0.78),
        _ => Color::srgb(0.66, 0.42, 0.76),
    };
    commands
        .spawn((
            EditorObject {
                name,
                catalog: catalog.clone(),
            },
            McfUid(uid),
            CatalogRef(catalog),
            LayerName(layer),
            Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
            MeshMaterial3d(materials.add(color)),
            transform,
        ))
        .id()
}

fn update_viewports(
    window: Single<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(&EditorViewport, &mut Camera)>,
) {
    let full_size = UVec2::new(window.physical_width(), window.physical_height());
    let (map_position, map_size, scene_position, scene_size) = split_viewport_layout(full_size);
    for (viewport, mut camera) in &mut cameras {
        let (position, size) = match viewport {
            EditorViewport::Map2d => (map_position, map_size),
            EditorViewport::Scene3d => (scene_position, scene_size),
        };
        camera.viewport = Some(Viewport {
            physical_position: position,
            physical_size: size,
            ..default()
        });
    }
}

/// Les deux vues de travail restent de même largeur et parfaitement adjacentes.
fn split_viewport_layout(full_size: UVec2) -> (UVec2, UVec2, UVec2, UVec2) {
    let map_width = full_size.x / 2;
    let scene_width = full_size.x - map_width;
    let map_position = UVec2::ZERO;
    let scene_position = UVec2::new(map_width, 0);
    (
        map_position,
        UVec2::new(map_width, full_size.y),
        scene_position,
        UVec2::new(scene_width, full_size.y),
    )
}

fn move_scene_camera(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut cameras: Query<(&EditorViewport, &mut Transform)>,
) {
    let multiplier = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        3.0
    } else {
        1.0
    };
    let speed = 12.0 * multiplier * time.delta_secs();
    for (viewport, mut transform) in &mut cameras {
        match viewport {
            EditorViewport::Scene3d => {
                let forward = transform.forward().as_vec3();
                let right = transform.right().as_vec3();
                if keys.pressed(KeyCode::KeyW) {
                    transform.translation += forward * speed;
                }
                if keys.pressed(KeyCode::KeyS) {
                    transform.translation -= forward * speed;
                }
                if keys.pressed(KeyCode::KeyA) {
                    transform.translation -= right * speed;
                }
                if keys.pressed(KeyCode::KeyD) {
                    transform.translation += right * speed;
                }
            }
            EditorViewport::Map2d => {
                if keys.pressed(KeyCode::ArrowLeft) {
                    transform.translation.x -= speed;
                }
                if keys.pressed(KeyCode::ArrowRight) {
                    transform.translation.x += speed;
                }
                if keys.pressed(KeyCode::ArrowUp) {
                    transform.translation.z -= speed;
                }
                if keys.pressed(KeyCode::ArrowDown) {
                    transform.translation.z += speed;
                }
            }
        }
    }
}

/// Regarde autour de la scène avec le clic droit dans le viewport 3D.
fn look_scene_camera(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut motions: MessageReader<MouseMotion>,
    mut cameras: Query<(&EditorViewport, &mut Transform)>,
) {
    if !buttons.pressed(MouseButton::Right) {
        motions.clear();
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if cursor.x < window.width() * 0.5 {
        return;
    }
    let delta = motions.read().fold(Vec2::ZERO, |sum, motion| sum + motion.delta);
    if delta == Vec2::ZERO {
        return;
    }
    for (viewport, mut transform) in &mut cameras {
        if matches!(viewport, EditorViewport::Scene3d) {
            transform.rotate_y(-delta.x * 0.004);
            transform.rotate_local_x(-delta.y * 0.004);
        }
    }
}

/// Zoom continu de la carte orthographique : Q rapproche, E éloigne.
fn zoom_map_camera(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut cameras: Query<(&EditorViewport, &mut Projection)>,
) {
    let zoom = if keys.pressed(KeyCode::KeyQ) {
        -time.delta_secs() * 0.12
    } else if keys.pressed(KeyCode::KeyE) {
        time.delta_secs() * 0.12
    } else {
        return;
    };
    for (viewport, mut projection) in &mut cameras {
        if matches!(viewport, EditorViewport::Map2d)
            && let Projection::Orthographic(projection) = &mut *projection
        {
            projection.scale = (projection.scale + zoom).clamp(8.0, 80.0);
        }
    }
}

/// La molette ajuste l'échelle du plan lorsque le curseur est dans sa moitié de fenêtre.
fn zoom_map_with_wheel(
    window: Single<&Window, With<PrimaryWindow>>,
    mut wheels: MessageReader<MouseWheel>,
    mut cameras: Query<(&EditorViewport, &mut Projection)>,
) {
    let delta: f32 = wheels.read().map(|event| event.y).sum();
    if delta == 0.0 {
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if cursor.x < 350.0 || cursor.x >= window.width() * 0.5 {
        return;
    }
    for (viewport, mut projection) in &mut cameras {
        if matches!(viewport, EditorViewport::Map2d)
            && let Projection::Orthographic(projection) = &mut *projection
        {
            projection.scale = (projection.scale - delta * 2.0).clamp(8.0, 80.0);
        }
    }
}

/// Sélectionne l'objet le plus proche du rayon de clic dans le plan 2D ou la scène 3D.
fn select_object_in_viewport(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<EditorViewport>>,
    objects: Query<(Entity, &Transform, Option<&RoadPreview>), (With<EditorObject>, Without<EditorHidden>)>,
    mut state: ResMut<EditorState>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    // Les contrôles egui occupent cette zone et gardent la priorité sur la sélection monde.
    if cursor.x < 350.0 || cursor.y < 52.0 {
        return;
    }
    let Some((camera, camera_transform)) = cameras.iter().find(|(camera, _)| {
        camera
            .logical_viewport_rect()
            .is_some_and(|viewport| viewport.contains(cursor))
    }) else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    let direction = ray.direction.as_vec3();
    let mut nearest = None;
    for (entity, transform, road) in &objects {
        if let Some(road) = road {
            for segment in road.points.windows(2) {
                let road_y = (segment[0].y + segment[1].y) * 0.5;
                if direction.y.abs() < 0.000_1 {
                    continue;
                }
                let distance_along_ray = (road_y - ray.origin.y) / direction.y;
                if distance_along_ray < 0.0 {
                    continue;
                }
                let point = ray.origin + direction * distance_along_ray;
                if point_road_distance(point.xz(), segment[0], segment[1]) <= road.emprise_m() * 0.5
                    && nearest.is_none_or(|(_, distance)| distance_along_ray < distance)
                {
                    nearest = Some((entity, distance_along_ray));
                }
            }
            continue;
        }
        let to_object = transform.translation - ray.origin;
        let distance_along_ray = to_object.dot(direction);
        if distance_along_ray < 0.0 {
            continue;
        }
        let point_on_ray = ray.origin + direction * distance_along_ray;
        let selection_radius = transform.scale.max_element() * 0.8 + 0.35;
        if transform.translation.distance_squared(point_on_ray) <= selection_radius * selection_radius
            && nearest.is_none_or(|(_, distance)| distance_along_ray < distance)
        {
            nearest = Some((entity, distance_along_ray));
        }
    }
    let additive = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    select_entities(&mut state, nearest.map(|(entity, _)| entity), additive);
}

/// Déplace l'objet sélectionné sur son plan horizontal lorsqu'il est glissé dans la vue 3D.
fn drag_selected_in_scene(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    cameras: Query<(&EditorViewport, &Camera, &GlobalTransform)>,
    objects: Query<(&Transform, Option<&RoadPreview>), (With<EditorObject>, Without<EditorHidden>)>,
    locked: Query<(), With<EditorLocked>>,
    mut commands: Commands,
    mut state: ResMut<EditorState>,
    mut document: ResMut<MapDocument>,
    mut history: ResMut<TransformHistory>,
    mut drag: ResMut<DragState>,
) {
    if drag.viewport == Some(EditorViewport::Scene3d) && buttons.just_released(MouseButton::Left) {
        if commit_drag(&mut drag, &mut history, &mut document) {
            state.message = "Objet déplacé dans la scène 3D.".into();
        }
        return;
    }
    if !buttons.pressed(MouseButton::Left) || state.selected_entities.len() != 1 {
        return;
    }
    let (Some(cursor), Some(entity)) = (window.cursor_position(), state.selected) else {
        return;
    };
    let Some((_, camera, camera_transform)) = cameras.iter().find(|(viewport, camera, _)| {
        matches!(viewport, EditorViewport::Scene3d)
            && camera
                .logical_viewport_rect()
                .is_some_and(|rect| rect.contains(cursor))
    }) else {
        return;
    };
    let Ok((before, road)) = objects.get(entity) else {
        return;
    };
    if buttons.just_pressed(MouseButton::Left) {
        if locked.contains(entity) {
            return;
        }
        if road.is_some() {
            state.message =
                "Les routes se sélectionnent, mais l’édition de leurs points arrive ensuite.".into();
            return;
        }
        *drag = DragState {
            entity: Some(entity),
            before: Some(*before),
            after: Some(*before),
            viewport: Some(EditorViewport::Scene3d),
        };
        return;
    }
    if drag.viewport != Some(EditorViewport::Scene3d) || drag.entity != Some(entity) {
        return;
    }
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    let direction = ray.direction.as_vec3();
    if direction.y.abs() < 0.000_1 {
        return;
    }
    let distance = (before.translation.y - ray.origin.y) / direction.y;
    if distance < 0.0 {
        return;
    }
    let point = ray.origin + direction * distance;
    let mut after = *before;
    after.translation.x = point.x;
    after.translation.z = point.z;
    if state.snap_enabled {
        after.translation.x = after.translation.x.round();
        after.translation.z = after.translation.z.round();
    }
    if after == *before {
        return;
    }
    drag.after = Some(after);
    commands.entity(entity).insert(after);
}

fn clear_selection(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<EditorState>,
    mut drag: ResMut<DragState>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        select_entities(&mut state, None, false);
        state.selection_box_origin = None;
        *drag = DragState::default();
    }
}

fn select_all(
    keys: Res<ButtonInput<KeyCode>>,
    objects: Query<Entity, With<EditorObject>>,
    mut state: ResMut<EditorState>,
) {
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if control && keys.just_pressed(KeyCode::KeyA) {
        state.selected_entities = objects.iter().collect();
        state.selected = state.selected_entities.last().copied();
    }
}

fn draw_grid(mut gizmos: Gizmos, roads: Query<&RoadPreview>, state: Res<EditorState>) {
    if !state.grid_visible {
        return;
    }
    let color = Color::srgba(0.35, 0.55, 0.5, 0.22);
    for coordinate in (-20..=20).step_by(2) {
        let coordinate = coordinate as f32;
        gizmos.line(
            Vec3::new(coordinate, 0.05, -20.0),
            Vec3::new(coordinate, 0.05, 20.0),
            color,
        );
        gizmos.line(
            Vec3::new(-20.0, 0.05, coordinate),
            Vec3::new(20.0, 0.05, coordinate),
            color,
        );
    }
    for road in &roads {
        gizmos.linestrip(road.points.iter().copied(), Color::srgb(0.98, 0.78, 0.12));
    }
}

/// Rend immédiatement visibles, dans les deux vues, les objets de la sélection.
fn draw_selection_outline(
    mut gizmos: Gizmos,
    state: Res<EditorState>,
    objects: Query<&Transform, With<EditorObject>>,
) {
    for entity in &state.selected_entities {
        let Ok(transform) = objects.get(*entity) else {
            continue;
        };
        // Les objets de l'éditeur sont des cuboïdes unitaires : 0,5 est donc
        // exactement leur demi-emprise. La marge précédente (0,56) rendait
        // visuellement la boîte de sélection sensiblement trop grande.
        let half = Vec3::splat(0.505);
        let corners = [
            Vec3::new(-half.x, -half.y, -half.z),
            Vec3::new(half.x, -half.y, -half.z),
            Vec3::new(half.x, -half.y, half.z),
            Vec3::new(-half.x, -half.y, half.z),
            Vec3::new(-half.x, half.y, -half.z),
            Vec3::new(half.x, half.y, -half.z),
            Vec3::new(half.x, half.y, half.z),
            Vec3::new(-half.x, half.y, half.z),
        ]
        .map(|corner| transform.transform_point(corner));
        let color = Color::srgb(1.0, 0.84, 0.18);
        for (start, end) in [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 0),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ] {
            gizmos.line(corners[start], corners[end], color);
        }
    }
}

/// Empreinte au sol du cuboïde rendu en 3D, projetée dans le canevas 2D.
/// Elle reprend position, échelle et rotation : le plan n'utilise pas d'icône
/// arbitraire qui divergerait de la scène.
fn plan_footprint(transform: &Transform, center: egui::Pos2, pixels_per_meter: f32) -> [egui::Pos2; 4] {
    [
        Vec3::new(-0.5, 0.0, -0.5),
        Vec3::new(0.5, 0.0, -0.5),
        Vec3::new(0.5, 0.0, 0.5),
        Vec3::new(-0.5, 0.0, 0.5),
    ]
    .map(|corner| {
        let world = transform.transform_point(corner);
        egui::pos2(
            center.x + world.x * pixels_per_meter,
            center.y + world.z * pixels_per_meter,
        )
    })
}

fn point_in_plan_footprint(point: egui::Pos2, footprint: &[egui::Pos2; 4]) -> bool {
    let mut direction = 0.0_f32;
    for index in 0..footprint.len() {
        let start = footprint[index];
        let end = footprint[(index + 1) % footprint.len()];
        let cross = (end.x - start.x) * (point.y - start.y) - (end.y - start.y) * (point.x - start.x);
        if cross.abs() < 0.01 {
            continue;
        }
        if direction == 0.0 {
            direction = cross.signum();
        } else if direction * cross < 0.0 {
            return false;
        }
    }
    true
}

fn point_segment_distance(point: egui::Pos2, start: egui::Pos2, end: egui::Pos2) -> f32 {
    let segment = end - start;
    let length_squared = segment.length_sq();
    if length_squared <= f32::EPSILON {
        return point.distance(start);
    }
    let position = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance(start + segment * position)
}

fn point_road_distance(point: Vec2, start: Vec3, end: Vec3) -> f32 {
    let start = start.xz();
    let segment = end.xz() - start;
    let length_squared = segment.length_squared();
    if length_squared <= f32::EPSILON {
        return point.distance(start);
    }
    let position = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance(start + segment * position)
}

fn editor_ui(
    mut contexts: EguiContexts,
    window: Single<&Window, With<PrimaryWindow>>,
    mut state: ResMut<EditorState>,
    objects: Query<(Entity, &EditorObject, &Transform, &LayerName, &McfUid), Without<EditorViewport>>,
    locked: Query<(), With<EditorLocked>>,
    hidden: Query<(), With<EditorHidden>>,
    mut road_entities: ParamSet<(
        Query<(Entity, &RoadPreview)>,
        Query<Entity, (With<RoadPreview>, Without<EditorObject>)>,
        Query<(Entity, &RoadSurfaceOwner)>,
    )>,
    mut cameras: Query<(&EditorViewport, &mut Transform)>,
    mut commands: Commands,
    mut document: ResMut<MapDocument>,
    mut save_request: ResMut<SaveRequest>,
    mut history: ResMut<TransformHistory>,
    mut drag: ResMut<DragState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) -> Result {
    let context = contexts.ctx_mut()?;
    egui::Area::new("barre_outils".into())
        .fixed_pos(egui::pos2(10.0, 8.0))
        .show(context, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_black_alpha(215))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 7))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong("MCF Editor");
                        ui.separator();
                        ui.label("Carte 2D + scène 3D — même monde");
                        ui.separator();
                        ui.label("3D : WASD, Shift, clic droit · Plan : flèches, Q / E");
                        ui.separator();
                        if ui
                            .button(if state.grid_visible {
                                "Grille : ON"
                            } else {
                                "Grille : OFF"
                            })
                            .clicked()
                        {
                            state.grid_visible = !state.grid_visible;
                        }
                        if ui
                            .button(if state.snap_enabled {
                                "Magnétisme : ON"
                            } else {
                                "Magnétisme : OFF"
                            })
                            .clicked()
                        {
                            state.snap_enabled = !state.snap_enabled;
                        }
                        if ui.button("Tout sélectionner").clicked() {
                            state.selected_entities =
                                objects.iter().map(|(entity, _, _, _, _)| entity).collect();
                            state.selected = state.selected_entities.last().copied();
                        }
                        ui.separator();
                        let dirty_marker = if document.dirty { " *" } else { "" };
                        ui.label(format!("{}{dirty_marker}", state.source));
                    });
                });
        });
    egui::Area::new("outils".into())
        .fixed_pos(egui::pos2(10.0, 52.0))
        .show(context, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_black_alpha(225))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(72)))
                .corner_radius(6)
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(315.0);
                    ui.heading("Hiérarchie");
                    ui.small("Calque actif");
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("hierarchie")
                        .max_height(130.0)
                        .show(ui, |ui| {
                            for (entity, object, _, layer, _) in &objects {
                                let selected = state.selected_entities.contains(&entity);
                                let lock_mark = if locked.contains(entity) { "🔒 " } else { "" };
                                let hidden_mark = if hidden.contains(entity) { "🙈 " } else { "" };
                                let label = format!("{hidden_mark}{lock_mark}{}", object.name);
                                if ui.selectable_label(selected, label).clicked() {
                                    let additive = ui.input(|input| input.modifiers.ctrl);
                                    select_entities(&mut state, Some(entity), additive);
                                }
                                if selected {
                                    ui.small(format!("{} · {}", layer.0, object.catalog));
                                }
                            }
                        });
                    ui.add_space(6.0);
                    ui.heading("Catalogue");
                    ui.add(
                        egui::TextEdit::singleline(&mut state.catalog_query)
                            .hint_text("Rechercher un catalogue"),
                    );
                    let query = state.catalog_query.to_lowercase();
                    let mut catalogs: Vec<String> = objects
                        .iter()
                        .map(|(_, object, _, _, _)| object.catalog.clone())
                        .collect();
                    catalogs.sort();
                    catalogs.dedup();
                    egui::ScrollArea::vertical()
                        .id_salt("catalogue")
                        .max_height(82.0)
                        .show(ui, |ui| {
                            for catalog in catalogs
                                .into_iter()
                                .filter(|catalog| catalog.to_lowercase().contains(&query))
                            {
                                let selected_catalog = state.selected_catalog.as_deref() == Some(&catalog);
                                if ui.selectable_label(selected_catalog, &catalog).clicked() {
                                    state.selected_catalog = Some(catalog);
                                }
                            }
                        });
                    let can_place = document.map.is_some() && state.selected_catalog.is_some();
                    if ui
                        .add_enabled(can_place, egui::Button::new("Poser l’objet sélectionné"))
                        .clicked()
                    {
                        let catalog = state.selected_catalog.clone().expect("catalogue activé");
                        let mut placement = state
                            .selected
                            .and_then(|entity| objects.get(entity).ok())
                            .map(|(_, _, transform, _, _)| *transform)
                            .unwrap_or_else(|| Transform::from_xyz(0.0, 0.5, 0.0));
                        placement.translation += Vec3::new(2.0, 0.0, 0.0);
                        let created = document.map.as_mut().map(|map| {
                            let uid = next_item_uid(map);
                            let layer = map.layer.clone();
                            map.items.push(mcf_map::MapItem {
                                uid: uid.clone(),
                                catalog: catalog.clone(),
                                transform: transform_to_map(&placement),
                                props: Default::default(),
                            });
                            (uid, layer)
                        });
                        if let Some((uid, layer)) = created {
                            let entity = spawn_editor_object(
                                &mut commands,
                                &mut meshes,
                                &mut materials,
                                catalog.rsplit('/').next().unwrap_or(&catalog).to_owned(),
                                catalog,
                                uid,
                                layer,
                                placement,
                            );
                            select_entities(&mut state, Some(entity), false);
                            document.dirty = true;
                            state.message = "Nouvel objet posé dans le calque.".into();
                        }
                    }
                    ui.add_space(8.0);
                    ui.heading("Inspecteur");
                    if state.selected_entities.len() > 1 {
                        ui.small(format!(
                            "{} objets sélectionnés — propriétés de l'objet principal ci-dessous.",
                            state.selected_entities.len()
                        ));
                    }
                    ui.separator();
                    let selected = state.selected.and_then(|entity| {
                        objects
                            .get(entity)
                            .ok()
                            .map(|(_, object, transform, layer, uid)| {
                                (
                                    entity,
                                    object.name.clone(),
                                    object.catalog.clone(),
                                    layer.0.clone(),
                                    uid.0.clone(),
                                    *transform,
                                )
                            })
                    });
                    if let Some((entity, name, catalog, layer, uid, mut transform)) = selected {
                        let is_locked = locked.contains(entity);
                        let is_hidden = hidden.contains(entity);
                        let is_road = road_entities.p0().get(entity).is_ok();
                        let before = transform;
                        ui.label(format!("Nom : {name}"));
                        ui.label(format!("Catalogue : {catalog}"));
                        ui.label(format!("Calque : {layer}"));
                        if ui
                            .button(if is_locked {
                                "Déverrouiller"
                            } else {
                                "Verrouiller"
                            })
                            .clicked()
                        {
                            if is_locked {
                                commands.entity(entity).remove::<EditorLocked>();
                                state.message = format!("« {name} » déverrouillé.");
                            } else {
                                commands.entity(entity).insert(EditorLocked);
                                state.message = format!("« {name} » verrouillé.");
                            }
                        }
                        if is_locked {
                            ui.colored_label(
                                egui::Color32::LIGHT_YELLOW,
                                "Objet verrouillé : déplacement interdit.",
                            );
                        }
                        if is_road {
                            ui.colored_label(
                                egui::Color32::LIGHT_BLUE,
                                "Route : ajoutez ou retirez un point final ; la géométrie 2D/3D est régénérée.",
                            );
                            let road_snapshot = road_entities
                                .p0()
                                .get(entity)
                                .ok()
                                .map(|(_, road)| road.clone());
                            if let Some(mut road) = road_snapshot {
                                ui.horizontal(|ui| {
                                    if ui.button("+ Point final").clicked() {
                                        let last = road.points.last().copied().unwrap_or(Vec3::ZERO);
                                        road.points.push(last + Vec3::X * 4.0);
                                        for (surface, owner) in &road_entities.p2() {
                                            if owner.0 == entity {
                                                commands.entity(surface).despawn();
                                            }
                                        }
                                        spawn_road_surfaces(
                                            &mut commands,
                                            &mut meshes,
                                            &mut materials,
                                            entity,
                                            &road,
                                        );
                                        commands.entity(entity).insert(road.clone());
                                        document.dirty = true;
                                        state.message = "Point de route ajouté.".into();
                                    }
                                    if ui
                                        .add_enabled(road.points.len() > 2, egui::Button::new("− Point final"))
                                        .clicked()
                                    {
                                        road.points.pop();
                                        for (surface, owner) in &road_entities.p2() {
                                            if owner.0 == entity {
                                                commands.entity(surface).despawn();
                                            }
                                        }
                                        spawn_road_surfaces(
                                            &mut commands,
                                            &mut meshes,
                                            &mut materials,
                                            entity,
                                            &road,
                                        );
                                        commands.entity(entity).insert(road.clone());
                                        document.dirty = true;
                                        state.message = "Point final de route retiré.".into();
                                    }
                                });
                                let mut points_changed = false;
                                for (index, point) in road.points.iter_mut().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("Point {index}"));
                                        points_changed |= ui
                                            .add(egui::DragValue::new(&mut point.x).speed(0.1).prefix("X "))
                                            .changed();
                                        points_changed |= ui
                                            .add(egui::DragValue::new(&mut point.y).speed(0.1).prefix("Y "))
                                            .changed();
                                        points_changed |= ui
                                            .add(egui::DragValue::new(&mut point.z).speed(0.1).prefix("Z "))
                                            .changed();
                                    });
                                }
                                if points_changed {
                                    for (surface, owner) in &road_entities.p2() {
                                        if owner.0 == entity {
                                            commands.entity(surface).despawn();
                                        }
                                    }
                                    spawn_road_surfaces(
                                        &mut commands,
                                        &mut meshes,
                                        &mut materials,
                                        entity,
                                        &road,
                                    );
                                    commands.entity(entity).insert(road.clone());
                                    document.dirty = true;
                                    state.message = "Points de route modifiés.".into();
                                }
                                if ui.button("Dupliquer la route").clicked() {
                                    let duplicated = document.map.as_mut().and_then(|map| {
                                        let mut duplicate = map.roads.iter().find(|entry| entry.uid == uid).cloned()?;
                                        duplicate.uid = next_road_uid(map);
                                        for point in &mut duplicate.points {
                                            point[0] += 2.0;
                                            point[2] += 2.0;
                                        }
                                        let layer = map.layer.clone();
                                        map.roads.push(duplicate.clone());
                                        Some((duplicate, layer))
                                    });
                                    if let Some((duplicate, layer)) = duplicated {
                                        let points = duplicate
                                            .points
                                            .iter()
                                            .map(|point| {
                                                Vec3::new(point[0] as f32, point[1] as f32 + 0.12, point[2] as f32)
                                            })
                                            .collect::<Vec<_>>();
                                        let road_entity = spawn_road_preview(
                                            &mut commands,
                                            &mut meshes,
                                            &mut materials,
                                            points.clone(),
                                            road_class(&duplicate.template),
                                        );
                                        commands.entity(road_entity).insert((
                                            EditorObject {
                                                name: format!("Route {} (copie)", duplicate.uid),
                                                catalog: duplicate.template.clone(),
                                            },
                                            McfUid(duplicate.uid),
                                            CatalogRef(duplicate.template),
                                            LayerName(layer),
                                            Transform::from_translation(
                                                points.first().copied().unwrap_or(Vec3::ZERO),
                                            ),
                                        ));
                                        select_entities(&mut state, Some(road_entity), false);
                                        document.dirty = true;
                                        state.message = "Route dupliquée.".into();
                                    }
                                }
                                if ui.button("Supprimer la route").clicked() {
                                    if let Some(map) = &mut document.map {
                                        map.roads.retain(|road| road.uid != uid);
                                    }
                                    for (surface, owner) in &road_entities.p2() {
                                        if owner.0 == entity {
                                            commands.entity(surface).despawn();
                                        }
                                    }
                                    commands.entity(entity).despawn();
                                    select_entities(&mut state, None, false);
                                    document.dirty = true;
                                    state.message = format!("Route « {name} » supprimée.");
                                }
                            }
                        }
                        if ui
                            .button(if is_hidden { "Afficher" } else { "Masquer" })
                            .clicked()
                        {
                            if is_hidden {
                                commands
                                    .entity(entity)
                                    .remove::<EditorHidden>()
                                    .insert(Visibility::Visible);
                                state.message = format!("« {name} » affiché.");
                            } else {
                                commands.entity(entity).insert((EditorHidden, Visibility::Hidden));
                                state.message = format!("« {name} » masqué.");
                            }
                        }
                        if ui.button("Cadrer dans la vue 3D").clicked() {
                            let target = transform.translation;
                            let distance = transform.scale.max_element().max(1.0) * 3.5;
                            for (viewport, mut camera_transform) in &mut cameras {
                                if matches!(viewport, EditorViewport::Scene3d) {
                                    *camera_transform = Transform::from_translation(
                                        target + Vec3::new(distance, distance * 0.7, distance),
                                    )
                                    .looking_at(target, Vec3::Y);
                                }
                            }
                        }
                        ui.separator();
                        ui.label("Position (mètres)");
                        let mut changed = false;
                        ui.horizontal(|ui| {
                            ui.label("X");
                            changed |= ui
                                .add(egui::DragValue::new(&mut transform.translation.x).speed(0.1))
                                .changed();
                            ui.label("Y");
                            changed |= ui
                                .add(egui::DragValue::new(&mut transform.translation.y).speed(0.1))
                                .changed();
                            ui.label("Z");
                            changed |= ui
                                .add(egui::DragValue::new(&mut transform.translation.z).speed(0.1))
                                .changed();
                        });
                        ui.small("Saisissez une valeur ou glissez sur les champs.");
                        ui.label("Échelle");
                        ui.horizontal(|ui| {
                            ui.label("X");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut transform.scale.x)
                                        .speed(0.05)
                                        .range(0.1..=10.0),
                                )
                                .changed();
                            ui.label("Y");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut transform.scale.y)
                                        .speed(0.05)
                                        .range(0.1..=10.0),
                                )
                                .changed();
                            ui.label("Z");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut transform.scale.z)
                                        .speed(0.05)
                                        .range(0.1..=10.0),
                                )
                                .changed();
                        });
                        ui.label("Rotation (degrés)");
                        let (rotation_x, rotation_y, rotation_z) = transform.rotation.to_euler(EulerRot::XYZ);
                        let mut rotation_x = rotation_x.to_degrees();
                        let mut rotation_y = rotation_y.to_degrees();
                        let mut rotation_z = rotation_z.to_degrees();
                        ui.horizontal(|ui| {
                            ui.label("X");
                            changed |= ui.add(egui::DragValue::new(&mut rotation_x).speed(1.0)).changed();
                            ui.label("Y");
                            changed |= ui.add(egui::DragValue::new(&mut rotation_y).speed(1.0)).changed();
                            ui.label("Z");
                            changed |= ui.add(egui::DragValue::new(&mut rotation_z).speed(1.0)).changed();
                        });
                        if changed && !is_locked && !is_road {
                            if state.snap_enabled {
                                transform.translation = transform.translation.round();
                            }
                            transform.rotation = Quat::from_euler(
                                EulerRot::XYZ,
                                rotation_x.to_radians(),
                                rotation_y.to_radians(),
                                rotation_z.to_radians(),
                            );
                            history.undo.push(TransformChange {
                                entity,
                                before,
                                after: transform,
                            });
                            history.redo.clear();
                            commands.entity(entity).insert(transform);
                            document.dirty = true;
                            state.message = format!("Transformation de « {name} » mise à jour.");
                        } else if changed {
                            state.message = format!("« {name} » est verrouillé.");
                        }
                        let can_edit_item = document
                            .map
                            .as_ref()
                            .is_some_and(|map| map.items.iter().any(|item| item.uid == uid));
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(can_edit_item && !is_locked, egui::Button::new("Dupliquer"))
                                .clicked()
                            {
                                let duplicated = document.map.as_mut().and_then(|map| {
                                    let mut item = map.items.iter().find(|item| item.uid == uid).cloned()?;
                                    let mut duplicate_transform = transform;
                                    duplicate_transform.translation += Vec3::new(1.0, 0.0, 1.0);
                                    item.uid = next_item_uid(map);
                                    item.transform = transform_to_map(&duplicate_transform);
                                    map.items.push(item.clone());
                                    Some((item, duplicate_transform))
                                });
                                if let Some((item, duplicate_transform)) = duplicated {
                                    let duplicate_entity = spawn_editor_object(
                                        &mut commands,
                                        &mut meshes,
                                        &mut materials,
                                        format!("{name} (copie)"),
                                        item.catalog,
                                        item.uid,
                                        layer.clone(),
                                        duplicate_transform,
                                    );
                                    select_entities(&mut state, Some(duplicate_entity), false);
                                    document.dirty = true;
                                    state.message = format!("« {name} » dupliqué.");
                                }
                            }
                            if ui
                                .add_enabled(can_edit_item && !is_locked, egui::Button::new("Supprimer"))
                                .clicked()
                            {
                                if let Some(map) = &mut document.map {
                                    map.items.retain(|item| item.uid != uid);
                                }
                                commands.entity(entity).despawn();
                                select_entities(&mut state, None, false);
                                document.dirty = true;
                                state.message = format!("« {name} » supprimé.");
                            }
                        });
                    } else {
                        ui.label("Sélectionnez un objet dans la hiérarchie.");
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!history.undo.is_empty(), egui::Button::new("Annuler"))
                            .clicked()
                        {
                            history.undo_requested = true;
                        }
                        if ui
                            .add_enabled(!history.redo.is_empty(), egui::Button::new("Refaire"))
                            .clicked()
                        {
                            history.redo_requested = true;
                        }
                    });
                    ui.small("Ctrl+Z / Ctrl+Y");
                    if ui.button("Réinitialiser la vue 3D").clicked() {
                        for (viewport, mut transform) in &mut cameras {
                            if matches!(viewport, EditorViewport::Scene3d) {
                                *transform = Transform::from_xyz(22.0, 16.0, 22.0)
                                    .looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y);
                            }
                        }
                    }
                    if ui.button("Centrer le plan 2D").clicked() {
                        for (viewport, mut transform) in &mut cameras {
                            if matches!(viewport, EditorViewport::Map2d) {
                                *transform =
                                    Transform::from_xyz(0.0, 60.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z);
                            }
                        }
                    }
                    ui.separator();
                    if ui
                        .add_enabled(document.map.is_none(), egui::Button::new("Créer un calque local"))
                        .clicked()
                    {
                        let mut map =
                            mcf_map::McfMap::new("nouveau_calque", "MCF Editor", "2026-11-21T00:00:00", 1);
                        map.items = objects
                            .iter()
                            .map(|(_, object, transform, _, uid)| mcf_map::MapItem {
                                uid: uid.0.clone(),
                                catalog: object.catalog.clone(),
                                transform: transform_to_map(transform),
                                props: Default::default(),
                            })
                            .collect();
                        document.path = None;
                        document.map = Some(map);
                        document.dirty = true;
                        state.save_path_input = "nouveau_calque.mcfmap".into();
                        state.source = "nouveau calque (non enregistré)".into();
                        state.message =
                            "Calque local prêt : choisissez un fichier de destination avant d’enregistrer."
                                .into();
                    }
                    if document.map.is_none() {
                        ui.small(
                            "Crée un calque à partir de la scène de démonstration, sans écrire de fichier.",
                        );
                    }
                    if document.map.is_some() && document.path.is_none() {
                        ui.separator();
                        ui.label("Fichier de destination");
                        ui.add(
                            egui::TextEdit::singleline(&mut state.save_path_input)
                                .hint_text("C:/.../ma_carte.mcfmap"),
                        );
                        if ui.button("Utiliser ce chemin").clicked() {
                            let candidate = PathBuf::from(state.save_path_input.trim());
                            if is_mcfmap_path(&candidate) {
                                state.source = candidate.display().to_string();
                                document.path = Some(candidate);
                                state.message =
                                    "Destination définie : cliquez sur Enregistrer pour écrire le calque."
                                        .into();
                            } else {
                                state.message =
                                    "Le fichier de destination doit se terminer par .mcfmap.".into();
                            }
                        }
                    }
                    ui.separator();
                    let save_label = if document.dirty {
                        "Enregistrer le calque *"
                    } else {
                        "Enregistrer le calque"
                    };
                    if ui
                        .add_enabled(document.path.is_some(), egui::Button::new(save_label))
                        .clicked()
                    {
                        save_request.0 = true;
                    }
                    if document.path.is_none() {
                        ui.small("Disponible lorsqu’un fichier .mcfmap est ouvert.");
                    }
                    ui.separator();
                    ui.strong("Chargement de carte");
                    ui.add(
                        egui::TextEdit::singleline(&mut state.open_path_input)
                            .hint_text("C:/.../carte.mcfmap"),
                    );
                    if ui.button("Ouvrir ce calque").clicked() {
                        let candidate = PathBuf::from(state.open_path_input.trim());
                        if document.dirty {
                            state.message =
                                "Enregistrez ou annulez vos modifications avant d’ouvrir un autre calque."
                                    .into();
                        } else if !is_mcfmap_path(&candidate) {
                            state.message = "Le fichier à ouvrir doit se terminer par .mcfmap.".into();
                        } else {
                            match mcf_map::load_file(&candidate, None) {
                                Ok(loaded) => {
                                    for (entity, _, _, _, _) in &objects {
                                        commands.entity(entity).despawn();
                                    }
                                    for entity in &road_entities.p1() {
                                        commands.entity(entity).despawn();
                                    }
                                    for (entity, _) in &road_entities.p2() {
                                        commands.entity(entity).despawn();
                                    }
                                    let count = spawn_loaded_map(
                                        &loaded.map,
                                        &mut commands,
                                        &mut meshes,
                                        &mut materials,
                                    );
                                    document.path = Some(candidate.clone());
                                    document.map = Some(loaded.map);
                                    document.dirty = false;
                                    history.undo.clear();
                                    history.redo.clear();
                                    select_entities(&mut state, None, false);
                                    state.source = candidate.display().to_string();
                                    state.message = format!("Calque chargé : {count} élément(s).");
                                }
                                Err(error) => {
                                    state.message = format!("Chargement refusé : {error}");
                                }
                            }
                        }
                    }
                    ui.small("Ou : cargo editor -- chemin/vers/calque.mcfmap");
                });
        });
    // Le rendu orthographique Bevy reste la source des interactions de carte,
    // tandis que ce canevas dessine un plan lisible au même endroit. Cela évite
    // qu'une interface egui à plusieurs caméras masque le plan sous Windows.
    let map_rect = egui::Rect::from_min_max(
        egui::pos2(360.0, 84.0),
        egui::pos2((window.width() * 0.5 - 5.0).max(361.0), window.height() - 4.0),
    );
    let roads = road_entities.p0();
    egui::Area::new("plan_2d".into())
        .fixed_pos(map_rect.min)
        .show(context, |ui| {
            ui.set_min_size(map_rect.size());
            let (response, painter) = ui.allocate_painter(map_rect.size(), egui::Sense::click_and_drag());
            let rect = response.rect;
            painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(15, 31, 39));
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, egui::Color32::from_gray(55)),
                egui::StrokeKind::Inside,
            );
            let pixels_per_meter = ((rect.width().min(rect.height()) - 28.0) / 42.0).max(2.0);
            let center = rect.center();
            for coordinate in (-20..=20).step_by(2) {
                let offset = coordinate as f32 * pixels_per_meter;
                let grid = egui::Color32::from_rgba_unmultiplied(83, 137, 118, 72);
                painter.line_segment(
                    [
                        egui::pos2(center.x + offset, rect.top()),
                        egui::pos2(center.x + offset, rect.bottom()),
                    ],
                    egui::Stroke::new(1.0, grid),
                );
                painter.line_segment(
                    [
                        egui::pos2(rect.left(), center.y + offset),
                        egui::pos2(rect.right(), center.y + offset),
                    ],
                    egui::Stroke::new(1.0, grid),
                );
            }
            let mut nearest = None;
            let pointer = response.interact_pointer_pos();
            for (entity, road) in &roads {
                // Mêmes bandes que le ruban 3D (`mcf_geo`) : le plan ne redessine pas la route à sa façon.
                let selected = state.selected_entities.contains(&entity);
                let plan = |p: mcf_geo::P2| {
                    egui::pos2(
                        center.x + p[0] as f32 * pixels_per_meter,
                        center.y + p[1] as f32 * pixels_per_meter,
                    )
                };
                for bande in road.bandes() {
                    let fill = if selected {
                        egui::Color32::YELLOW
                    } else {
                        let [r, g, b] = band_color(road, bande.nature).to_srgba().to_u8_array_no_alpha();
                        egui::Color32::from_rgb(r, g, b)
                    };
                    for quad in bande.quads() {
                        painter.add(egui::Shape::convex_polygon(
                            quad.iter().map(|p| plan(*p)).collect(),
                            fill,
                            egui::Stroke::NONE,
                        ));
                    }
                }
                for segment in road.points.windows(2) {
                    let to_point = |point: Vec3| {
                        egui::pos2(
                            center.x + point.x * pixels_per_meter,
                            center.y + point.z * pixels_per_meter,
                        )
                    };
                    if state.selected_entities.contains(&entity) {
                        for point in [segment[0], segment[1]] {
                            painter.circle_filled(to_point(point), 5.0, egui::Color32::YELLOW);
                            painter.circle_stroke(
                                to_point(point),
                                5.0,
                                egui::Stroke::new(1.0, egui::Color32::BLACK),
                            );
                        }
                    }
                    if let Some(pointer) = pointer {
                        let distance =
                            point_segment_distance(pointer, to_point(segment[0]), to_point(segment[1]));
                        if distance <= road.emprise_m() * pixels_per_meter * 0.5
                            && nearest.is_none_or(|(_, best)| distance < best)
                        {
                            nearest = Some((entity, distance));
                        }
                    }
                }
            }
            for (entity, object, transform, _, _) in &objects {
                if hidden.contains(entity) || roads.get(entity).is_ok() {
                    continue;
                }
                let footprint = plan_footprint(transform, center, pixels_per_meter);
                let object_center = egui::pos2(
                    center.x + transform.translation.x * pixels_per_meter,
                    center.y + transform.translation.z * pixels_per_meter,
                );
                let color = if object.catalog.contains("road") {
                    egui::Color32::from_rgb(214, 181, 76)
                } else {
                    egui::Color32::from_rgb(184, 121, 214)
                };
                painter.add(egui::Shape::convex_polygon(
                    footprint.to_vec(),
                    color,
                    egui::Stroke::new(1.0, egui::Color32::from_black_alpha(150)),
                ));
                if state.selected_entities.contains(&entity) {
                    for index in 0..footprint.len() {
                        painter.line_segment(
                            [footprint[index], footprint[(index + 1) % footprint.len()]],
                            egui::Stroke::new(2.0, egui::Color32::YELLOW),
                        );
                    }
                }
                if let Some(pointer) = pointer
                    && point_in_plan_footprint(pointer, &footprint)
                {
                    let center_distance = pointer.distance(object_center);
                    if nearest.is_none_or(|(_, best)| center_distance < best) {
                        nearest = Some((entity, center_distance));
                    }
                }
            }
            if response.drag_started_by(egui::PointerButton::Primary) {
                let additive = context.input(|input| input.modifiers.ctrl);
                select_entities(&mut state, nearest.map(|(entity, _)| entity), additive);
                if let Some((entity, _)) = nearest
                    && let Ok((_, object, before, _, _)) = objects.get(entity)
                {
                    if locked.contains(entity) {
                        state.message = format!("« {} » est verrouillé.", object.name);
                    } else if roads.get(entity).is_ok() {
                        state.message =
                            "Les routes se sélectionnent, mais l’édition de leurs points arrive ensuite."
                                .into();
                    } else {
                        *drag = DragState {
                            entity: Some(entity),
                            before: Some(*before),
                            after: Some(*before),
                            viewport: Some(EditorViewport::Map2d),
                        };
                    }
                } else {
                    state.selection_box_origin = response.interact_pointer_pos();
                }
            }
            if response.dragged_by(egui::PointerButton::Primary)
                && drag.viewport == Some(EditorViewport::Map2d)
                && let (Some(entity), Some(pointer)) = (drag.entity, response.interact_pointer_pos())
                && let Ok((_, object, before, _, _)) = objects.get(entity)
            {
                let mut after = *before;
                after.translation.x = (pointer.x - center.x) / pixels_per_meter;
                after.translation.z = (pointer.y - center.y) / pixels_per_meter;
                if state.snap_enabled {
                    after.translation.x = after.translation.x.round();
                    after.translation.z = after.translation.z.round();
                }
                if after != *before {
                    drag.after = Some(after);
                    commands.entity(entity).insert(after);
                    state.message = format!("« {} » déplacé dans le plan 2D.", object.name);
                }
            }
            if let (Some(origin), Some(pointer)) =
                (state.selection_box_origin, response.interact_pointer_pos())
                && response.dragged_by(egui::PointerButton::Primary)
            {
                painter.rect_stroke(
                    egui::Rect::from_two_pos(origin, pointer),
                    0.0,
                    egui::Stroke::new(1.0, egui::Color32::from_rgb(112, 205, 255)),
                    egui::StrokeKind::Outside,
                );
            }
            if response.drag_stopped_by(egui::PointerButton::Primary) {
                if drag.viewport == Some(EditorViewport::Map2d) {
                    let _ = commit_drag(&mut drag, &mut history, &mut document);
                }
                if let (Some(origin), Some(pointer)) =
                    (state.selection_box_origin.take(), response.interact_pointer_pos())
                {
                    let selection = egui::Rect::from_two_pos(origin, pointer);
                    for (entity, _, transform, _, _) in &objects {
                        if hidden.contains(entity) {
                            continue;
                        }
                        let point = egui::pos2(
                            center.x + transform.translation.x * pixels_per_meter,
                            center.y + transform.translation.z * pixels_per_meter,
                        );
                        if selection.contains(point) {
                            select_entities(&mut state, Some(entity), true);
                        }
                    }
                }
            } else if response.clicked() {
                let additive = context.input(|input| input.modifiers.ctrl);
                select_entities(&mut state, nearest.map(|(entity, _)| entity), additive);
            }
            painter.text(
                rect.left_top() + egui::vec2(12.0, 12.0),
                egui::Align2::LEFT_TOP,
                "PLAN 2D — même carte",
                egui::FontId::proportional(15.0),
                egui::Color32::WHITE,
            );
        });
    egui::Area::new("etat".into())
        .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(10.0, -10.0))
        .show(context, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_black_alpha(180))
                .corner_radius(5)
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| ui.small(&state.message));
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_pose_mcf_conserve_position_rotation_et_echelle() {
        let source = mcf_map::MapTransform {
            origin: [12.0, 3.0, -7.0],
            basis: [[0.0, 0.0, -2.0], [0.0, 3.0, 0.0], [4.0, 0.0, 0.0]],
        };

        let transform = transform_from_map(&source);

        assert_eq!(transform.translation, Vec3::new(12.0, 3.0, -7.0));
        assert_eq!(transform.scale, Vec3::new(2.0, 3.0, 4.0));
        assert!((transform.rotation * Vec3::X - Vec3::NEG_Z).length() < 0.000_1);
        assert!((transform.rotation * Vec3::Y - Vec3::Y).length() < 0.000_1);
        assert!((transform.rotation * Vec3::Z - Vec3::X).length() < 0.000_1);

        let exported = transform_to_map(&transform);
        assert_eq!(exported.origin, source.origin);
        for (actual, expected) in exported.basis.into_iter().zip(source.basis) {
            for (actual, expected) in actual.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 0.000_1);
            }
        }
    }

    #[test]
    fn les_deux_vues_se_partagent_la_fenetre() {
        let (map_position, map_size, scene_position, scene_size) =
            split_viewport_layout(UVec2::new(1440, 840));

        assert_eq!(map_position, UVec2::ZERO);
        assert_eq!(map_size, UVec2::new(720, 840));
        assert_eq!(scene_position, UVec2::new(720, 0));
        assert_eq!(scene_size, UVec2::new(720, 840));
    }

    #[test]
    fn un_uid_de_duplication_est_valide_et_inutilise() {
        let mut map = mcf_map::McfMap::new("test", "test", "2026-11-21T00:00:00", 1);
        map.items.push(mcf_map::MapItem {
            uid: "ed000001".into(),
            catalog: "props/test".into(),
            transform: mcf_map::MapTransform::IDENTITY,
            props: Default::default(),
        });

        let uid = next_item_uid(&map);

        assert_eq!(uid, "ed000002");
        assert!(uid.chars().all(|character| character.is_ascii_hexdigit()));
    }

    #[test]
    fn un_calque_cree_par_lediteur_est_valide() {
        let transform = Transform::from_xyz(2.0, 0.5, 0.0)
            .with_rotation(Quat::from_rotation_y(0.5))
            .with_scale(Vec3::new(1.0, 2.0, 1.0));
        let mut map = mcf_map::McfMap::new("nouveau_calque", "MCF Editor", "2026-11-21T00:00:00", 1);
        map.items.push(mcf_map::MapItem {
            uid: "ed000001".into(),
            catalog: "batiments/entrepot".into(),
            transform: transform_to_map(&transform),
            props: Default::default(),
        });

        let text = map.to_json_pretty().expect("calque sérialisable");
        let loaded = mcf_map::load_str(&text, None).expect("calque relisible");

        assert_eq!(loaded.map, map);
    }

    #[test]
    fn le_chemin_de_sortie_exige_lextension_mcfmap() {
        assert!(is_mcfmap_path(Path::new("cartes/port.mcfmap")));
        assert!(!is_mcfmap_path(Path::new("cartes/port.json")));
        assert!(!is_mcfmap_path(Path::new("cartes/port")));
    }

    #[test]
    fn la_selection_multiple_garde_un_objet_principal() {
        let first = Entity::from_raw_u32(1).expect("entité de test");
        let second = Entity::from_raw_u32(2).expect("entité de test");
        let mut state = EditorState::default();
        select_entities(&mut state, Some(first), false);
        select_entities(&mut state, Some(second), true);
        assert_eq!(state.selected_entities, vec![first, second]);
        assert_eq!(state.selected, Some(second));
        select_entities(&mut state, Some(second), true);
        assert_eq!(state.selected_entities, vec![first]);
        assert_eq!(state.selected, Some(first));
    }

    #[test]
    fn un_glisser_cree_une_unique_operation_annulable() {
        let entity = Entity::from_raw_u32(1).expect("entité de test");
        let before = Transform::from_xyz(1.0, 0.0, 2.0);
        let after = Transform::from_xyz(5.0, 0.0, -3.0);
        let mut drag = DragState {
            entity: Some(entity),
            before: Some(before),
            after: Some(after),
            viewport: Some(EditorViewport::Map2d),
        };
        let mut history = TransformHistory::default();
        let mut document = MapDocument::default();

        assert!(commit_drag(&mut drag, &mut history, &mut document));
        assert!(document.dirty);
        assert_eq!(history.undo.len(), 1);
        assert_eq!(history.undo[0].entity, entity);
        assert_eq!(history.undo[0].before, before);
        assert_eq!(history.undo[0].after, after);
        assert!(drag.entity.is_none());
    }

    #[test]
    fn le_plan_2d_reprend_lemprise_du_cuboide_3d() {
        let transform = Transform::from_xyz(2.0, 0.0, -3.0).with_scale(Vec3::new(4.0, 1.0, 6.0));
        let footprint = plan_footprint(&transform, egui::pos2(100.0, 100.0), 10.0);

        assert_eq!(footprint[0], egui::pos2(100.0, 40.0));
        assert_eq!(footprint[2], egui::pos2(140.0, 100.0));
        assert!(point_in_plan_footprint(egui::pos2(120.0, 70.0), &footprint));
        assert!(!point_in_plan_footprint(egui::pos2(145.0, 70.0), &footprint));
    }

    #[test]
    fn les_classes_du_calque_sont_celles_de_mcf_geo() {
        let ids: Vec<&str> = ClasseRoute::TOUTES.iter().map(|c| c.id()).collect();
        assert_eq!(ids, mcf_map::ROAD_TEMPLATES);
        assert_eq!(road_class("avenue"), ClasseRoute::Avenue);
    }

    #[test]
    fn le_clic_de_route_est_mesure_depuis_son_ruban() {
        let start = egui::pos2(0.0, 0.0);
        let end = egui::pos2(100.0, 0.0);
        assert!(point_segment_distance(egui::pos2(30.0, 0.0), start, end) < 0.001);
        assert!((point_segment_distance(egui::pos2(30.0, 12.0), start, end) - 12.0).abs() < 0.001);
        assert!((point_segment_distance(egui::pos2(120.0, 0.0), start, end) - 20.0).abs() < 0.001);
    }

    #[test]
    fn la_selection_3d_mesure_la_largeur_du_ruban() {
        let start = Vec3::new(-10.0, 0.0, 0.0);
        let end = Vec3::new(10.0, 0.0, 0.0);
        assert!(point_road_distance(Vec2::new(2.0, 1.9), start, end) < 2.0);
        assert!(point_road_distance(Vec2::new(2.0, 2.1), start, end) > 2.0);
    }

    #[test]
    fn les_points_de_route_modifies_sont_prets_a_etre_enregistres() {
        let mut map = mcf_map::McfMap::new("test", "test", "2026-11-22", 1);
        map.roads.push(mcf_map::MapRoad {
            uid: "abcdef12".into(),
            template: "route".into(),
            points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        });
        let preview = [Vec3::new(-2.0, 0.12, 3.0), Vec3::new(4.0, 1.12, 5.0)];

        sync_road_points(&mut map, "abcdef12", &preview);

        assert_eq!(map.roads[0].points, vec![[-2.0, 0.0, 3.0], [4.0, 1.0, 5.0]]);
        assert!(map.to_json_pretty().is_ok());
    }
}
