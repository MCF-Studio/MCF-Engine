//! Géométrie dérivée du réseau : axes discrétisés, reculs aux carrefours, bandes des tronçons, polygones des
//! carrefours et ronds-points. Recalculée à chaque modification ; jamais enregistrée.

use std::collections::BTreeMap;
use std::f64::consts::TAU;

use mcf_geo::{Bande, Bras, Carrefour, ClasseRoute, P2, TableProfils, distance};

use crate::ids::Id;
use crate::reseau::Reseau;

/// Rayon des arrondis de bordure aux carrefours (m).
pub const RAYON_BORDURE_M: f64 = 4.0;
/// Largeur de l'anneau d'un rond-point (m).
pub const ANNEAU_M: f64 = 8.0;

/// Géométrie d'un tronçon.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoTroncon {
    /// Classe.
    pub classe: ClasseRoute,
    /// Axe complet discrétisé (de nœud à nœud).
    pub axe: Vec<P2>,
    /// Longueur de l'axe complet (m).
    pub longueur: f64,
    /// Recul au départ (m le long de l'axe).
    pub recul_debut: f64,
    /// Recul à l'arrivée.
    pub recul_fin: f64,
    /// Axe raccourci des reculs : c'est lui qui porte les bandes.
    pub axe_rogne: Vec<P2>,
    /// Bandes du profil le long de `axe_rogne`.
    pub bandes: Vec<Bande>,
}

impl GeoTroncon {
    /// Abscisse sur l'axe complet du point `i` de `axe_rogne`.
    pub fn abscisses_rognees(&self) -> Vec<f64> {
        let mut d = self.recul_debut;
        let mut v = Vec::with_capacity(self.axe_rogne.len());
        for (i, p) in self.axe_rogne.iter().enumerate() {
            if i > 0 {
                d += distance(self.axe_rogne[i - 1], *p);
            }
            v.push(d);
        }
        v
    }
}

/// Géométrie d'un nœud.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GeoNoeud {
    /// Position.
    pub position: P2,
    /// Polygones de chaussée et d'emprise (vides si inutiles).
    pub carrefour: Carrefour,
    /// Îlot central d'un rond-point.
    pub ilot: Vec<P2>,
}

/// Géométrie du réseau.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GeoReseau {
    /// Par tronçon.
    pub troncons: BTreeMap<Id, GeoTroncon>,
    /// Par nœud.
    pub noeuds: BTreeMap<Id, GeoNoeud>,
}

fn cercle(c: P2, r: f64) -> Vec<P2> {
    let n = ((TAU * r / 1.5).ceil() as usize).clamp(16, 128);
    (0..n)
        .map(|k| {
            let a = TAU * k as f64 / n as f64;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

fn direction(a: P2, b: P2) -> P2 {
    let l = distance(a, b).max(1e-12);
    [(b[0] - a[0]) / l, (b[1] - a[1]) / l]
}

/// Calcule toute la géométrie du réseau.
pub fn calculer(reseau: &Reseau, profils: &TableProfils) -> GeoReseau {
    let mut geo = GeoReseau::default();
    for (id, t) in &reseau.troncons {
        let Some(axe) = reseau.axe(*id) else { continue };
        if axe.len() < 2 {
            continue;
        }
        geo.troncons.insert(
            *id,
            GeoTroncon {
                classe: t.classe,
                longueur: mcf_geo::longueur(&axe),
                axe,
                recul_debut: 0.0,
                recul_fin: 0.0,
                axe_rogne: Vec::new(),
                bandes: Vec::new(),
            },
        );
    }
    // Bras de chaque nœud : (tronçon, départ ?).
    let mut bras: BTreeMap<Id, Vec<(Id, bool)>> = BTreeMap::new();
    for (id, t) in &reseau.troncons {
        if geo.troncons.contains_key(id) {
            bras.entry(t.de).or_default().push((*id, true));
            bras.entry(t.vers).or_default().push((*id, false));
        }
    }
    for (nid, n) in &reseau.noeuds {
        let liste = bras.get(nid).cloned().unwrap_or_default();
        let mut g = GeoNoeud {
            position: n.position,
            ..Default::default()
        };
        if let Some(r_ilot) = n.rond_point_m.filter(|_| !liste.is_empty()) {
            let r_ext = r_ilot + ANNEAU_M;
            let cote = liste
                .iter()
                .map(|(t, _)| profils.get(geo.troncons[t].classe).cote_m)
                .fold(0.0, f64::max);
            g.carrefour = Carrefour {
                reculs: vec![r_ext; liste.len()],
                chaussee: cercle(n.position, r_ext),
                emprise: cercle(n.position, r_ext + cote),
            };
            g.ilot = cercle(n.position, r_ilot);
        } else {
            let b: Vec<Bras> = liste
                .iter()
                .map(|(t, depart)| {
                    let gt = &geo.troncons[t];
                    let a = &gt.axe;
                    let d = if *depart {
                        direction(a[0], a[1])
                    } else {
                        direction(a[a.len() - 1], a[a.len() - 2])
                    };
                    Bras {
                        direction: d,
                        profil: *profils.get(gt.classe),
                        recul_max: gt.longueur * 0.45,
                    }
                })
                .collect();
            g.carrefour = mcf_geo::carrefour(n.position, &b, RAYON_BORDURE_M);
        }
        for ((t, depart), recul) in liste.iter().zip(&g.carrefour.reculs) {
            if let Some(gt) = geo.troncons.get_mut(t) {
                let recul = recul.min(gt.longueur * 0.45);
                if *depart {
                    gt.recul_debut = recul;
                } else {
                    gt.recul_fin = recul;
                }
            }
        }
        geo.noeuds.insert(*nid, g);
    }
    for gt in geo.troncons.values_mut() {
        gt.axe_rogne = mcf_geo::rogner(&gt.axe, gt.recul_debut, gt.recul_fin);
        gt.bandes = mcf_geo::bandes(&gt.axe_rogne, profils.get(gt.classe));
    }
    geo
}

impl GeoReseau {
    /// Tronçon sous le point `p` (dans son emprise), le plus proche de son axe.
    pub fn troncon_sous(&self, p: P2, profils: &TableProfils) -> Option<Id> {
        self.troncons
            .iter()
            .map(|(id, g)| (*id, mcf_geo::distance_polyligne(p, &g.axe), profils.get(g.classe).emprise_m() * 0.5))
            .filter(|(_, d, w)| d <= w)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _, _)| id)
    }

    /// Le point est-il sur une chaussée, un trottoir ou un carrefour ?
    pub fn sur_emprise(&self, p: P2, profils: &TableProfils) -> bool {
        self.troncon_sous(p, profils).is_some()
            || self
                .noeuds
                .values()
                .any(|n| !n.carrefour.emprise.is_empty() && mcf_geo::polygone::contient(&n.carrefour.emprise, p))
    }

    /// Polygones d'emprise de tout le réseau (quads des bandes et carrefours), pour les tests de collision.
    pub fn polygones_emprise(&self) -> Vec<Vec<P2>> {
        let mut v: Vec<Vec<P2>> = Vec::new();
        for g in self.troncons.values() {
            for b in &g.bandes {
                v.extend(b.quads().map(|q| q.to_vec()));
            }
        }
        for n in self.noeuds.values() {
            if !n.carrefour.emprise.is_empty() {
                v.push(n.carrefour.emprise.clone());
            }
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifeste::Manifeste;

    #[test]
    fn croisement_recule_les_rubans() {
        let m = Manifeste::nouveau("t", 1024.0);
        let mut r = Reseau::default();
        r.tracer(ClasseRoute::Rue, &[[-100.0, 0.0], [100.0, 0.0]], 2.0, &m).expect("1");
        r.tracer(ClasseRoute::Rue, &[[0.0, -100.0], [0.0, 100.0]], 2.0, &m).expect("2");
        let g = calculer(&r, &m.profils);
        let centre = r.noeud_proche([0.0, 0.0], 0.01).expect("centre");
        let c = &g.noeuds[&centre];
        assert!(!c.carrefour.emprise.is_empty());
        for gt in g.troncons.values() {
            // Chaque tronçon touche le carrefour d'un côté (recul 7 + 4 m) et une impasse de l'autre (0).
            let reculs = [gt.recul_debut, gt.recul_fin];
            assert!(reculs.contains(&0.0) && reculs.iter().any(|r| (r - 11.0).abs() < 1e-9));
            assert_eq!(gt.bandes.len(), 3);
        }
        assert!(g.sur_emprise([0.0, 0.0], &m.profils));
        assert!(g.sur_emprise([50.0, 6.9], &m.profils));
        assert!(!g.sur_emprise([50.0, 7.1], &m.profils));
    }
}
