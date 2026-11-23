//! Modèles du banc, générés en code (cubes, prismes, cylindres, cônes, sphères).
//!
//! Règles de performance :
//! - **un modèle = un seul maillage** (les pièces sont fusionnées) : une seule entité par élément posé, aucun
//!   enfant, aucune propagation de transformation ;
//! - **couleurs dans les sommets** (`ATTRIBUTE_COLOR`) : tous les modèles partagent quelques matériaux, ce qui
//!   permet à Bevy de regrouper les dessins (même matériau ⇒ même lot, instancing par maillage) ;
//! - chaque modèle est construit **une fois** ; les éléments posés ne font que cloner le `Handle`.
//!
//! Les formes et dimensions sont tirées d'un hachage de l'emplacement du modèle : mêmes maillages à chaque
//! exécution, sur toutes les machines. Origine de chaque modèle : au sol (y = 0), avant vers −Z.
//!
//! Lecture honnête : ce sont des volumes simples (quelques centaines à ~2 000 triangles), en dessous des budgets
//! mid poly de `rendu-lod-objets.md` (bâtiment 10 000 – 50 000, véhicule 30 000 – 60 000…). Ils servent à mesurer
//! le **nombre** d'objets et la tenue de la carte, pas encore la densité de triangles d'un vrai modèle.

use bevy::color::{ColorToComponents, LinearRgba, Srgba};
use bevy::math::{Quat, Vec3};
use bevy::mesh::{Mesh, MeshBuilder, Meshable, VertexAttributeValues};
use bevy::shape::{Cone, Cuboid, Cylinder, Plane3d, Sphere};
use bevy::transform::components::Transform;
use mcf_big_map::Kind;

use crate::catalog::ModelRef;

/// Position de la tête d'une lampe (lumière et ampoule émissive), dans le repère du modèle.
pub const LAMP_HEAD: Vec3 = Vec3::new(0.0, 4.85, 0.95);

const GLASS: Srgba = Srgba::rgb(0.10, 0.13, 0.18);
const ROOF: Srgba = Srgba::rgb(0.24, 0.24, 0.26);
const DOOR: Srgba = Srgba::rgb(0.32, 0.21, 0.13);
const METAL: Srgba = Srgba::rgb(0.30, 0.31, 0.33);
const TIRE: Srgba = Srgba::rgb(0.05, 0.05, 0.05);
const BARK: Srgba = Srgba::rgb(0.33, 0.24, 0.16);
const SKIN: Srgba = Srgba::rgb(0.80, 0.62, 0.50);

const FACADES: [Srgba; 6] = [
    Srgba::rgb(0.80, 0.74, 0.62),
    Srgba::rgb(0.62, 0.33, 0.26),
    Srgba::rgb(0.55, 0.56, 0.58),
    Srgba::rgb(0.88, 0.87, 0.84),
    Srgba::rgb(0.78, 0.62, 0.38),
    Srgba::rgb(0.45, 0.52, 0.58),
];
const PROPS: [Srgba; 6] = [
    Srgba::rgb(0.55, 0.40, 0.24),
    Srgba::rgb(0.20, 0.35, 0.22),
    Srgba::rgb(0.65, 0.12, 0.10),
    Srgba::rgb(0.25, 0.30, 0.45),
    Srgba::rgb(0.70, 0.65, 0.20),
    Srgba::rgb(0.45, 0.45, 0.45),
];
const LEAVES: [Srgba; 4] = [
    Srgba::rgb(0.13, 0.30, 0.12),
    Srgba::rgb(0.20, 0.38, 0.14),
    Srgba::rgb(0.10, 0.24, 0.15),
    Srgba::rgb(0.28, 0.40, 0.16),
];
const CLOTHES: [Srgba; 5] = [
    Srgba::rgb(0.15, 0.20, 0.35),
    Srgba::rgb(0.55, 0.15, 0.15),
    Srgba::rgb(0.20, 0.20, 0.20),
    Srgba::rgb(0.75, 0.70, 0.60),
    Srgba::rgb(0.20, 0.40, 0.30),
];

/// Hachage SplitMix64 → [0, 1) : paramètres d'un modèle, stables d'une machine à l'autre.
fn unit(slot: usize, salt: u64) -> f32 {
    let mut z = (slot as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(salt.wrapping_mul(0xD1B5_4A32_D192_ED03));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

fn pick<T: Copy>(list: &[T], u: f32) -> T {
    list[((u * list.len() as f32) as usize).min(list.len() - 1)]
}

fn cuboid(x: f32, y: f32, z: f32) -> Mesh {
    Cuboid::new(x, y, z).mesh().build()
}

fn cylinder(r: f32, h: f32, res: u32) -> Mesh {
    Cylinder::new(r, h).mesh().resolution(res).build()
}

fn cone(r: f32, h: f32, res: u32) -> Mesh {
    Cone { radius: r, height: h }.mesh().resolution(res).build()
}

fn sphere(r: f32, sectors: u32, stacks: u32) -> Mesh {
    Sphere::new(r).mesh().uv(sectors, stacks)
}

fn at(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_xyz(x, y, z)
}

/// Retourne un maillage (faces visibles de l'intérieur) : ordre des sommets inversé, normales opposées.
fn inverted(mut m: Mesh) -> Mesh {
    // Les primitives de Bevy sont des listes de triangles indexées : l'inversion ne peut pas échouer.
    m.invert_winding().expect("liste de triangles indexée");
    if let Some(VertexAttributeValues::Float32x3(normals)) = m.attribute_mut(Mesh::ATTRIBUTE_NORMAL) {
        for n in normals {
            *n = [-n[0], -n[1], -n[2]];
        }
    }
    m
}

/// Assemble des pièces colorées en **un** maillage.
struct Parts(Option<Mesh>);

impl Parts {
    fn new() -> Self {
        Self(None)
    }

    fn add(&mut self, mut part: Mesh, place: Transform, color: Srgba) {
        let c = LinearRgba::from(color).to_f32_array();
        part.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; part.count_vertices()]);
        let part = part.transformed_by(place);
        match &mut self.0 {
            None => self.0 = Some(part),
            // Toutes les pièces viennent des primitives de Bevy + une couleur : mêmes attributs, même
            // topologie. Une erreur ici est un défaut de ce fichier (couvert par les tests), pas des données.
            Some(acc) => acc.merge(&part).expect("pièces aux attributs identiques"),
        }
    }

    fn finish(self) -> Mesh {
        self.0.unwrap_or_else(|| cuboid(0.1, 0.1, 0.1))
    }
}

/// Construit le maillage d'un modèle (même résultat à chaque appel).
pub fn build_model(m: ModelRef) -> Mesh {
    let slot = m.slot();
    let i = usize::from(m.index);
    match m.kind {
        Kind::Building => building(slot, false),
        Kind::Interior => building(slot, true),
        Kind::Object => object(slot, i),
        Kind::Tree => tree(slot, i),
        Kind::Lamp => lamp(METAL),
        Kind::ShadowLamp => lamp(Srgba::rgb(0.16, 0.26, 0.20)),
        Kind::Character => character(slot),
        Kind::Vehicle => vehicle(i),
    }
}

/// Bâtiment sur une parcelle de 20 m : volume, acrotère, local technique, fenêtres par étage, porte côté −Z.
/// Avec intérieur : coque intérieure du rez-de-chaussée (faces tournées vers l'intérieur) et un comptoir.
fn building(slot: usize, interior: bool) -> Mesh {
    let r = |s| unit(slot, s);
    let w = 12.0 + 6.0 * r(1);
    let d = 12.0 + 6.0 * r(2);
    let floors = 2 + ((r(3) * 7.0) as usize).min(6);
    let fh = 3.2;
    let h = floors as f32 * fh;
    let mut p = Parts::new();
    p.add(cuboid(w, h, d), at(0.0, h / 2.0, 0.0), pick(&FACADES, r(4)));
    p.add(cuboid(w + 0.4, 0.5, d + 0.4), at(0.0, h + 0.25, 0.0), ROOF);
    p.add(
        cuboid(w * 0.3, 1.6, d * 0.25),
        at(w * (r(5) - 0.5) * 0.4, h + 1.3, d * (r(6) - 0.5) * 0.4),
        METAL,
    );
    let (win_w, win_h, spacing, depth) = (1.4, 1.5, 3.0, 0.12);
    let row = |len: f32| {
        let n = (((len - 2.0) / spacing) as usize).max(1);
        (0..n).map(move |k| (k as f32 - (n - 1) as f32 / 2.0) * spacing)
    };
    let door_w = if interior { 3.0 } else { 1.8 };
    for f in 0..floors {
        let y = f as f32 * fh + 1.7;
        for x in row(w) {
            for side in [-1.0, 1.0] {
                // Rez-de-chaussée côté −Z : la porte remplace les fenêtres centrales.
                if f == 0 && side < 0.0 && x.abs() < door_w / 2.0 + 1.0 {
                    continue;
                }
                let z = side * (d / 2.0 + 0.03);
                p.add(cuboid(win_w, win_h, depth), at(x, y, z), GLASS);
            }
        }
        for z in row(d) {
            for side in [-1.0, 1.0] {
                let x = side * (w / 2.0 + 0.03);
                p.add(cuboid(depth, win_h, win_w), at(x, y, z), GLASS);
            }
        }
    }
    p.add(cuboid(door_w, 2.6, 0.15), at(0.0, 1.3, -(d / 2.0 + 0.05)), DOOR);
    if interior {
        p.add(
            inverted(cuboid(w - 0.6, fh, d - 0.6)),
            at(0.0, fh / 2.0, 0.0),
            Srgba::rgb(0.86, 0.80, 0.70),
        );
        p.add(cuboid(3.0, 1.0, 0.8), at(0.0, 0.5, d / 4.0), DOOR);
    }
    p.finish()
}

/// Mobilier et props : six formes (caisse, tonneau, banc, poubelle, borne, table), taille et couleur variables.
fn object(slot: usize, i: usize) -> Mesh {
    let r = |s| unit(slot, s);
    let s = 0.8 + 0.5 * r(1);
    let color = pick(&PROPS, r(2));
    let mut p = Parts::new();
    match i % 6 {
        0 => {
            let c = (0.6 + 0.6 * r(3)) * s;
            p.add(cuboid(c, c, c), at(0.0, c / 2.0, 0.0), color);
            p.add(cuboid(c * 1.04, 0.05, c * 1.04), at(0.0, c, 0.0), METAL);
        }
        1 => {
            p.add(cylinder(0.3 * s, 0.9 * s, 16), at(0.0, 0.45 * s, 0.0), color);
            for y in [0.2, 0.7] {
                p.add(cylinder(0.31 * s, 0.04, 16), at(0.0, y * s, 0.0), METAL);
            }
        }
        2 => {
            p.add(cuboid(1.6 * s, 0.08, 0.45), at(0.0, 0.45, 0.0), color);
            p.add(cuboid(1.6 * s, 0.4, 0.06), at(0.0, 0.75, 0.2), color);
            for x in [-0.7 * s, 0.7 * s] {
                p.add(cuboid(0.08, 0.45, 0.4), at(x, 0.225, 0.0), METAL);
            }
        }
        3 => {
            p.add(cylinder(0.28 * s, 0.9 * s, 12), at(0.0, 0.45 * s, 0.0), color);
            p.add(cylinder(0.31 * s, 0.06, 12), at(0.0, 0.93 * s, 0.0), METAL);
        }
        4 => {
            p.add(cuboid(0.4 * s, 0.1, 0.4 * s), at(0.0, 0.05, 0.0), METAL);
            p.add(cone(0.2 * s, 0.7 * s, 12), at(0.0, 0.1 + 0.35 * s, 0.0), color);
        }
        _ => {
            p.add(cylinder(0.5 * s, 0.05, 16), at(0.0, 0.75, 0.0), color);
            p.add(cylinder(0.05, 0.72, 8), at(0.0, 0.37, 0.0), METAL);
            p.add(cylinder(0.25, 0.03, 12), at(0.0, 0.015, 0.0), METAL);
        }
    }
    p.finish()
}

/// Arbres : conifères (tronc + 3 cônes) pour les numéros pairs, feuillus (tronc + 2 sphères) pour les impairs.
fn tree(slot: usize, i: usize) -> Mesh {
    let r = |s| unit(slot, s);
    let leaves = pick(&LEAVES, r(2));
    let mut p = Parts::new();
    if i.is_multiple_of(2) {
        let h = 7.0 + 5.0 * r(1);
        p.add(cylinder(0.22, h * 0.35, 8), at(0.0, h * 0.175, 0.0), BARK);
        for j in 0..3 {
            let radius = (2.4 - j as f32 * 0.6) * h / 10.0;
            let ch = h * 0.4;
            p.add(
                cone(radius, ch, 12),
                at(0.0, h * 0.3 + j as f32 * h * 0.2 + ch / 2.0, 0.0),
                leaves,
            );
        }
    } else {
        let h = 6.0 + 4.0 * r(1);
        p.add(cylinder(0.25, h * 0.5, 8), at(0.0, h * 0.25, 0.0), BARK);
        p.add(sphere(h * 0.3, 14, 10), at(0.0, h * 0.65, 0.0), leaves);
        p.add(sphere(h * 0.2, 12, 8), at(h * 0.15, h * 0.55, h * 0.1), leaves);
    }
    p.finish()
}

/// Lampadaire : socle, mât, potence vers +Z. L'ampoule est une entité à part ([`lamp_head_mesh`]), émissive.
fn lamp(pole: Srgba) -> Mesh {
    let mut p = Parts::new();
    p.add(cylinder(0.15, 0.4, 10), at(0.0, 0.2, 0.0), pole);
    p.add(cylinder(0.07, 5.0, 10), at(0.0, 2.5, 0.0), pole);
    p.add(cuboid(0.07, 0.07, 1.0), at(0.0, 4.95, 0.5), pole);
    p.finish()
}

/// Ampoule d'un lampadaire, centrée sur [`LAMP_HEAD`] une fois placée.
pub fn lamp_head_mesh() -> Mesh {
    let mut p = Parts::new();
    p.add(cuboid(0.35, 0.12, 0.25), Transform::IDENTITY, Srgba::WHITE);
    p.finish()
}

/// Personnage statique (pose de référence) : jambes, torse, bras, tête. L'animation viendra avec les vrais
/// modèles.
fn character(slot: usize) -> Mesh {
    let r = |s| unit(slot, s);
    let k = 0.9 + 0.2 * r(1);
    let shirt = pick(&CLOTHES, r(2));
    let pants = pick(&CLOTHES, r(3));
    let mut p = Parts::new();
    for x in [-0.1, 0.1] {
        p.add(cylinder(0.09, 0.85, 8), at(x, 0.425, 0.0), pants);
    }
    p.add(cylinder(0.2, 0.6, 10), at(0.0, 1.15, 0.0), shirt);
    for x in [-0.27, 0.27] {
        p.add(cylinder(0.06, 0.6, 8), at(x, 1.15, 0.0), shirt);
    }
    p.add(sphere(0.12, 10, 8), at(0.0, 1.6, 0.0), SKIN);
    let mut m = p.finish();
    m.transform_by(Transform::from_scale(Vec3::splat(k)));
    m
}

/// Véhicules : voiture, fourgon, camion (selon le numéro), avant vers −Z. La carrosserie est blanche dans les
/// sommets : la teinte vient du matériau (réglage `color`), partagé entre tous les véhicules de même couleur.
fn vehicle(i: usize) -> Mesh {
    let mut p = Parts::new();
    let wheel = |p: &mut Parts, x: f32, z: f32, r: f32| {
        let t =
            Transform::from_xyz(x, r, z).with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
        p.add(cylinder(r, 0.25, 14), t, TIRE);
    };
    let lights = |p: &mut Parts, w: f32, l: f32, y: f32| {
        for x in [-w * 0.32, w * 0.32] {
            p.add(
                cuboid(0.3, 0.12, 0.05),
                at(x, y, -l / 2.0 - 0.02),
                Srgba::rgb(0.95, 0.92, 0.75),
            );
            p.add(
                cuboid(0.3, 0.12, 0.05),
                at(x, y, l / 2.0 + 0.02),
                Srgba::rgb(0.70, 0.05, 0.05),
            );
        }
    };
    let white = Srgba::WHITE;
    match i % 3 {
        0 => {
            let (w, l) = (1.8, 4.3);
            p.add(cuboid(w, 0.7, l), at(0.0, 0.65, 0.0), white);
            p.add(cuboid(w * 0.88, 0.6, 2.2), at(0.0, 1.3, 0.2), GLASS);
            for (x, z) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                wheel(&mut p, x * (w / 2.0 - 0.05), z * (l / 2.0 - 0.8), 0.33);
            }
            lights(&mut p, w, l, 0.8);
        }
        1 => {
            let (w, l) = (2.0, 5.0);
            p.add(cuboid(w, 1.6, l), at(0.0, 1.15, 0.0), white);
            p.add(cuboid(w * 0.9, 0.6, 0.05), at(0.0, 1.55, -l / 2.0 - 0.01), GLASS);
            for side in [-1.0, 1.0] {
                p.add(
                    cuboid(0.05, 0.5, 1.0),
                    at(side * (w / 2.0 + 0.01), 1.55, -l / 2.0 + 0.8),
                    GLASS,
                );
            }
            for (x, z) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                wheel(&mut p, x * (w / 2.0 - 0.05), z * (l / 2.0 - 0.9), 0.36);
            }
            lights(&mut p, w, l, 0.8);
        }
        _ => {
            let (w, l) = (2.4, 7.0);
            p.add(cuboid(w, 2.0, 1.8), at(0.0, 1.4, -l / 2.0 + 0.9), white);
            p.add(cuboid(w * 0.9, 0.7, 0.05), at(0.0, 1.9, -l / 2.0 - 0.01), GLASS);
            p.add(
                cuboid(w, 2.6, l - 1.9),
                at(0.0, 1.7, 0.95),
                Srgba::rgb(0.85, 0.85, 0.85),
            );
            for z in [-l / 2.0 + 0.9, l / 2.0 - 2.4, l / 2.0 - 1.1] {
                for x in [-1.0, 1.0] {
                    wheel(&mut p, x * (w / 2.0 - 0.1), z, 0.45);
                }
            }
            lights(&mut p, w, l, 0.9);
        }
    }
    p.finish()
}

/// Sol de l'île : un carré d'herbe de `size` mètres de côté.
pub fn ground_mesh(size: f32) -> Mesh {
    let mut p = Parts::new();
    p.add(
        Plane3d::default().mesh().size(size, size).build(),
        Transform::IDENTITY,
        Srgba::rgb(0.30, 0.40, 0.24),
    );
    p.finish()
}

/// Sol de la ville en **un** maillage : chaussée sur toute la ville, trottoirs / îlots par-dessus. Même
/// disposition que le générateur (îlots de `block` m, rues de `street` m centrées sur les lignes de la grille).
pub fn city_ground_mesh(city: f32, block: f32, street: f32) -> Mesh {
    let mut p = Parts::new();
    p.add(
        Plane3d::default()
            .mesh()
            .size(city + street, city + street)
            .build(),
        at(0.0, 0.03, 0.0),
        Srgba::rgb(0.18, 0.18, 0.19),
    );
    let blocks = (city / block).round().max(0.0) as usize;
    let side = block - street;
    for bi in 0..blocks {
        for bj in 0..blocks {
            let cx = -city / 2.0 + (bi as f32 + 0.5) * block;
            let cz = -city / 2.0 + (bj as f32 + 0.5) * block;
            p.add(
                Plane3d::default().mesh().size(side, side).build(),
                at(cx, 0.08, cz),
                Srgba::rgb(0.52, 0.51, 0.49),
            );
        }
    }
    p.finish()
}

/// Nombre de triangles d'un maillage indexé (ou non).
pub fn triangles(m: &Mesh) -> usize {
    m.indices().map_or(m.count_vertices(), |i| i.len()) / 3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn positions(m: &Mesh) -> Vec<[f32; 3]> {
        match m.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(v)) => v.clone(),
            _ => panic!("positions absentes"),
        }
    }

    #[test]
    fn tous_les_modeles_sont_complets_et_poses_au_sol() {
        for m in ModelRef::all() {
            let mesh = build_model(m);
            let n = mesh.count_vertices();
            for attr in [
                Mesh::ATTRIBUTE_POSITION,
                Mesh::ATTRIBUTE_NORMAL,
                Mesh::ATTRIBUTE_UV_0,
                Mesh::ATTRIBUTE_COLOR,
            ] {
                let len = mesh.attribute(attr).map(VertexAttributeValues::len);
                assert_eq!(len, Some(n), "{m:?} : attribut {attr:?}");
            }
            let pos = positions(&mesh);
            let min_y = pos.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
            assert!(min_y.abs() < 0.02, "{m:?} ne touche pas le sol (y min = {min_y})");
            assert!(pos.iter().flatten().all(|v| v.is_finite()));
            if matches!(m.kind, Kind::Building | Kind::Interior) {
                // Parcelles de 20 m : le bâtiment ne déborde pas sur la voisine.
                let reach = pos.iter().map(|p| p[0].abs().max(p[2].abs())).fold(0.0, f32::max);
                assert!(reach < 10.0, "{m:?} déborde de sa parcelle ({reach} m)");
            }
        }
    }

    #[test]
    fn modeles_deterministes() {
        for m in ModelRef::all().step_by(7) {
            assert_eq!(positions(&build_model(m)), positions(&build_model(m)), "{m:?}");
        }
    }

    #[test]
    fn triangles_par_nature() {
        // Bornes larges : elles détectent une forme vide ou un maillage qui explose, pas un réglage fin.
        for m in ModelRef::all() {
            let t = triangles(&build_model(m));
            let (lo, hi) = match m.kind {
                Kind::Building | Kind::Interior => (200, 4_000),
                Kind::Object => (20, 400),
                Kind::Tree => (50, 1_000),
                Kind::Lamp | Kind::ShadowLamp => (30, 200),
                Kind::Character => (100, 600),
                Kind::Vehicle => (50, 1_000),
            };
            assert!((lo..=hi).contains(&t), "{m:?} : {t} triangles");
        }
    }

    #[test]
    fn coque_interieure_tournee_vers_le_dedans() {
        // Normales et sens des triangles (face avant = sens trigonométrique, celle que Bevy dessine) doivent
        // tous deux regarder le centre : depuis l'intérieur, les murs sont visibles.
        let m = inverted(cuboid(4.0, 3.0, 5.0));
        let pos = positions(&m);
        let Some(VertexAttributeValues::Float32x3(normals)) = m.attribute(Mesh::ATTRIBUTE_NORMAL) else {
            panic!("normales absentes");
        };
        for (p, n) in pos.iter().zip(normals) {
            assert!(Vec3::from_array(*n).dot(-Vec3::from_array(*p)) > 0.0);
        }
        let idx: Vec<usize> = m.indices().expect("indexé").iter().collect();
        for t in idx.chunks(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from_array(pos[i]));
            let face = (b - a).cross(c - a);
            assert!(
                face.dot(-(a + b + c) / 3.0) > 0.0,
                "triangle tourné vers l'extérieur"
            );
        }
    }

    #[test]
    fn sol_de_la_ville() {
        let m = city_ground_mesh(1200.0, 100.0, 20.0);
        // 1 chaussée + 12 × 12 îlots, 2 triangles chacun.
        assert_eq!(triangles(&m), 2 * (1 + 144));
        assert_eq!(triangles(&ground_mesh(4000.0)), 2);
    }
}
