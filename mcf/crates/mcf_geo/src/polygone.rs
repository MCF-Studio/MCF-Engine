//! Polygones simples du plan : aire, orientation, appartenance, intersections, triangulation (oreilles) et
//! retrait intérieur. Utilisés par les carrefours, îlots, zones et empreintes de bâtiments.

use crate::P2;

/// Aire signée (positive si le polygone tourne dans le sens trigonométrique du repère `(x, z)`).
pub fn aire_signee(poly: &[P2]) -> f64 {
    let n = poly.len();
    if n < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    for i in 0..n {
        let p = poly[i];
        let q = poly[(i + 1) % n];
        a += p[0] * q[1] - q[0] * p[1];
    }
    a * 0.5
}

/// Aire absolue.
pub fn aire(poly: &[P2]) -> f64 {
    aire_signee(poly).abs()
}

/// Copie orientée positivement (aire signée > 0).
pub fn oriente_positif(poly: &[P2]) -> Vec<P2> {
    let mut v = poly.to_vec();
    if aire_signee(&v) < 0.0 {
        v.reverse();
    }
    v
}

/// Centre de gravité (moyenne des sommets si l'aire est nulle).
pub fn centroide(poly: &[P2]) -> P2 {
    let n = poly.len();
    if n == 0 {
        return [0.0, 0.0];
    }
    let a = aire_signee(poly);
    if a.abs() < 1e-12 {
        let s = poly.iter().fold([0.0, 0.0], |s, p| [s[0] + p[0], s[1] + p[1]]);
        return [s[0] / n as f64, s[1] / n as f64];
    }
    let (mut cx, mut cz) = (0.0, 0.0);
    for i in 0..n {
        let p = poly[i];
        let q = poly[(i + 1) % n];
        let c = p[0] * q[1] - q[0] * p[1];
        cx += (p[0] + q[0]) * c;
        cz += (p[1] + q[1]) * c;
    }
    [cx / (6.0 * a), cz / (6.0 * a)]
}

/// Le point est-il dans le polygone (règle pair-impair) ?
pub fn contient(poly: &[P2], p: P2) -> bool {
    let n = poly.len();
    let mut dedans = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0];
            if p[0] < x {
                dedans = !dedans;
            }
        }
        j = i;
    }
    dedans
}

fn croix(o: P2, a: P2, b: P2) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// Les segments `[a, b]` et `[c, d]` se coupent-ils (intérieurs, extrémités communes exclues) ?
pub fn segments_se_coupent(a: P2, b: P2, c: P2, d: P2) -> bool {
    let d1 = croix(c, d, a);
    let d2 = croix(c, d, b);
    let d3 = croix(a, b, c);
    let d4 = croix(a, b, d);
    ((d1 > 1e-12 && d2 < -1e-12) || (d1 < -1e-12 && d2 > 1e-12))
        && ((d3 > 1e-12 && d4 < -1e-12) || (d3 < -1e-12 && d4 > 1e-12))
}

/// Point d'intersection des segments `[a, b]` et `[c, d]`, avec le paramètre `t` sur `[a, b]`.
pub fn intersection_segments(a: P2, b: P2, c: P2, d: P2) -> Option<(P2, f64)> {
    let r = [b[0] - a[0], b[1] - a[1]];
    let s = [d[0] - c[0], d[1] - c[1]];
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1e-12 {
        return None;
    }
    let q = [c[0] - a[0], c[1] - a[1]];
    let t = (q[0] * s[1] - q[1] * s[0]) / den;
    let u = (q[0] * r[1] - q[1] * r[0]) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| ([a[0] + r[0] * t, a[1] + r[1] * t], t))
}

/// Intersection de deux droites `(p, direction d)` et `(q, direction e)`.
pub fn intersection_droites(p: P2, d: P2, q: P2, e: P2) -> Option<P2> {
    let den = d[0] * e[1] - d[1] * e[0];
    if den.abs() < 1e-9 {
        return None;
    }
    let t = ((q[0] - p[0]) * e[1] - (q[1] - p[1]) * e[0]) / den;
    Some([p[0] + d[0] * t, p[1] + d[1] * t])
}

/// Polygone simple : au moins 3 sommets finis, aire non nulle, aucune arête qui en coupe une autre.
pub fn est_simple(poly: &[P2]) -> bool {
    let n = poly.len();
    if n < 3 || poly.iter().any(|p| !p[0].is_finite() || !p[1].is_finite()) || aire(poly) < 1e-9 {
        return false;
    }
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        for j in i + 1..n {
            if j == i || (j + 1) % n == i || (i + 1) % n == j {
                continue;
            }
            if segments_se_coupent(a, b, poly[j], poly[(j + 1) % n]) {
                return false;
            }
        }
    }
    true
}

/// Triangulation par oreilles d'un polygone simple. Renvoie des indices dans `poly`, triangles d'aire signée
/// positive dans `(x, z)`. **Attention** : vus de +Y en 3D, ils sont alors à l'envers ; `maillage_polygone` les
/// retourne.
pub fn trianguler(poly: &[P2]) -> Vec<[usize; 3]> {
    let n = poly.len();
    if n < 3 {
        return Vec::new();
    }
    let positif = aire_signee(poly) >= 0.0;
    let mut restants: Vec<usize> = if positif { (0..n).collect() } else { (0..n).rev().collect() };
    let mut sortie = Vec::with_capacity(n - 2);
    let mut garde = 0;
    while restants.len() > 3 && garde < n * n {
        garde += 1;
        let m = restants.len();
        let mut trouve = false;
        for k in 0..m {
            let (ia, ib, ic) = (restants[(k + m - 1) % m], restants[k], restants[(k + 1) % m]);
            let (a, b, c) = (poly[ia], poly[ib], poly[ic]);
            if croix(a, b, c) <= 1e-12 {
                continue; // sommet rentrant ou plat
            }
            let contient_autre = restants.iter().any(|&j| {
                j != ia
                    && j != ib
                    && j != ic
                    && croix(a, b, poly[j]) >= -1e-12
                    && croix(b, c, poly[j]) >= -1e-12
                    && croix(c, a, poly[j]) >= -1e-12
            });
            if !contient_autre {
                sortie.push([ia, ib, ic]);
                restants.remove(k);
                trouve = true;
                break;
            }
        }
        if !trouve {
            // Polygone dégénéré : on retire un sommet plat pour avancer plutôt que de boucler.
            let k = (0..m)
                .min_by(|&x, &y| {
                    let f = |k: usize| croix(poly[restants[(k + m - 1) % m]], poly[restants[k]], poly[restants[(k + 1) % m]]).abs();
                    f(x).total_cmp(&f(y))
                })
                .unwrap_or(0);
            restants.remove(k);
        }
    }
    if restants.len() == 3 && croix(poly[restants[0]], poly[restants[1]], poly[restants[2]]) > 1e-12 {
        sortie.push([restants[0], restants[1], restants[2]]);
    }
    sortie
}

/// Retrait intérieur d'un polygone de `retraits[i]` mètres sur l'arête `i` (de `poly[i]` à `poly[i+1]`). Les arêtes
/// décalées sont raccordées par intersection de droites. `None` si le résultat n'est plus un polygone simple de même
/// orientation (îlot trop petit pour ces retraits).
pub fn retrait(poly: &[P2], retraits: &[f64]) -> Option<Vec<P2>> {
    let n = poly.len();
    if n < 3 || retraits.len() != n {
        return None;
    }
    if aire_signee(poly) < 0.0 {
        // Arête i du polygone inversé = arête (n - 2 - i) mod n de l'original.
        let inverse: Vec<P2> = poly.iter().rev().copied().collect();
        let r: Vec<f64> = (0..n).map(|i| retraits[(2 * n - 2 - i) % n]).collect();
        return retrait(&inverse, &r);
    }
    // Polygone positif : l'intérieur est à gauche en sens trigonométrique, soit (-dz, dx).
    let lignes: Vec<Option<(P2, P2)>> = (0..n)
        .map(|i| {
            let a = poly[i];
            let b = poly[(i + 1) % n];
            let l = crate::distance(a, b);
            (l > 1e-9).then(|| {
                let d = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
                let normale = [-d[1], d[0]];
                ([a[0] + normale[0] * retraits[i], a[1] + normale[1] * retraits[i]], d)
            })
        })
        .collect();
    let mut sortie = Vec::with_capacity(n);
    for i in 0..n {
        let prec = lignes[(i + n - 1) % n];
        let cour = lignes[i];
        match (prec, cour) {
            (Some((p, d)), Some((q, e))) => match intersection_droites(p, d, q, e) {
                Some(x) => sortie.push(x),
                None => sortie.push(q),
            },
            (_, Some((q, _))) => sortie.push(q),
            _ => {}
        }
    }
    // Un retrait trop fort retourne le polygone (même orientation, arêtes inversées) : chaque arête doit garder
    // son sens d'origine.
    let meme_sens = sortie.len() == n
        && (0..n).all(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let (p, q) = (sortie[i], sortie[(i + 1) % n]);
            (b[0] - a[0]) * (q[0] - p[0]) + (b[1] - a[1]) * (q[1] - p[1]) > 0.0
        });
    (meme_sens && est_simple(&sortie) && aire_signee(&sortie) > 0.0).then_some(sortie)
}

/// Rectangle englobant `(min, max)`.
pub fn englobant(poly: &[P2]) -> (P2, P2) {
    poly.iter().fold(([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]), |(a, b), p| {
        ([a[0].min(p[0]), a[1].min(p[1])], [b[0].max(p[0]), b[1].max(p[1])])
    })
}

/// Deux polygones simples se chevauchent-ils (intérieurs communs) ? Des polygones qui se touchent seulement par un
/// bord peuvent être signalés : réduire légèrement l'un d'eux pour tolérer la mitoyenneté.
pub fn se_chevauchent(a: &[P2], b: &[P2]) -> bool {
    if a.len() < 3 || b.len() < 3 {
        return false;
    }
    let (amin, amax) = englobant(a);
    let (bmin, bmax) = englobant(b);
    if amax[0] < bmin[0] || bmax[0] < amin[0] || amax[1] < bmin[1] || bmax[1] < amin[1] {
        return false;
    }
    for i in 0..a.len() {
        let (p, q) = (a[i], a[(i + 1) % a.len()]);
        for j in 0..b.len() {
            if segments_se_coupent(p, q, b[j], b[(j + 1) % b.len()]) {
                return true;
            }
        }
    }
    contient(a, b[0]) || contient(b, a[0])
}

/// Polygone réduit vers son centre de `marge` mètres (approximation par homothétie, pour les tests de contact).
pub fn reduit(poly: &[P2], marge: f64) -> Vec<P2> {
    let c = centroide(poly);
    poly.iter()
        .map(|p| {
            let d = crate::distance(*p, c);
            if d <= marge {
                c
            } else {
                let k = (d - marge) / d;
                [c[0] + (p[0] - c[0]) * k, c[1] + (p[1] - c[1]) * k]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARRE: [P2; 4] = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];

    #[test]
    fn aire_et_orientation() {
        assert_eq!(aire_signee(&CARRE), 100.0);
        let inverse: Vec<P2> = CARRE.iter().rev().copied().collect();
        assert_eq!(aire_signee(&inverse), -100.0);
        assert_eq!(oriente_positif(&inverse), {
            let mut v = inverse.clone();
            v.reverse();
            v
        });
        assert_eq!(centroide(&CARRE), [5.0, 5.0]);
    }

    #[test]
    fn appartenance() {
        assert!(contient(&CARRE, [5.0, 5.0]));
        assert!(!contient(&CARRE, [15.0, 5.0]));
    }

    #[test]
    fn simple_ou_croise() {
        assert!(est_simple(&CARRE));
        assert!(!est_simple(&[[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]]));
        assert!(!est_simple(&[[0.0, 0.0], [1.0, 1.0]]));
    }

    #[test]
    fn triangulation_d_un_l() {
        let l = [[0.0, 0.0], [20.0, 0.0], [20.0, 5.0], [5.0, 5.0], [5.0, 20.0], [0.0, 20.0]];
        let t = trianguler(&l);
        assert_eq!(t.len(), 4);
        let somme: f64 = t.iter().map(|t| aire_signee(&[l[t[0]], l[t[1]], l[t[2]]])).sum();
        assert!((somme - aire(&l)).abs() < 1e-9);
        assert!(t.iter().all(|t| aire_signee(&[l[t[0]], l[t[1]], l[t[2]]]) > 0.0));
        // Polygone horaire : mêmes aires, triangles toujours positifs.
        let inverse: Vec<P2> = l.iter().rev().copied().collect();
        let t = trianguler(&inverse);
        assert!(t.iter().all(|t| aire_signee(&[inverse[t[0]], inverse[t[1]], inverse[t[2]]]) > 0.0));
    }

    #[test]
    fn retrait_uniforme_et_variable() {
        let r = retrait(&CARRE, &[1.0; 4]).expect("retrait");
        assert!((aire(&r) - 64.0).abs() < 1e-9);
        let r = retrait(&CARRE, &[1.0, 2.0, 1.0, 2.0]).expect("retrait");
        assert!((aire(&r) - 48.0).abs() < 1e-9);
        assert!(retrait(&CARRE, &[6.0; 4]).is_none());
        // Même résultat quelle que soit l'orientation d'entrée.
        let inverse: Vec<P2> = CARRE.iter().rev().copied().collect();
        let r2 = retrait(&inverse, &[1.0; 4]).expect("retrait");
        assert!((aire(&r2) - 64.0).abs() < 1e-9);
    }

    #[test]
    fn chevauchement() {
        let b = [[5.0, 5.0], [15.0, 5.0], [15.0, 15.0], [5.0, 15.0]];
        assert!(se_chevauchent(&CARRE, &b));
        let voisin = [[10.0, 0.0], [20.0, 0.0], [20.0, 10.0], [10.0, 10.0]];
        assert!(!se_chevauchent(&reduit(&CARRE, 0.01), &voisin));
        let dedans = [[2.0, 2.0], [3.0, 2.0], [3.0, 3.0]];
        assert!(se_chevauchent(&CARRE, &dedans));
        assert!(!se_chevauchent(&CARRE, &[[20.0, 20.0], [30.0, 20.0], [30.0, 30.0]]));
    }

    #[test]
    fn intersections() {
        let (p, t) = intersection_segments([0.0, 0.0], [10.0, 0.0], [5.0, -5.0], [5.0, 5.0]).expect("croise");
        assert_eq!(p, [5.0, 0.0]);
        assert_eq!(t, 0.5);
        assert!(intersection_segments([0.0, 0.0], [1.0, 0.0], [5.0, -5.0], [5.0, 5.0]).is_none());
    }
}
