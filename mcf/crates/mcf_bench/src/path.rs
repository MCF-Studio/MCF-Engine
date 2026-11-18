use bevy::math::Vec3;
use bevy::transform::components::Transform;

/// Point de passage du parcours caméra.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraKey {
    /// Instant de passage, en secondes depuis le début du parcours.
    pub time: f32,
    /// Position de la caméra.
    pub position: Vec3,
    /// Point regardé.
    pub look_at: Vec3,
}

/// Parcours caméra : courbe de Catmull-Rom passant par chaque point, échantillonnée selon le temps écoulé. Le
/// même parcours donne exactement les mêmes images d'une exécution à l'autre (à durée d'image près).
#[derive(Debug, Clone, PartialEq)]
pub struct CameraPath {
    keys: Vec<CameraKey>,
}

impl CameraPath {
    /// `None` si moins de 2 points, ou si les instants ne sont pas strictement croissants et finis.
    pub fn new(keys: Vec<CameraKey>) -> Option<Self> {
        let ok = keys.len() >= 2
            && keys
                .iter()
                .all(|k| k.time.is_finite() && k.position.is_finite() && k.look_at.is_finite())
            && keys.windows(2).all(|w| w[1].time > w[0].time);
        ok.then_some(Self { keys })
    }

    /// Durée totale.
    pub fn duration(&self) -> f32 {
        self.last().time - self.first().time
    }

    fn first(&self) -> &CameraKey {
        &self.keys[0]
    }

    fn last(&self) -> &CameraKey {
        &self.keys[self.keys.len() - 1]
    }

    /// Position et point regardé à l'instant `t` (borné au parcours).
    pub fn sample(&self, t: f32) -> (Vec3, Vec3) {
        let t = t.clamp(self.first().time, self.last().time);
        let i = self
            .keys
            .partition_point(|k| k.time <= t)
            .clamp(1, self.keys.len() - 1)
            - 1;
        let k = |j: isize| self.keys[(i as isize + j).clamp(0, self.keys.len() as isize - 1) as usize];
        let (k0, k1, k2, k3) = (k(-1), k(0), k(1), k(2));
        let u = (t - k1.time) / (k2.time - k1.time);
        (
            catmull_rom(k0.position, k1.position, k2.position, k3.position, u),
            catmull_rom(k0.look_at, k1.look_at, k2.look_at, k3.look_at, u),
        )
    }

    /// `Transform` de la caméra à l'instant `t`.
    pub fn transform_at(&self, t: f32) -> Transform {
        let (pos, target) = self.sample(t);
        let mut tr = Transform::from_translation(pos);
        if (target - pos).length_squared() > 1e-8 {
            tr.look_at(target, Vec3::Y);
        }
        tr
    }
}

fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, u: f32) -> Vec3 {
    let u2 = u * u;
    let u3 = u2 * u;
    0.5 * ((2.0 * p1)
        + (p2 - p0) * u
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * u2
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * u3)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn key(time: f32, x: f32) -> CameraKey {
        CameraKey {
            time,
            position: Vec3::new(x, 10.0, 0.0),
            look_at: Vec3::ZERO,
        }
    }

    #[test]
    fn passe_par_les_points() {
        let p = CameraPath::new(vec![key(0.0, 0.0), key(2.0, 10.0), key(5.0, -3.0), key(6.0, 4.0)]).unwrap();
        for k in [key(0.0, 0.0), key(2.0, 10.0), key(5.0, -3.0), key(6.0, 4.0)] {
            let (pos, _) = p.sample(k.time);
            assert!((pos - k.position).length() < 1e-4, "t={} {pos}", k.time);
        }
        assert_eq!(p.duration(), 6.0);
    }

    #[test]
    fn borne_hors_parcours() {
        let p = CameraPath::new(vec![key(0.0, 0.0), key(1.0, 10.0)]).unwrap();
        assert!((p.sample(-5.0).0.x - 0.0).abs() < 1e-6);
        assert!((p.sample(99.0).0.x - 10.0).abs() < 1e-6);
    }

    #[test]
    fn parcours_invalides() {
        assert!(CameraPath::new(vec![key(0.0, 0.0)]).is_none());
        assert!(CameraPath::new(vec![key(1.0, 0.0), key(1.0, 1.0)]).is_none());
        assert!(CameraPath::new(vec![key(0.0, 0.0), key(f32::NAN, 1.0)]).is_none());
    }

    #[test]
    fn regarde_la_cible() {
        let p = CameraPath::new(vec![key(0.0, 0.0), key(1.0, 10.0)]).unwrap();
        let t = p.transform_at(0.5);
        let dir = (Vec3::ZERO - t.translation).normalize();
        assert!((t.forward().as_vec3() - dir).length() < 1e-4);
    }
}
