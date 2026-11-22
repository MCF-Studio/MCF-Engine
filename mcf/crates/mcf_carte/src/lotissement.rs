//! Lotissement : découpe les façades d'un îlot en parcelles et y fait « pousser » des blocs selon le gabarit.
//! Déterministe (même graine, même résultat) et respectueux des obstacles (bâtiments manuels, emprises).

use mcf_geo::{P2, polygone};

use crate::gabarits::Gabarit;
use crate::ids::Alea;

/// Bâtiments maximum générés par îlot.
pub const MAX_PAR_ILOT: usize = 2_000;
/// Marge de tolérance des contacts (mitoyenneté) en mètres.
const MARGE: f64 = 0.02;

/// Un bloc proposé : empreinte et niveaux.
pub type Bloc = (Vec<P2>, u32);

fn rectangle(a: P2, d: P2, n: P2, x: f64, w: f64, s: f64, p: f64) -> Vec<P2> {
    let pt = |u: f64, v: f64| [a[0] + d[0] * u + n[0] * v, a[1] + d[1] * u + n[1] * v];
    vec![pt(x, s), pt(x + w, s), pt(x + w, s + p), pt(x, s + p)]
}

fn dans(ilot: &[P2], r: &[P2]) -> bool {
    r.iter().all(|p| polygone::contient(ilot, *p))
        && (0..r.len()).all(|i| {
            let (a, b) = (r[i], r[(i + 1) % r.len()]);
            (0..ilot.len()).all(|j| !polygone::segments_se_coupent(a, b, ilot[j], ilot[(j + 1) % ilot.len()]))
        })
}

/// Lotit un îlot. `interieur` : surface constructible (sens trigonométrique), `obstacles` : polygones à éviter.
pub fn lotir(interieur: &[P2], gabarit: &Gabarit, graine: u64, obstacles: &[Vec<P2>]) -> Vec<Bloc> {
    let ilot = polygone::oriente_positif(interieur);
    let n = ilot.len();
    let (imin, imax) = polygone::englobant(&ilot);
    let proches: Vec<Vec<P2>> = obstacles
        .iter()
        .filter(|o| {
            let (a, b) = polygone::englobant(o);
            !(b[0] < imin[0] || imax[0] < a[0] || b[1] < imin[1] || imax[1] < a[1])
        })
        .map(|o| polygone::reduit(o, MARGE))
        .collect();
    let mut alea = Alea::new(graine);
    let mut blocs: Vec<Bloc> = Vec::new();
    let libre = |r: &[P2], blocs: &[Bloc]| {
        let r = polygone::reduit(r, MARGE);
        dans(&ilot, &r)
            && !blocs.iter().any(|(b, _)| polygone::se_chevauchent(&r, b))
            && !proches.iter().any(|o| polygone::se_chevauchent(&r, o))
    };
    for i in 0..n {
        let (a, b) = (ilot[i], ilot[(i + 1) % n]);
        let l = mcf_geo::distance(a, b);
        if l < gabarit.facade_m[0] {
            continue;
        }
        let d = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
        let nrm = [-d[1], d[0]]; // vers l'intérieur
        let mut x = 0.0;
        while x + gabarit.facade_m[0] <= l && blocs.len() < MAX_PAR_ILOT {
            let w = alea.entre(gabarit.facade_m[0], gabarit.facade_m[1]).min(l - x);
            let s = alea.entre(gabarit.retrait_m[0], gabarit.retrait_m[1]);
            let profondeur = alea.entre(gabarit.profondeur_m[0], gabarit.profondeur_m[1]);
            let niveaux = alea.entier(gabarit.niveaux[0], gabarit.niveaux[1]);
            let mut place = None;
            for p in [profondeur, gabarit.profondeur_m[0]] {
                let r = rectangle(a, d, nrm, x, w, s, p);
                if libre(&r, &blocs) {
                    place = Some(r);
                    break;
                }
            }
            match place {
                Some(r) => {
                    blocs.push((r, niveaux));
                    x += w + if gabarit.mitoyen { 0.0 } else { alea.entre(2.0, 6.0) };
                }
                None => x += 2.0,
            }
        }
    }
    blocs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gabarits::{Densite, Famille, TableGabarits};

    fn ilot() -> Vec<P2> {
        vec![[0.0, 0.0], [86.0, 0.0], [86.0, 86.0], [0.0, 86.0]]
    }

    #[test]
    fn blocs_dans_l_ilot_sans_chevauchement() {
        let t = TableGabarits::defaut();
        for (f, d) in [(Famille::Residentiel, Densite::Moyenne), (Famille::Residentiel, Densite::Faible)] {
            let g = t.get(f, d).expect("gabarit");
            let blocs = lotir(&ilot(), g, 42, &[]);
            assert!(blocs.len() >= 4, "{f:?} {d:?} : {}", blocs.len());
            for (i, (b, niv)) in blocs.iter().enumerate() {
                assert!((g.niveaux[0]..=g.niveaux[1]).contains(niv));
                assert!(b.iter().all(|p| polygone::contient(&ilot(), *p) || on_edge(*p)));
                for (c, _) in &blocs[i + 1..] {
                    assert!(!polygone::se_chevauchent(&polygone::reduit(b, 0.05), &polygone::reduit(c, 0.05)));
                }
            }
            // Déterministe.
            assert_eq!(blocs, lotir(&ilot(), g, 42, &[]));
        }
    }

    fn on_edge(p: P2) -> bool {
        p[0].abs() < 1e-6 || p[1].abs() < 1e-6 || (p[0] - 86.0).abs() < 1e-6 || (p[1] - 86.0).abs() < 1e-6
    }

    #[test]
    fn obstacle_respecte() {
        let t = TableGabarits::defaut();
        let g = t.get(Famille::Residentiel, Densite::Moyenne).expect("gabarit");
        let obstacle = vec![[0.0, 0.0], [86.0, 0.0], [86.0, 20.0], [0.0, 20.0]];
        let blocs = lotir(&ilot(), g, 1, std::slice::from_ref(&obstacle));
        assert!(blocs.iter().all(|(b, _)| !polygone::se_chevauchent(&polygone::reduit(b, 0.05), &obstacle)));
    }
}
