//! Passage `.mcfmap` ↔ Bevy. Godot et Bevy ont le même repère (Y vers le haut, main droite, avant = −Z,
//! mètres) : il n'y a **aucune** conversion d'axes, seulement un changement de type (f64 → f32).

use bevy::math::{Mat3, Mat4, Vec3};
use bevy::transform::components::Transform;
use mcf_map::MapTransform;

/// Placement `.mcfmap` → `Transform` Bevy. La base est supposée rotation × échelle (ce que la validation et
/// l'éditeur garantissent).
pub fn to_bevy(t: &MapTransform) -> Transform {
    let v = |a: [f64; 3]| Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
    let basis = Mat3::from_cols(v(t.basis[0]), v(t.basis[1]), v(t.basis[2]));
    let m = Mat4::from_cols(
        basis.x_axis.extend(0.0),
        basis.y_axis.extend(0.0),
        basis.z_axis.extend(0.0),
        v(t.origin).extend(1.0),
    );
    Transform::from_matrix(m)
}

/// `Transform` Bevy → placement `.mcfmap`.
pub fn from_bevy(t: &Transform) -> MapTransform {
    let basis = Mat3::from_quat(t.rotation) * Mat3::from_diagonal(t.scale);
    let d = |v: Vec3| [f64::from(v.x), f64::from(v.y), f64::from(v.z)];
    MapTransform {
        origin: d(t.translation),
        basis: [d(basis.x_axis), d(basis.y_axis), d(basis.z_axis)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Quat;

    fn close(a: &MapTransform, b: &MapTransform, tol: f64) -> bool {
        let o = a.origin.iter().zip(b.origin).all(|(x, y)| (x - y).abs() <= tol);
        let bs = a
            .basis
            .iter()
            .flatten()
            .zip(b.basis.iter().flatten())
            .all(|(x, y)| (x - y).abs() <= 1e-5);
        o && bs
    }

    #[test]
    fn aller_retour_au_millimetre() {
        // Exigence du cahier des charges : export → import → mêmes positions au millimètre, jusqu'au bord de
        // la carte (±5 km).
        let rot = Quat::from_euler(bevy::math::EulerRot::YXZ, 1.2, 0.3, -0.1);
        for origin in [
            [0.0, 0.0, 0.0],
            [4999.9, 120.25, -4999.9],
            [-1234.567, 3.21, 987.654],
        ] {
            let t = Transform {
                translation: Vec3::ZERO,
                rotation: rot,
                scale: Vec3::new(2.0, 0.5, 1.0),
            };
            let mut m = from_bevy(&t);
            m.origin = origin;
            let back = from_bevy(&to_bevy(&m));
            assert!(close(&m, &back, 0.001), "{m:?} != {back:?}");
        }
    }

    #[test]
    fn meme_repere_que_godot() {
        // Basis Godot rotation de 90° autour de Y : X → -Z, Z → X. Même résultat attendu dans Bevy.
        let m = MapTransform {
            origin: [1.0, 2.0, 3.0],
            basis: [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
        };
        let t = to_bevy(&m);
        // Chaque axe de la base Godot doit être l'image de l'axe Bevy correspondant.
        for (axis, expected) in [(Vec3::X, Vec3::NEG_Z), (Vec3::Y, Vec3::Y), (Vec3::Z, Vec3::X)] {
            let got = t.rotation * axis;
            assert!(
                (got - expected).length() < 1e-6,
                "{axis} → {got}, attendu {expected}"
            );
        }
        assert_eq!(t.translation, Vec3::new(1.0, 2.0, 3.0));
        // Et c'est bien la rotation de +90° autour de Y de Bevy (comparaison par produit scalaire : `angle_between`
        // passe par un acos imprécis près de 1 en f32).
        let expected = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        assert!(t.rotation.dot(expected).abs() > 1.0 - 1e-6);
    }
}
