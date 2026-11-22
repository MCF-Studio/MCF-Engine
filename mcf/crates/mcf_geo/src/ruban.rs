//! Rubans continus le long d'un axe : bandes du profil en travers (chaussée, côtés) et leurs maillages. Les bords
//! sont des décalages exacts de l'axe, raccordés en onglet : aucun trou ni chevauchement dans les virages.

use crate::{P2, ProfilRoute};

/// Facteur d'onglet maximal : au-delà (angle de plus de ~151°), le sommet décalé est borné pour ne pas partir à
/// l'infini. Les tracés arrondis n'en approchent jamais.
pub const ONGLET_MAX: f64 = 4.0;

/// Gauche dans le sens de marche, vu de dessus (nord = −Z) : `(dz, −dx)`.
fn gauche(d: P2) -> P2 {
    [d[1], -d[0]]
}

fn direction(a: P2, b: P2) -> Option<P2> {
    let l = crate::distance(a, b);
    (l > 1e-12).then(|| [(b[0] - a[0]) / l, (b[1] - a[1]) / l])
}

/// Décale une polyligne de `d` mètres (positif = à gauche du sens de marche). Le résultat a **autant de points**
/// que l'axe : le point `i` du bord correspond au point `i` de l'axe, ce qui permet d'y reporter l'altitude.
pub fn decaler(axe: &[P2], d: f64) -> Vec<P2> {
    let n = axe.len();
    if n < 2 {
        return axe.to_vec();
    }
    let segments: Vec<Option<P2>> = axe
        .windows(2)
        .map(|s| direction(s[0], s[1]).map(gauche))
        .collect();
    (0..n)
        .map(|i| {
            let avant = if i > 0 { segments[i - 1] } else { None };
            let apres = segments.get(i).copied().flatten();
            let normale = match (avant, apres) {
                (Some(a), Some(b)) => {
                    let m = [a[0] + b[0], a[1] + b[1]];
                    let l = m[0].hypot(m[1]);
                    if l < 1e-12 {
                        a // demi-tour : on garde la normale entrante
                    } else {
                        let m = [m[0] / l, m[1] / l];
                        let facteur = (1.0 / (m[0] * a[0] + m[1] * a[1])).min(ONGLET_MAX);
                        [m[0] * facteur, m[1] * facteur]
                    }
                }
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => [0.0, 0.0],
            };
            [axe[i][0] + normale[0] * d, axe[i][1] + normale[1] * d]
        })
        .collect()
}

/// Rôle d'une bande dans le profil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatureBande {
    /// Chaussée.
    Chaussee,
    /// Côté gauche (trottoir ou accotement).
    CoteGauche,
    /// Côté droit.
    CoteDroit,
}

/// Une bande : deux bords décalés de l'axe, point pour point.
#[derive(Debug, Clone, PartialEq)]
pub struct Bande {
    /// Rôle.
    pub nature: NatureBande,
    /// Bord gauche.
    pub gauche: Vec<P2>,
    /// Bord droit.
    pub droite: Vec<P2>,
}

impl Bande {
    /// Quadrilatères successifs `[gauche_i, droite_i, droite_i+1, gauche_i+1]` (pour le plan 2D ou le picking).
    pub fn quads(&self) -> impl Iterator<Item = [P2; 4]> + '_ {
        (0..self.gauche.len().saturating_sub(1)).map(|i| {
            [
                self.gauche[i],
                self.droite[i],
                self.droite[i + 1],
                self.gauche[i + 1],
            ]
        })
    }
}

/// Bandes du profil le long d'un axe déjà discrétisé : chaussée, puis côtés s'il y en a.
pub fn bandes(axe: &[P2], profil: &ProfilRoute) -> Vec<Bande> {
    let c = profil.chaussee_m * 0.5;
    let e = profil.emprise_m() * 0.5;
    let mut sortie = vec![Bande {
        nature: NatureBande::Chaussee,
        gauche: decaler(axe, c),
        droite: decaler(axe, -c),
    }];
    if profil.cote_m > 0.0 {
        sortie.push(Bande {
            nature: NatureBande::CoteGauche,
            gauche: decaler(axe, e),
            droite: decaler(axe, c),
        });
        sortie.push(Bande {
            nature: NatureBande::CoteDroit,
            gauche: decaler(axe, -c),
            droite: decaler(axe, -e),
        });
    }
    sortie
}

/// Maillage indexé prêt pour un moteur (positions `[x, y, z]`, normales, UV, triangles en sens direct vus de +Y).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Maillage {
    /// Positions.
    pub positions: Vec<[f64; 3]>,
    /// Normales (vers le haut pour une bande plane).
    pub normales: Vec<[f64; 3]>,
    /// UV : `u` en travers (0 à gauche, 1 à droite), `v` = distance le long de l'axe en mètres.
    pub uvs: Vec<[f64; 2]>,
    /// Triangles.
    pub indices: Vec<u32>,
}

/// Maillage d'une bande. `altitudes[i]` est l'altitude du point `i` de l'axe (une seule valeur si la route est
/// plane), `surelevation` la hauteur ajoutée (bordure de trottoir, anti-scintillement). L'altitude ne touche jamais
/// `x` ni `z`.
pub fn maillage_bande(bande: &Bande, altitudes: &[f64], surelevation: f64) -> Maillage {
    let n = bande.gauche.len().min(bande.droite.len());
    let mut m = Maillage::default();
    if n < 2 {
        return m;
    }
    let altitude = |i: usize| altitudes.get(i).or(altitudes.last()).copied().unwrap_or(0.0) + surelevation;
    let mut v = 0.0;
    for i in 0..n {
        if i > 0 {
            let milieu = |k: usize| {
                [
                    (bande.gauche[k][0] + bande.droite[k][0]) * 0.5,
                    (bande.gauche[k][1] + bande.droite[k][1]) * 0.5,
                ]
            };
            v += crate::distance(milieu(i - 1), milieu(i));
        }
        let y = altitude(i);
        m.positions.push([bande.gauche[i][0], y, bande.gauche[i][1]]);
        m.positions.push([bande.droite[i][0], y, bande.droite[i][1]]);
        m.uvs.push([0.0, v]);
        m.uvs.push([1.0, v]);
    }
    for i in 0..n - 1 {
        let g0 = (2 * i) as u32;
        let (d0, g1, d1) = (g0 + 1, g0 + 2, g0 + 3);
        m.indices.extend_from_slice(&[g0, d0, g1, d0, d1, g1]);
    }
    m.normales = normales(&m.positions, &m.indices);
    m
}

fn normales(positions: &[[f64; 3]], indices: &[u32]) -> Vec<[f64; 3]> {
    let mut acc = vec![[0.0f64; 3]; positions.len()];
    for t in indices.as_chunks::<3>().0 {
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| positions[i as usize]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let w = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * w[2] - u[2] * w[1],
            u[2] * w[0] - u[0] * w[2],
            u[0] * w[1] - u[1] * w[0],
        ];
        for &i in t {
            for k in 0..3 {
                acc[i as usize][k] += n[k];
            }
        }
    }
    acc.into_iter()
        .map(|n| {
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if l > 1e-12 {
                [n[0] / l, n[1] / l, n[2] / l]
            } else {
                [0.0, 1.0, 0.0]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ClasseRoute, ECART_CORDE_M, TableProfils, discretiser, distance_polyligne, distance_segment,
    };

    fn route_en_l() -> Vec<P2> {
        discretiser(
            &[[0.0, 0.0], [60.0, 0.0], [60.0, -60.0]],
            &[0.0, 15.0, 0.0],
            ECART_CORDE_M,
        )
    }

    #[test]
    fn la_gauche_est_vue_de_dessus() {
        // Vers +X (vers l'est), la gauche est le nord, donc −Z.
        assert_eq!(
            decaler(&[[0.0, 0.0], [10.0, 0.0]], 2.0),
            vec![[0.0, -2.0], [10.0, -2.0]]
        );
    }

    #[test]
    fn bords_a_distance_exacte_de_l_axe() {
        let axe = route_en_l();
        let profil = TableProfils::DEFAUT.get(ClasseRoute::Rue);
        for bande in bandes(&axe, profil) {
            let (dg, dd) = match bande.nature {
                NatureBande::Chaussee => (3.5, 3.5),
                NatureBande::CoteGauche => (7.0, 3.5),
                NatureBande::CoteDroit => (3.5, 7.0),
            };
            for (bord, attendu) in [(&bande.gauche, dg), (&bande.droite, dd)] {
                assert_eq!(bord.len(), axe.len());
                // Chaque segment de bord est parallèle au segment d'axe correspondant, à la bonne distance.
                for i in 0..axe.len() - 1 {
                    let milieu = [
                        (bord[i][0] + bord[i + 1][0]) * 0.5,
                        (bord[i][1] + bord[i + 1][1]) * 0.5,
                    ];
                    let d = distance_segment(milieu, axe[i], axe[i + 1]);
                    assert!(
                        (d - attendu).abs() < 1e-6,
                        "segment {i} : {d} au lieu de {attendu}"
                    );
                }
            }
        }
    }

    #[test]
    fn aucun_trou_ni_chevauchement_entre_bandes() {
        let axe = route_en_l();
        let b = bandes(&axe, TableProfils::DEFAUT.get(ClasseRoute::Avenue));
        // Les bandes partagent exactement leurs bords : continuité garantie.
        assert_eq!(b[0].gauche, b[1].droite);
        assert_eq!(b[0].droite, b[2].gauche);
        // Et chaque bande est continue le long de l'axe : le quad i finit où commence le quad i+1.
        let quads: Vec<_> = b[0].quads().collect();
        for q in quads.windows(2) {
            assert_eq!(q[0][3], q[1][0]);
            assert_eq!(q[0][2], q[1][1]);
        }
    }

    #[test]
    fn triangles_tournes_vers_le_haut() {
        let axe = route_en_l();
        let bande = &bandes(&axe, TableProfils::DEFAUT.get(ClasseRoute::Route))[0];
        let m = maillage_bande(bande, &[0.0], 0.0);
        for t in m.indices.as_chunks::<3>().0 {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| m.positions[i as usize]);
            let ny = (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]);
            assert!(ny > 0.0, "triangle retourné : {t:?}");
        }
        assert!(m.normales.iter().all(|n| (n[1] - 1.0).abs() < 1e-9));
    }

    /// Contrat d'exactitude (spec §2.5) : la façade d'un bâtiment posé sur le bord d'emprise est à 0 mm du maillage
    /// 3D, quelle que soit l'altitude, et la distance mesurée en 3D projeté égale celle du plan à 1 mm près.
    #[test]
    fn exactitude_plan_3d() {
        let axe = route_en_l();
        let profil = TableProfils::DEFAUT.get(ClasseRoute::Rue);
        let b = bandes(&axe, profil);
        let bord_plan = &b[1].gauche; // bord d'emprise gauche, tel que dessiné en vue Plan
        let altitudes: Vec<f64> = (0..axe.len()).map(|i| 3.0 + i as f64 * 0.37).collect();
        let m = maillage_bande(&b[1], &altitudes, 0.15);
        let bord_3d: Vec<P2> = m.positions.iter().step_by(2).map(|p| [p[0], p[2]]).collect();
        assert_eq!(&bord_3d, bord_plan, "l'altitude a déplacé le plan");
        // Façades posées contre le bord, sur la partie droite et dans le virage.
        for facade in [
            bord_plan[0],
            bord_plan[bord_plan.len() / 2],
            bord_plan[bord_plan.len() - 1],
        ] {
            let plan = distance_polyligne(facade, bord_plan);
            let trois_d = distance_polyligne(facade, &bord_3d);
            assert!(plan < 1e-3 && (plan - trois_d).abs() < 1e-3);
            // Et la façade est à une demi-emprise de l'axe (7 m pour une rue). Dans un virage, l'onglet place les
            // sommets extérieurs un peu au-delà (d / cos(δ/2), quelques cm) : la référence reste le bord d'emprise,
            // identique dans les deux vues.
            assert!((distance_polyligne(facade, &axe) - profil.emprise_m() * 0.5).abs() < 0.03);
        }
    }
}
