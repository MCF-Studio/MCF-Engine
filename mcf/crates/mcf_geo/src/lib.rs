//! Noyau de géométrie de carte MCF (ADR-0001, `docs/SPEC_CONCEPTION_CARTE_2D_3D.md`).
//!
//! **Seule** source de géométrie des routes : la vue Plan et la scène 3D affichent les maillages produits ici, et
//! rien d'autre. C'est ce qui garantit qu'un bâtiment au bord d'une route en 2D l'est aussi en 3D.
//!
//! Conventions : mètres, `f64`, plan horizontal `(x, z)` du repère MCF (Y vers le haut, nord = −Z). L'altitude ne
//! modifie jamais `x` ni `z`.

pub mod carrefour;
pub mod polygone;
pub mod profil;
pub mod ruban;
pub mod trace;

pub use carrefour::{Bras, Carrefour, carrefour};
pub use profil::{ClasseRoute, Cote, ProfilRoute, Revetement, TableProfils};
pub use ruban::{Bande, Maillage, NatureBande, bandes, decaler, maillage_bande, maillage_polygone};
pub use trace::{ECART_CORDE_M, discretiser};

/// Point du plan horizontal : `[x, z]` en mètres.
pub type P2 = [f64; 2];

/// Distance horizontale entre deux points.
pub fn distance(a: P2, b: P2) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// Distance horizontale d'un point à un segment.
pub fn distance_segment(p: P2, a: P2, b: P2) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l2 = d[0] * d[0] + d[1] * d[1];
    if l2 <= f64::EPSILON {
        return distance(p, a);
    }
    let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
    distance(p, [a[0] + d[0] * t, a[1] + d[1] * t])
}

/// Distance horizontale d'un point à une polyligne (`f64::INFINITY` si elle est vide).
pub fn distance_polyligne(p: P2, ligne: &[P2]) -> f64 {
    match ligne {
        [] => f64::INFINITY,
        [seul] => distance(p, *seul),
        _ => ligne
            .windows(2)
            .map(|s| distance_segment(p, s[0], s[1]))
            .fold(f64::INFINITY, f64::min),
    }
}

/// Longueur d'une polyligne.
pub fn longueur(ligne: &[P2]) -> f64 {
    ligne.windows(2).map(|s| distance(s[0], s[1])).sum()
}

/// Point situé à la distance `d` depuis le début d'une polyligne (borné aux extrémités).
pub fn point_a(ligne: &[P2], d: f64) -> Option<P2> {
    let mut reste = d.max(0.0);
    for s in ligne.windows(2) {
        let l = distance(s[0], s[1]);
        if reste <= l && l > 0.0 {
            let t = reste / l;
            return Some([s[0][0] + (s[1][0] - s[0][0]) * t, s[0][1] + (s[1][1] - s[0][1]) * t]);
        }
        reste -= l;
    }
    ligne.last().copied()
}

/// Polyligne raccourcie de `debut` mètres au départ et `fin` mètres à l'arrivée (vide si plus rien ne reste).
pub fn rogner(ligne: &[P2], debut: f64, fin: f64) -> Vec<P2> {
    let total = longueur(ligne);
    let (a, b) = (debut.max(0.0), total - fin.max(0.0));
    if ligne.len() < 2 || b - a < 1e-6 {
        return Vec::new();
    }
    let mut sortie = Vec::new();
    let mut cumul = 0.0;
    if let Some(p) = point_a(ligne, a) {
        sortie.push(p);
    }
    for s in ligne.windows(2) {
        cumul += distance(s[0], s[1]);
        if cumul > a + 1e-9 && cumul < b - 1e-9 {
            sortie.push(s[1]);
        }
    }
    if let Some(p) = point_a(ligne, b) {
        sortie.push(p);
    }
    sortie
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rognage() {
        let l = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        assert_eq!(longueur(&l), 20.0);
        assert_eq!(rogner(&l, 2.0, 3.0), vec![[2.0, 0.0], [10.0, 0.0], [10.0, 7.0]]);
        assert!(rogner(&l, 15.0, 6.0).is_empty());
        assert_eq!(point_a(&l, 15.0), Some([10.0, 5.0]));
    }
}
