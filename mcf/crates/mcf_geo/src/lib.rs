//! Noyau de géométrie de carte MCF (ADR-0001, `docs/SPEC_CONCEPTION_CARTE_2D_3D.md`).
//!
//! **Seule** source de géométrie des routes : la vue Plan et la scène 3D affichent les maillages produits ici, et
//! rien d'autre. C'est ce qui garantit qu'un bâtiment au bord d'une route en 2D l'est aussi en 3D.
//!
//! Conventions : mètres, `f64`, plan horizontal `(x, z)` du repère MCF (Y vers le haut, nord = −Z). L'altitude ne
//! modifie jamais `x` ni `z`.

pub mod profil;
pub mod ruban;
pub mod trace;

pub use profil::{ClasseRoute, Cote, ProfilRoute, Revetement, TableProfils};
pub use ruban::{Bande, Maillage, NatureBande, bandes, decaler, maillage_bande};
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
