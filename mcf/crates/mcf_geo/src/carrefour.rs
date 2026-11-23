//! Carrefours : à chaque nœud du réseau, les rubans des tronçons sont reculés et l'espace central est rempli par un
//! polygone de chaussée et un polygone d'emprise, avec des bordures arrondies entre bras voisins.

use crate::{P2, ProfilRoute};

/// Un bras du carrefour.
#[derive(Debug, Clone, Copy)]
pub struct Bras {
    /// Direction unitaire du centre vers le tronçon.
    pub direction: P2,
    /// Profil du tronçon.
    pub profil: ProfilRoute,
    /// Recul maximal autorisé (moitié de la longueur du tronçon, par exemple).
    pub recul_max: f64,
}

/// Géométrie d'un carrefour.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Carrefour {
    /// Distance, le long de chaque bras (même ordre que l'entrée), où le ruban du tronçon s'arrête.
    pub reculs: Vec<f64>,
    /// Polygone de chaussée (vide si inutile : impasse ou continuité droite).
    pub chaussee: Vec<P2>,
    /// Polygone d'emprise (chaussée + côtés).
    pub emprise: Vec<P2>,
}

/// Recul maximal absolu d'un bras (m).
pub const RECUL_LIMITE_M: f64 = 80.0;
/// Points par arrondi de bordure.
const PAS_ARRONDI: usize = 8;

fn angle(d: P2) -> f64 {
    d[1].atan2(d[0])
}

/// Calcule le carrefour au point `centre`. `rayon_bordure` : rayon des arrondis entre bras (m).
pub fn carrefour(centre: P2, bras: &[Bras], rayon_bordure: f64) -> Carrefour {
    let n = bras.len();
    let mut reculs = vec![0.0; n];
    if n < 2 {
        return Carrefour {
            reculs,
            ..Default::default()
        };
    }
    // Recul : là où le bord de ce bras sort de la bande de chacun des autres.
    for i in 0..n {
        let wi = bras[i].profil.emprise_m() * 0.5;
        let mut r: f64 = 0.0;
        for (j, autre) in bras.iter().enumerate() {
            if i == j {
                continue;
            }
            let wj = autre.profil.emprise_m() * 0.5;
            let d = bras[i].direction;
            let e = autre.direction;
            let cos = (d[0] * e[0] + d[1] * e[1]).clamp(-1.0, 1.0);
            let sin = (1.0 - cos * cos).sqrt();
            let t = if sin < 0.05 {
                if cos < 0.0 { 0.0 } else { wi.max(wj) * 4.0 } // continuité droite / bras confondus
            } else {
                ((wj + wi * cos) / sin).max(0.0) + rayon_bordure
            };
            r = r.max(t);
        }
        reculs[i] = r.min(bras[i].recul_max).clamp(0.0, RECUL_LIMITE_M);
    }
    let mut ordre: Vec<usize> = (0..n).collect();
    ordre.sort_by(|&a, &b| angle(bras[a].direction).total_cmp(&angle(bras[b].direction)));

    let polygone = |demi: &dyn Fn(&ProfilRoute) -> f64| -> Vec<P2> {
        let coins = |i: usize| {
            let d = bras[i].direction;
            let p = [centre[0] + d[0] * reculs[i], centre[1] + d[1] * reculs[i]];
            let w = demi(&bras[i].profil);
            let s = [-d[1], d[0]]; // vers l'angle croissant
            (
                [p[0] - s[0] * w, p[1] - s[1] * w],
                [p[0] + s[0] * w, p[1] + s[1] * w],
            )
        };
        let mut poly = Vec::new();
        for k in 0..n {
            let i = ordre[k];
            let j = ordre[(k + 1) % n];
            let (avant_i, apres_i) = coins(i);
            let (avant_j, _) = coins(j);
            poly.push(avant_i);
            poly.push(apres_i);
            // Arrondi de bordure entre le bras i et le suivant : Bézier quadratique sur l'intersection des bords.
            if let Some(x) =
                crate::polygone::intersection_droites(apres_i, bras[i].direction, avant_j, bras[j].direction)
            {
                let vers_x = [x[0] - centre[0], x[1] - centre[1]];
                let devant = vers_x[0] * bras[i].direction[0] + vers_x[1] * bras[i].direction[1] > -1e-9
                    && vers_x[0] * bras[j].direction[0] + vers_x[1] * bras[j].direction[1] > -1e-9;
                if devant && crate::distance(x, centre) < RECUL_LIMITE_M * 2.0 {
                    for k in 1..PAS_ARRONDI {
                        let t = k as f64 / PAS_ARRONDI as f64;
                        let u = 1.0 - t;
                        poly.push([
                            u * u * apres_i[0] + 2.0 * u * t * x[0] + t * t * avant_j[0],
                            u * u * apres_i[1] + 2.0 * u * t * x[1] + t * t * avant_j[1],
                        ]);
                    }
                }
            }
        }
        if crate::polygone::aire(&poly) < 0.01 || !crate::polygone::est_simple(&poly) {
            Vec::new()
        } else {
            poly
        }
    };
    let chaussee = polygone(&|p: &ProfilRoute| p.chaussee_m * 0.5);
    let emprise = polygone(&|p: &ProfilRoute| p.emprise_m() * 0.5);
    Carrefour {
        reculs,
        chaussee,
        emprise,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClasseRoute, TableProfils, polygone};

    fn bras(d: P2, c: ClasseRoute) -> Bras {
        Bras {
            direction: d,
            profil: *TableProfils::DEFAUT.get(c),
            recul_max: 50.0,
        }
    }

    #[test]
    fn croisement_de_deux_rues() {
        let b = [
            bras([1.0, 0.0], ClasseRoute::Rue),
            bras([0.0, 1.0], ClasseRoute::Rue),
            bras([-1.0, 0.0], ClasseRoute::Rue),
            bras([0.0, -1.0], ClasseRoute::Rue),
        ];
        let c = carrefour([0.0, 0.0], &b, 3.0);
        // Demi-emprise de la rue (7 m) + rayon de bordure.
        assert!(c.reculs.iter().all(|r| (r - 10.0).abs() < 1e-9));
        assert!(polygone::est_simple(&c.emprise));
        assert!(polygone::aire_signee(&c.emprise) > 0.0);
        // L'emprise couvre le centre et touche les bouts de chaque bras.
        assert!(polygone::contient(&c.emprise, [0.0, 0.0]));
        assert!(polygone::aire(&c.chaussee) < polygone::aire(&c.emprise));
    }

    #[test]
    fn impasse_et_continuite_droite() {
        let c = carrefour([0.0, 0.0], &[bras([1.0, 0.0], ClasseRoute::Rue)], 3.0);
        assert_eq!(c.reculs, vec![0.0]);
        assert!(c.emprise.is_empty());
        let c = carrefour(
            [0.0, 0.0],
            &[
                bras([1.0, 0.0], ClasseRoute::Rue),
                bras([-1.0, 0.0], ClasseRoute::Rue),
            ],
            3.0,
        );
        assert_eq!(c.reculs, vec![0.0, 0.0]);
        assert!(c.emprise.is_empty());
    }

    #[test]
    fn t_avec_avenue() {
        let b = [
            bras([1.0, 0.0], ClasseRoute::Avenue),
            bras([-1.0, 0.0], ClasseRoute::Avenue),
            bras([0.0, 1.0], ClasseRoute::Ruelle),
        ];
        let c = carrefour([0.0, 0.0], &b, 3.0);
        // La ruelle recule au moins de la demi-emprise de l'avenue (12 m).
        assert!(c.reculs[2] >= 12.0);
        assert!(polygone::est_simple(&c.emprise));
    }
}
