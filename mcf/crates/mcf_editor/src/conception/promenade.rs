//! Promenade : un personnage du jeu marche sur la carte, pour juger l'échelle (rues, bâtiments, distances) au niveau
//! du sol. P (ou le bouton « 🚶 Se promener ») le pose là où regarde la caméra 3D ; ZQSD / WASD (touches physiques)
//! le font marcher par rapport à la caméra, Maj pour courir ; la caméra le suit (clic droit : tourner autour,
//! molette : distance). P ou Échap pour revenir à l'édition.
//!
//! Modèle : `assets/personnages/perso_homme.glb`, copié du jeu (non versionné : asset du jeu). Le jeu l'affiche à
//! l'échelle 0,153 (1,65 m) : même échelle ici, pour voir la carte comme en jeu.
//!
//! Collisions : les murs d'un bâtiment modélisé sont la **coupe horizontale de ses maillages de collision** `-col`
//! (ceux que Godot importe comme collisions dans le jeu) à [`HAUTEUR_COUPE_M`] au-dessus de son sol, portes et volets
//! exclus (ils s'ouvrent en jeu). On longe donc le vrai mur, pas le rectangle englobant du modèle (débords de toit,
//! auvents, perrons). Un bloc, ou un modèle sans maillage de collision, garde son empreinte. Le personnage est un
//! disque repoussé hors de chaque mur proche : il glisse le long d'un mur quel que soit son angle et contourne les
//! coins.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bevy::{
    camera::visibility::RenderLayers,
    gltf::{GltfMesh, GltfNode},
    mesh::VertexAttributeValues,
    prelude::*,
    world_serialization::WorldInstanceReady,
};
use mcf_carte::Id;
use mcf_geo::P2;

use super::{COUCHE_3D, Disposition, Edition, Projet, ZoneVues, camera::Orbite, outils::GizmosPlan};

/// Chemin du modèle, relatif au dossier `assets` de l'éditeur.
const MODELE: &str = "personnages/perso_homme.glb";
/// Échelle du modèle dans le jeu (`ShopKeeper3D.tscn`) : 10,8 unités → 1,65 m.
const ECHELLE: f32 = 0.153;
/// Vitesses (m/s) : marche et course légère.
const MARCHE: f32 = 1.4;
const COURSE: f32 = 3.6;
/// Clips du modèle : attente, marche, course.
const CLIPS: [&str; 3] = ["q_Idle_Loop", "q_Walk_Loop", "q_Jog_Fwd_Loop"];
/// Hauteur des yeux de la caméra au-dessus des pieds (m).
const REGARD: f32 = 1.4;

pub struct PromenadePlugin;

impl Plugin for PromenadePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Promenade>()
            .add_systems(Update, (basculer, couper_murs, marcher).chain());
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Allure {
    #[default]
    Arret,
    Marche,
    Course,
}

impl Allure {
    fn clip(self) -> usize {
        match self {
            Self::Arret => 0,
            Self::Marche => 1,
            Self::Course => 2,
        }
    }
}

/// État de la promenade.
#[derive(Resource, Default)]
pub struct Promenade {
    /// Modèle en cours de chargement ou chargé.
    gltf: Option<Handle<Gltf>>,
    /// Graphe d'animation et nœuds des trois clips.
    graphe: Option<(Handle<AnimationGraph>, Vec<AnimationNodeIndex>)>,
    /// Personnage posé.
    marcheur: Option<Entity>,
    /// Position au sol (m) et cap (rad, 0 = vers +Z).
    position: P2,
    cap: f32,
    allure: Allure,
    /// Était active à l'image précédente.
    active: bool,
    /// Caméra avant la promenade (distance, tangage), rendue à la sortie.
    camera_avant: Option<(f32, f32, f32)>,
    /// Physique des bâtiments coupée (touche C) : on traverse les murs.
    sans_collisions: bool,
    /// Empreintes des bâtiments indexées, refaites quand le bâti change.
    obstacles: Option<Obstacles>,
    /// Modèles des fiches chargés pour en couper les murs.
    gltf_murs: HashMap<String, Handle<Gltf>>,
    /// Murs de chaque fiche, dans le repère du modèle (`None` : pas de maillage de collision utilisable).
    murs: HashMap<String, Option<Arc<Vec<[P2; 2]>>>>,
}

/// Rayon du personnage (m).
const RAYON: f64 = 0.35;
/// Hauteur de la coupe des maillages de collision au-dessus du sol du modèle (m) : hanches du personnage, au-dessus
/// des marches et des bordures, sous les appuis de fenêtre.
pub const HAUTEUR_COUPE_M: f32 = 0.9;
/// Pas maximal d'un déplacement vérifié d'un coup (m) : moins que le rayon, on ne traverse pas un mur fin.
const PAS_MAX_M: f64 = 0.15;

/// Murs connus de chaque fiche (repère du modèle).
type MursFiches = HashMap<String, Option<Arc<Vec<[P2; 2]>>>>;

/// Murs des bâtiments pour les collisions du personnage : empreintes indexées, segments de murs calculés à la
/// demande pour les bâtiments approchés.
struct Obstacles {
    revision: u64,
    /// Fiches dont les murs étaient connus à la construction (refait quand d'autres arrivent).
    murs_connus: usize,
    index: mcf_carte::grille::Grille,
    ids: Vec<Id>,
    polys: Vec<Vec<P2>>,
    /// Segments de murs (m, repère de la carte) des bâtiments déjà approchés, et s'ils viennent d'un modèle.
    segments: HashMap<u32, (bool, Vec<[P2; 2]>)>,
}

impl Obstacles {
    fn de(carte: &mcf_carte::Carte, murs_connus: usize) -> Self {
        let (ids, polys): (Vec<Id>, Vec<Vec<P2>>) = carte
            .batiments
            .iter()
            .map(|(i, b)| (*i, b.empreinte.clone()))
            .unzip();
        Self {
            revision: carte.revisions.batiments,
            murs_connus,
            index: mcf_carte::grille::Grille::de_polygones(&polys, 32.0),
            ids,
            polys,
            segments: HashMap::new(),
        }
    }

    /// Murs du bâtiment `i` : coupe de son modèle si elle existe, sinon les côtés de son empreinte.
    fn murs(
        &mut self,
        i: u32,
        carte: &mcf_carte::Carte,
        catalogue: &super::modeles::Catalogue,
        murs: &MursFiches,
    ) -> &(bool, Vec<[P2; 2]>) {
        let (ids, polys) = (&self.ids, &self.polys);
        self.segments.entry(i).or_insert_with(|| {
            let id = ids[i as usize];
            let modele = carte.batiments.get(&id).and_then(|b| {
                let m = b.modele.as_ref()?;
                let fiche = catalogue.fiche(&m.fiche)?;
                let coupe = murs.get(&m.fiche)?.as_ref()?;
                let t = super::modeles::placement(fiche, b, 0.0)?;
                let monde = |p: P2| {
                    let q = t.transform_point(Vec3::new(p[0] as f32, HAUTEUR_COUPE_M, p[1] as f32));
                    [f64::from(q.x), f64::from(q.z)]
                };
                Some(
                    coupe
                        .iter()
                        .map(|[a, b]| [monde(*a), monde(*b)])
                        .collect::<Vec<_>>(),
                )
            });
            match modele {
                Some(s) => (true, s),
                None => {
                    let poly = &polys[i as usize];
                    let n = poly.len();
                    (false, (0..n).map(|k| [poly[k], poly[(k + 1) % n]]).collect())
                }
            }
        })
    }

    /// Déplace le disque du personnage de `depart` vers `voulu` sans traverser de mur : à chaque petit pas, il est
    /// repoussé hors des murs qu'il touche (il glisse le long, contourne les coins). Un bloc dans lequel on se trouve
    /// au départ est ignoré (on peut en sortir).
    fn deplacer(
        &mut self,
        depart: P2,
        voulu: P2,
        carte: &mcf_carte::Carte,
        catalogue: &super::modeles::Catalogue,
        murs: &MursFiches,
    ) -> P2 {
        let total = [voulu[0] - depart[0], voulu[1] - depart[1]];
        let pas = (total[0].hypot(total[1]) / PAS_MAX_M).ceil().max(1.0) as usize;
        let marge = RAYON + PAS_MAX_M + 0.05;
        let b = [
            [depart[0].min(voulu[0]) - marge, depart[1].min(voulu[1]) - marge],
            [depart[0].max(voulu[0]) + marge, depart[1].max(voulu[1]) + marge],
        ];
        let proches = self.index.proches(&b);
        let mut actifs = Vec::with_capacity(proches.len());
        for &i in &proches {
            let (modele, _) = *self.murs(i, carte, catalogue, murs);
            if modele || !mcf_geo::polygone::contient(&self.polys[i as usize], depart) {
                actifs.push(i);
            }
        }
        let mut p = depart;
        for _ in 0..pas {
            p = [p[0] + total[0] / pas as f64, p[1] + total[1] / pas as f64];
            // Quelques passes : repoussé par un mur, on peut en toucher un autre (coin rentrant).
            for _ in 0..4 {
                let mut bouge = false;
                for &i in &actifs {
                    let (_, segs) = self.murs(i, carte, catalogue, murs);
                    for [a, s] in segs {
                        let q = point_proche(p, *a, *s);
                        let (dx, dz) = (p[0] - q[0], p[1] - q[1]);
                        let d = dx.hypot(dz);
                        if d >= RAYON {
                            continue;
                        }
                        // Pile sur le mur : on recule vers le départ.
                        let (nx, nz) = if d > 1e-9 {
                            (dx / d, dz / d)
                        } else {
                            let (rx, rz) = (depart[0] - p[0], depart[1] - p[1]);
                            let r = rx.hypot(rz).max(1e-9);
                            (rx / r, rz / r)
                        };
                        p = [p[0] + nx * (RAYON - d), p[1] + nz * (RAYON - d)];
                        bouge = true;
                    }
                }
                if !bouge {
                    break;
                }
            }
        }
        p
    }
}

/// Point du segment `[a, b]` le plus proche de `p`.
fn point_proche(p: P2, a: P2, b: P2) -> P2 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l2 = d[0] * d[0] + d[1] * d[1];
    if l2 <= f64::EPSILON {
        return a;
    }
    let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
    [a[0] + d[0] * t, a[1] + d[1] * t]
}

/// Pièce qui s'ouvre en jeu (porte, volet roulant) : pas un mur pour la promenade.
fn ouvrant(nom: &str) -> bool {
    let n = nom.to_ascii_lowercase();
    ["door", "shutter", "porte", "volet"]
        .iter()
        .any(|m| n.contains(m))
}

/// Coupe horizontale d'un triangle à `hauteur` : segment `(x, z)` s'il la traverse.
fn coupe_triangle(tri: [Vec3; 3], hauteur: f32) -> Option<[P2; 2]> {
    let mut pts: Vec<P2> = Vec::with_capacity(2);
    for k in 0..3 {
        let (a, b) = (tri[k], tri[(k + 1) % 3]);
        if (a.y < hauteur) != (b.y < hauteur) {
            let q = a + (b - a) * ((hauteur - a.y) / (b.y - a.y));
            pts.push([f64::from(q.x), f64::from(q.z)]);
        }
    }
    (pts.len() == 2 && mcf_geo::distance(pts[0], pts[1]) > 1e-4).then(|| [pts[0], pts[1]])
}

/// Coupe horizontale à `hauteur` (repère du modèle) des maillages de collision `-col` / `-colonly` d'un modèle chargé,
/// ouvrants exclus : segments `(x, z)`. `None` si un maillage n'est pas (encore) disponible ; liste vide si le modèle
/// n'a pas de collision utilisable.
fn couper(
    gltf: &Gltf,
    noeuds: &Assets<GltfNode>,
    gmeshes: &Assets<GltfMesh>,
    meshes: &Assets<Mesh>,
    hauteur: f32,
) -> Option<Vec<[P2; 2]>> {
    let mut enfants = std::collections::HashSet::new();
    for h in &gltf.nodes {
        enfants.extend(noeuds.get(h)?.children.iter().map(|c| c.id()));
    }
    let mut pile: Vec<(Handle<GltfNode>, bevy::math::Affine3A)> = gltf
        .nodes
        .iter()
        .filter(|h| !enfants.contains(&h.id()))
        .map(|h| (h.clone(), bevy::math::Affine3A::IDENTITY))
        .collect();
    let mut segs = Vec::new();
    while let Some((h, parent)) = pile.pop() {
        let n = noeuds.get(&h)?;
        let m = parent * n.transform.compute_affine();
        if n.name.contains("-col")
            && !ouvrant(&n.name)
            && let Some(gm) = &n.mesh
        {
            for prim in &gmeshes.get(gm)?.primitives {
                let mesh = meshes.get(&prim.mesh)?;
                let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    continue;
                };
                let pos: Vec<Vec3> = pos.iter().map(|p| m.transform_point3(Vec3::from(*p))).collect();
                let idx: Vec<usize> = match mesh.indices() {
                    Some(i) => i.iter().collect(),
                    None => (0..pos.len()).collect(),
                };
                for t in idx.as_chunks::<3>().0 {
                    if let (Some(a), Some(b), Some(c)) = (pos.get(t[0]), pos.get(t[1]), pos.get(t[2]))
                        && let Some(s) = coupe_triangle([*a, *b, *c], hauteur)
                    {
                        segs.push(s);
                    }
                }
            }
        }
        pile.extend(n.children.iter().map(|c| (c.clone(), m)));
    }
    Some(segs)
}

/// Pendant la promenade : charge les modèles des fiches et en coupe les murs, une fois par fiche.
#[allow(clippy::too_many_arguments)]
fn couper_murs(
    assets: Res<AssetServer>,
    catalogue: Res<super::modeles::Catalogue>,
    gltfs: Res<Assets<Gltf>>,
    noeuds: Res<Assets<GltfNode>>,
    gmeshes: Res<Assets<GltfMesh>>,
    meshes: Res<Assets<Mesh>>,
    mut promenade: ResMut<Promenade>,
) {
    if !promenade.active || promenade.sans_collisions {
        return;
    }
    let p = &mut *promenade;
    for f in &catalogue.fiches {
        if p.murs.contains_key(&f.id) || !catalogue.disponibles.contains(&f.id) {
            continue;
        }
        let h = p
            .gltf_murs
            .entry(f.id.clone())
            .or_insert_with(|| assets.load(format!("{}/{}", super::modeles::DOSSIER, f.fichier)));
        if assets.load_state(h.id()).is_failed() {
            warn!(
                "promenade : modèle de {} illisible, son empreinte sert de mur",
                f.id
            );
            p.murs.insert(f.id.clone(), None);
            continue;
        }
        if !assets.is_loaded_with_dependencies(h.id()) {
            continue;
        }
        let Some(g) = gltfs.get(h.id()) else { continue };
        if let Some(segs) = couper(g, &noeuds, &gmeshes, &meshes, HAUTEUR_COUPE_M) {
            debug!("promenade : {} segment(s) de mur pour {}", segs.len(), f.id);
            if segs.is_empty() {
                warn!(
                    "promenade : {} n'a pas de maillage de collision, son empreinte sert de mur",
                    f.id
                );
            }
            p.murs
                .insert(f.id.clone(), (!segs.is_empty()).then(|| Arc::new(segs)));
            p.gltf_murs.remove(&f.id);
        }
    }
}

/// Racine du personnage.
#[derive(Component)]
struct Marcheur;

fn chemin_modele() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(MODELE)
}

#[allow(clippy::too_many_arguments)]
fn basculer(
    mut commands: Commands,
    touches: Res<ButtonInput<KeyCode>>,
    zone: Res<ZoneVues>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    mut graphes: ResMut<Assets<AnimationGraph>>,
    mut promenade: ResMut<Promenade>,
    mut edition: ResMut<Edition>,
    mut projet: ResMut<Projet>,
    mut orbite: ResMut<Orbite>,
) {
    if !zone.clavier_ui
        && (touches.just_pressed(KeyCode::KeyP)
            || (edition.promenade && touches.just_pressed(KeyCode::Escape)))
    {
        edition.promenade = !edition.promenade;
    }
    // Entrée en promenade.
    if edition.promenade && !promenade.active {
        if !chemin_modele().is_file() {
            edition.promenade = false;
            projet.echec(format!(
                "Promenade : modèle introuvable ({}). Copiez perso_homme.glb du jeu à cet endroit.",
                chemin_modele().display()
            ));
            return;
        }
        promenade.active = true;
        promenade.gltf.get_or_insert_with(|| assets.load(MODELE));
        promenade.position = [f64::from(orbite.cible.x), f64::from(orbite.cible.z)];
        promenade.camera_avant = Some((orbite.distance, orbite.tangage, orbite.elevation));
        orbite.distance = 5.0;
        orbite.tangage = 0.3;
        orbite.elevation = REGARD;
        edition.disposition = Disposition::Perspective;
        projet.info(
            "Promenade : ZQSD pour marcher, Maj pour courir, clic droit pour tourner la caméra, P ou Échap pour sortir.",
        );
    }
    // Sortie.
    if !edition.promenade && promenade.active {
        promenade.active = false;
        if let Some(e) = promenade.marcheur.take() {
            commands.entity(e).despawn();
        }
        if let Some((d, t, e)) = promenade.camera_avant.take() {
            orbite.distance = d;
            orbite.tangage = t;
            orbite.elevation = e;
        }
        projet.info("Promenade terminée.");
        return;
    }
    // Personnage à poser dès que le modèle est chargé.
    if promenade.active
        && promenade.marcheur.is_none()
        && let Some(h) = &promenade.gltf
        && assets.is_loaded_with_dependencies(h)
        && let Some(gltf) = gltfs.get(h)
    {
        let Some(scene) = gltf.default_scene.clone() else {
            projet.echec("Promenade : le modèle n'a pas de scène.");
            edition.promenade = false;
            return;
        };
        if promenade.graphe.is_none() {
            let clips: Option<Vec<_>> = CLIPS
                .iter()
                .map(|n| gltf.named_animations.get(*n).cloned())
                .collect();
            let Some(clips) = clips else {
                projet.echec(format!("Promenade : clips {CLIPS:?} absents du modèle."));
                edition.promenade = false;
                return;
            };
            let (graphe, noeuds) = AnimationGraph::from_clips(clips);
            promenade.graphe = Some((graphes.add(graphe), noeuds));
        }
        let modele = commands
            .spawn((
                WorldAssetRoot(scene),
                Transform::from_scale(Vec3::splat(ECHELLE)),
                RenderLayers::layer(COUCHE_3D),
            ))
            .observe(preparer_animations)
            .id();
        let e = commands
            .spawn((
                Marcheur,
                Transform::default(),
                Visibility::default(),
                RenderLayers::layer(COUCHE_3D),
            ))
            .add_child(modele)
            .id();
        promenade.marcheur = Some(e);
        promenade.allure = Allure::Arret;
    }
}

/// Scène du modèle instanciée : couche 3D sur toute la hiérarchie (sinon la caméra 3D ne la voit pas), graphe
/// d'animation branché sur le lecteur, attente en boucle.
fn preparer_animations(
    pret: On<WorldInstanceReady>,
    mut commands: Commands,
    promenade: Res<Promenade>,
    enfants: Query<&Children>,
    mut lecteurs: Query<&mut AnimationPlayer>,
) {
    let racine = pret.event_target();
    let Some((graphe, noeuds)) = &promenade.graphe else {
        return;
    };
    {
        for e in enfants.iter_descendants(racine) {
            commands.entity(e).insert(RenderLayers::layer(COUCHE_3D));
            if let Ok(mut lecteur) = lecteurs.get_mut(e) {
                let mut transitions = AnimationTransitions::new();
                transitions.play(&mut lecteur, noeuds[0], Duration::ZERO).repeat();
                commands
                    .entity(e)
                    .insert((AnimationGraphHandle(graphe.clone()), transitions));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn marcher(
    catalogue: Res<super::modeles::Catalogue>,
    touches: Res<ButtonInput<KeyCode>>,
    temps: Res<Time>,
    zone: Res<ZoneVues>,
    mut promenade: ResMut<Promenade>,
    mut projet: ResMut<Projet>,
    mut orbite: ResMut<Orbite>,
    mut racines: Query<&mut Transform, With<Marcheur>>,
    enfants: Query<&Children>,
    mut lecteurs: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
    mut plan: Gizmos<GizmosPlan>,
) {
    if !promenade.active {
        return;
    }
    // Direction voulue, relative à la caméra.
    let (s, c) = bevy::math::ops::sin_cos(orbite.lacet);
    let avant = Vec2::new(-s, -c);
    let droite = Vec2::new(c, -s);
    let mut d = Vec2::ZERO;
    if !zone.clavier_ui {
        for (k, v) in [
            (KeyCode::KeyW, avant),
            (KeyCode::KeyS, -avant),
            (KeyCode::KeyA, -droite),
            (KeyCode::KeyD, droite),
        ] {
            if touches.pressed(k) {
                d += v;
            }
        }
    }
    if !zone.clavier_ui && touches.just_pressed(KeyCode::KeyC) {
        promenade.sans_collisions = !promenade.sans_collisions;
        projet.info(if promenade.sans_collisions {
            "Promenade : physique des bâtiments coupée (C pour la remettre)."
        } else {
            "Promenade : physique des bâtiments active, on ne traverse plus les murs (C pour la couper)."
        });
    }
    let court = touches.pressed(KeyCode::ShiftLeft) || touches.pressed(KeyCode::ShiftRight);
    let allure = match (d.length_squared() > 0.0, court) {
        (false, _) => Allure::Arret,
        (true, false) => Allure::Marche,
        (true, true) => Allure::Course,
    };
    if allure != Allure::Arret {
        let d = d.normalize();
        let v = if allure == Allure::Course { COURSE } else { MARCHE };
        let dt = temps.delta_secs().min(0.1);
        let suivant = [
            promenade.position[0] + f64::from(d.x * v * dt),
            promenade.position[1] + f64::from(d.y * v * dt),
        ];
        // Physique des bâtiments : on ne traverse pas un mur, on glisse le long.
        let depart = promenade.position;
        let connus = promenade.murs.len();
        let arrivee = if promenade.sans_collisions {
            suivant
        } else {
            let p = &mut *promenade;
            if p.obstacles
                .as_ref()
                .is_none_or(|o| o.revision != projet.carte.revisions.batiments || o.murs_connus != connus)
            {
                p.obstacles = Some(Obstacles::de(&projet.carte, connus));
            }
            match p.obstacles.as_mut() {
                Some(o) => o.deplacer(depart, suivant, &projet.carte, &catalogue, &p.murs),
                None => suivant,
            }
        };
        if projet.carte.manifeste.contient(arrivee) {
            promenade.position = arrivee;
        }
        // Le modèle regarde vers +Z (glTF) : cap = angle de +Z vers la direction de marche.
        let voulu = bevy::math::ops::atan2(d.x, d.y);
        let ecart = (voulu - promenade.cap + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        promenade.cap += ecart * (dt * 12.0).min(1.0);
    }
    let p = promenade.position;
    let h = projet.carte.derive().terrain.hauteur(p) as f32;
    let pied = Vec3::new(p[0] as f32, h, p[1] as f32);
    // Caméra : la cible suit le personnage (la caméra 3D remet sa hauteur au sol, `elevation` = hauteur du regard).
    orbite.cible = pied;
    orbite.elevation = REGARD;
    plan.circle(
        Isometry3d::new(
            pied.with_y(projet.carte.manifeste.altitude_max_m as f32 + 40.0),
            Quat::from_rotation_arc(Vec3::Z, Vec3::Y),
        ),
        6.0,
        Color::srgb(1.0, 0.3, 0.1),
    );
    let Some(racine) = promenade.marcheur else {
        return;
    };
    if let Ok(mut t) = racines.get_mut(racine) {
        t.translation = pied;
        t.rotation = Quat::from_rotation_y(promenade.cap);
    }
    // Clip selon l'allure (fondu 0,2 s).
    if allure != promenade.allure
        && let Some((_, noeuds)) = &promenade.graphe
    {
        for e in enfants.iter_descendants(racine) {
            if let Ok((mut lecteur, mut transitions)) = lecteurs.get_mut(e) {
                transitions
                    .play(&mut lecteur, noeuds[allure.clip()], Duration::from_millis(200))
                    .repeat();
            }
        }
        promenade.allure = allure;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcf_carte::{Batiment, Carte, Famille, Manifeste, Origine, batiments::empreinte_rectangle};

    /// Carte avec un bloc de 10 × 10 m centré en (0, 0), tourné de `cap` degrés.
    fn carte(cap: f64) -> Carte {
        let mut c = Carte::nouvelle(Manifeste::nouveau("t", 512.0)).expect("carte");
        c.modifier("bloc", |tx| {
            tx.poser_batiment(
                Id::nouveau(),
                Batiment {
                    empreinte: empreinte_rectangle([0.0, 0.0], 10.0, 10.0, cap),
                    niveaux: 2,
                    hauteur_niveau_m: 3.0,
                    famille: Famille::Residentiel,
                    origine: Origine::Manuel,
                    verrou: false,
                    affectation: None,
                    modele: None,
                    decalage_y_m: 0.0,
                },
            );
            Ok(())
        })
        .expect("pose");
        c
    }

    /// Marche `n` pas de `pas` m dans la direction `d`, comme `marcher`.
    fn marche(c: &Carte, depart: P2, d: P2, pas: f64, n: usize) -> P2 {
        let cat = super::super::modeles::Catalogue::default();
        let murs = MursFiches::new();
        let mut o = Obstacles::de(c, 0);
        let mut p = depart;
        for _ in 0..n {
            p = o.deplacer(p, [p[0] + d[0] * pas, p[1] + d[1] * pas], c, &cat, &murs);
        }
        p
    }

    #[test]
    fn on_contourne_un_coin_en_longeant_le_mur() {
        let c = carte(0.0);
        // Le long du mur nord (z = -5), à 40 cm, en visant un peu vers le mur : on glisse et on dépasse le coin.
        let d: P2 = [1.0, 0.08];
        let l = d[0].hypot(d[1]);
        let p = marche(&c, [-8.0, -5.4], [d[0] / l, d[1] / l], 0.06, 300);
        assert!(p[0] > 6.0, "bloqué au coin : {p:?}");
        // Jamais dans le mur.
        assert!(p[1] < -5.0 + 1e-6 || p[0] > 5.0 + RAYON - 1e-6, "{p:?}");
    }

    #[test]
    fn on_glisse_le_long_d_un_mur_en_biais() {
        let c = carte(30.0);
        // Droit sur le centre du bloc tourné : on s'arrête contre le mur, à son rayon, sans le traverser.
        let p = marche(&c, [-12.0, 0.0], [1.0, 0.0], 0.06, 300);
        let poly = &c.batiments.values().next().expect("bloc").empreinte;
        assert!(!mcf_geo::polygone::contient(poly, p), "dans le mur : {p:?}");
        let dmin = (0..4)
            .map(|k| mcf_geo::distance_segment(p, poly[k], poly[(k + 1) % 4]))
            .fold(f64::MAX, f64::min);
        assert!(dmin > RAYON - 0.02, "trop près du mur : {dmin}");
        // Le mur est en biais : on n'est pas resté planté, on a glissé de côté.
        assert!(p[1].abs() > 1.0, "pas de glissade : {p:?}");
    }

    #[test]
    fn on_sort_d_un_bloc_dans_lequel_on_est_pose() {
        let c = carte(0.0);
        let p = marche(&c, [0.0, 0.0], [1.0, 0.0], 0.06, 200);
        assert!(p[0] > 6.0, "{p:?}");
    }

    #[test]
    fn coupe_d_un_triangle_vertical() {
        let tri = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 2.0, 0.0),
        ];
        let s = coupe_triangle(tri, 1.0).expect("coupé");
        let xs = [s[0][0].min(s[1][0]), s[0][0].max(s[1][0])];
        assert!((xs[0] - 0.0).abs() < 1e-6 && (xs[1] - 1.0).abs() < 1e-6, "{s:?}");
        assert!(coupe_triangle(tri, 3.0).is_none());
        assert!(ouvrant("House_A_Door_01-col") && ouvrant("Superette_Shutter_02-col"));
        assert!(!ouvrant("House_A_Shell-col"));
    }
}
