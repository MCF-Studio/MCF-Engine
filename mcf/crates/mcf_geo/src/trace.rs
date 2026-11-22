//! Tracé d'axe : points cliqués + rayon de courbure à chaque sommet intérieur, discrétisés en polyligne. La même
//! polyligne sert à la vue Plan, à la 3D, au jeu et au serveur.

use crate::P2;

/// Écart maximal entre un arc et sa corde (m).
pub const ECART_CORDE_M: f64 = 0.05;

/// Nombre maximal de points produits pour un tracé (garde-fou contre une donnée piégée).
pub const MAX_POINTS_DISCRETS: usize = 100_000;

/// Discrétise un tracé. `rayons[i]` est le rayon de courbure au sommet `i` (0 = angle vif) ; les extrémités sont
/// ignorées et un rayon trop grand pour les segments voisins est réduit. Les points non finis ou confondus doivent
/// avoir été refusés par la validation ; ici ils sont simplement ignorés.
pub fn discretiser(points: &[P2], rayons: &[f64], ecart: f64) -> Vec<P2> {
    let points: Vec<P2> = dedoublonner(
        points
            .iter()
            .copied()
            .filter(|p| p[0].is_finite() && p[1].is_finite()),
    );
    if points.len() < 3 {
        return points;
    }
    let ecart = if ecart.is_finite() && ecart > 0.0 {
        ecart
    } else {
        ECART_CORDE_M
    };
    let mut sortie = vec![points[0]];
    for i in 1..points.len() - 1 {
        let rayon = rayons.get(i).copied().filter(|r| r.is_finite() && *r > 0.0);
        match rayon.and_then(|r| arrondi(points[i - 1], points[i], points[i + 1], r, ecart)) {
            Some(arc) => sortie.extend(arc),
            None => sortie.push(points[i]),
        }
        if sortie.len() > MAX_POINTS_DISCRETS {
            sortie.truncate(MAX_POINTS_DISCRETS);
            return sortie;
        }
    }
    sortie.push(points[points.len() - 1]);
    dedoublonner(sortie.into_iter())
}

fn dedoublonner(points: impl Iterator<Item = P2>) -> Vec<P2> {
    let mut sortie: Vec<P2> = Vec::new();
    for p in points {
        if sortie.last().is_none_or(|q| crate::distance(*q, p) > 1e-9) {
            sortie.push(p);
        }
    }
    sortie
}

fn unitaire(a: P2, b: P2) -> Option<(P2, f64)> {
    let l = crate::distance(a, b);
    (l > 1e-9).then(|| ([(b[0] - a[0]) / l, (b[1] - a[1]) / l], l))
}

/// Arc tangent aux segments `a→b` et `b→c`, de rayon `r` (réduit si les segments sont trop courts : chaque segment
/// cède au plus sa moitié à chacun de ses deux sommets).
fn arrondi(a: P2, b: P2, c: P2, r: f64, ecart: f64) -> Option<Vec<P2>> {
    let (d1, l1) = unitaire(a, b)?;
    let (d2, l2) = unitaire(b, c)?;
    let croix = d1[0] * d2[1] - d1[1] * d2[0];
    let theta = (d1[0] * d2[0] + d1[1] * d2[1]).clamp(-1.0, 1.0).acos(); // angle de virage
    if theta < 1e-6 || theta > std::f64::consts::PI - 1e-6 {
        return None;
    }
    let tan = (theta * 0.5).tan();
    let t = (r * tan).min(l1 * 0.5).min(l2 * 0.5);
    let r = t / tan;
    let t1 = [b[0] - d1[0] * t, b[1] - d1[1] * t];
    let t2 = [b[0] + d2[0] * t, b[1] + d2[1] * t];
    // Normale vers l'intérieur du virage.
    let n = if croix > 0.0 {
        [-d1[1], d1[0]]
    } else {
        [d1[1], -d1[0]]
    };
    let centre = [t1[0] + n[0] * r, t1[1] + n[1] * r];
    let pas_max = if ecart >= r {
        theta
    } else {
        2.0 * (1.0 - ecart / r).acos()
    };
    let pas = ((theta / pas_max).ceil() as usize).clamp(1, 4096);
    let signe = if croix > 0.0 { 1.0 } else { -1.0 };
    let v = [t1[0] - centre[0], t1[1] - centre[1]];
    let mut arc = Vec::with_capacity(pas + 1);
    arc.push(t1);
    for k in 1..pas {
        let ang = signe * theta * k as f64 / pas as f64;
        let (s, co) = ang.sin_cos();
        arc.push([centre[0] + v[0] * co - v[1] * s, centre[1] + v[0] * s + v[1] * co]);
    }
    arc.push(t2);
    Some(arc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distance;

    #[test]
    fn sans_rayon_les_points_sont_conserves() {
        let pts = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        assert_eq!(discretiser(&pts, &[0.0; 3], ECART_CORDE_M), pts.to_vec());
    }

    #[test]
    fn virage_a_angle_droit_suit_le_cercle() {
        let pts = [[0.0, 0.0], [50.0, 0.0], [50.0, 50.0]];
        let sortie = discretiser(&pts, &[0.0, 12.0, 0.0], ECART_CORDE_M);
        // Le centre du cercle de 12 m est en (38, 12) : tous les points de l'arc sont à 12 m.
        let arc: Vec<_> = sortie.iter().filter(|p| p[0] > 38.0 && p[1] < 12.0).collect();
        assert!(arc.len() > 5);
        for p in arc {
            assert!((distance(*p, [38.0, 12.0]) - 12.0).abs() < 1e-9);
        }
        // Écart de corde respecté entre deux points consécutifs.
        for s in sortie.windows(2) {
            let milieu = [(s[0][0] + s[1][0]) * 0.5, (s[0][1] + s[1][1]) * 0.5];
            let d = distance(milieu, [38.0, 12.0]);
            if d < 12.0 {
                assert!(12.0 - d <= ECART_CORDE_M + 1e-9);
            }
        }
    }

    #[test]
    fn rayon_trop_grand_reduit() {
        let pts = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0]];
        let sortie = discretiser(&pts, &[0.0, 1000.0, 0.0], ECART_CORDE_M);
        assert_eq!(sortie.first(), Some(&[0.0, 0.0]));
        assert_eq!(sortie.last(), Some(&[4.0, 4.0]));
        assert!(sortie.iter().all(|p| p[0] <= 4.0 + 1e-9 && p[1] >= -1e-9));
    }

    #[test]
    fn donnees_degenerees() {
        assert!(discretiser(&[], &[], ECART_CORDE_M).is_empty());
        let pts = [[0.0, 0.0], [0.0, 0.0], [f64::NAN, 1.0], [5.0, 0.0]];
        assert_eq!(
            discretiser(&pts, &[], ECART_CORDE_M),
            vec![[0.0, 0.0], [5.0, 0.0]]
        );
    }
}
