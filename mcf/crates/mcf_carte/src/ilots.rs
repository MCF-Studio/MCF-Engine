//! Îlots : faces fermées du graphe routier (les impasses ne ferment rien), puis retrait de la demi-emprise de chaque
//! rue qui les borde. Dérivés, jamais enregistrés.

use std::collections::{BTreeMap, BTreeSet};

use mcf_geo::{P2, TableProfils, polygone};

use crate::geometrie::GeoReseau;
use crate::ids::{Id, melanger};
use crate::reseau::Reseau;

/// Un îlot.
#[derive(Debug, Clone, PartialEq)]
pub struct Ilot {
    /// Clé stable tant que les mêmes tronçons le bordent.
    pub cle: u64,
    /// Contour sur les axes des rues (sens trigonométrique).
    pub contour: Vec<P2>,
    /// Tronçon de chaque arête du contour (arête `i` = `contour[i]` → `contour[i + 1]`).
    pub aretes: Vec<Id>,
    /// Surface constructible : contour moins les emprises (`None` si l'îlot est trop petit).
    pub interieur: Option<Vec<P2>>,
}

/// Aire minimale d'un îlot (m²).
pub const AIRE_MIN_M2: f64 = 50.0;

/// Calcule les îlots du réseau.
pub fn calculer(reseau: &Reseau, geo: &GeoReseau, profils: &TableProfils) -> Vec<Ilot> {
    // 1. Retire les impasses de proche en proche.
    let mut actifs: BTreeSet<Id> = geo.troncons.keys().copied().collect();
    loop {
        let mut degre: BTreeMap<Id, usize> = BTreeMap::new();
        for id in &actifs {
            let t = &reseau.troncons[id];
            *degre.entry(t.de).or_default() += 1;
            *degre.entry(t.vers).or_default() += 1;
        }
        let avant = actifs.len();
        actifs.retain(|id| {
            let t = &reseau.troncons[id];
            t.de == t.vers || (degre[&t.de] > 1 && degre[&t.vers] > 1)
        });
        if actifs.len() == avant {
            break;
        }
    }
    // 2. Demi-arêtes : (tronçon, sens) partant de chaque nœud, triées par angle.
    let mut sortantes: BTreeMap<Id, Vec<(f64, Id, bool)>> = BTreeMap::new();
    for id in &actifs {
        let t = &reseau.troncons[id];
        let axe = &geo.troncons[id].axe;
        let n = axe.len();
        let ang = |a: P2, b: P2| (b[1] - a[1]).atan2(b[0] - a[0]);
        sortantes
            .entry(t.de)
            .or_default()
            .push((ang(axe[0], axe[1]), *id, true));
        sortantes
            .entry(t.vers)
            .or_default()
            .push((ang(axe[n - 1], axe[n - 2]), *id, false));
    }
    for v in sortantes.values_mut() {
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
    // 3. Parcours des faces en tournant toujours à gauche : faces bornées en sens trigonométrique.
    let mut vues: BTreeSet<(Id, bool)> = BTreeSet::new();
    let mut ilots = Vec::new();
    for depart in sortantes
        .values()
        .flatten()
        .map(|(_, id, s)| (*id, *s))
        .collect::<Vec<_>>()
    {
        if vues.contains(&depart) {
            continue;
        }
        let mut contour = Vec::new();
        let mut aretes = Vec::new();
        let mut h = depart;
        let mut garde = 0;
        loop {
            vues.insert(h);
            let t = &reseau.troncons[&h.0];
            let axe = &geo.troncons[&h.0].axe;
            let pts: Vec<P2> = if h.1 {
                axe.clone()
            } else {
                axe.iter().rev().copied().collect()
            };
            for p in &pts[..pts.len() - 1] {
                contour.push(*p);
                aretes.push(h.0);
            }
            let arrivee = if h.1 { t.vers } else { t.de };
            let liste = &sortantes[&arrivee];
            let jumelle = (h.0, !h.1);
            let k = liste
                .iter()
                .position(|(_, id, s)| (*id, *s) == jumelle)
                .unwrap_or(0);
            let suivante = liste[(k + liste.len() - 1) % liste.len()];
            h = (suivante.1, suivante.2);
            garde += 1;
            if h == depart || garde > 100_000 {
                break;
            }
        }
        if h != depart || contour.len() < 3 || polygone::aire_signee(&contour) < AIRE_MIN_M2 {
            continue;
        }
        let mut ids: Vec<u64> = aretes.iter().map(|i| i.0).collect();
        ids.sort_unstable();
        ids.dedup();
        let mut cle = 0x51_7C_C1_B7_27_22_0A_95u64;
        for i in ids {
            cle ^= i;
            cle = melanger(&mut cle);
        }
        let retraits: Vec<f64> = aretes
            .iter()
            .map(|t| profils.get(geo.troncons[t].classe).emprise_m() * 0.5)
            .collect();
        let interieur = polygone::retrait(&contour, &retraits).filter(|p| polygone::aire(p) >= AIRE_MIN_M2);
        ilots.push(Ilot {
            cle,
            contour,
            aretes,
            interieur,
        });
    }
    ilots
}

/// Îlot qui contient un point.
pub fn ilot_en(ilots: &[Ilot], p: P2) -> Option<&Ilot> {
    ilots
        .iter()
        .filter(|i| polygone::contient(&i.contour, p))
        .min_by(|a, b| polygone::aire(&a.contour).total_cmp(&polygone::aire(&b.contour)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometrie;
    use crate::manifeste::Manifeste;
    use mcf_geo::ClasseRoute;

    #[test]
    fn grille_de_quatre_ilots() {
        let m = Manifeste::nouveau("t", 1024.0);
        let mut r = Reseau::default();
        for k in [-100.0, 0.0, 100.0] {
            r.tracer(ClasseRoute::Rue, &[[-100.0, k], [100.0, k]], 2.0, &m)
                .expect("h");
            r.tracer(ClasseRoute::Rue, &[[k, -100.0], [k, 100.0]], 2.0, &m)
                .expect("v");
        }
        // Une impasse dans un îlot ne le coupe pas.
        r.tracer(ClasseRoute::Ruelle, &[[50.0, 0.0], [50.0, 50.0]], 2.0, &m)
            .expect("impasse");
        let geo = geometrie::calculer(&r, &m.profils);
        let ilots = calculer(&r, &geo, &m.profils);
        assert_eq!(ilots.len(), 4);
        for i in &ilots {
            assert!((polygone::aire(&i.contour) - 10_000.0).abs() < 1.0);
            // 100 m moins 2 × 7 m de demi-emprise de rue.
            let int = i.interieur.as_ref().expect("intérieur");
            assert!((polygone::aire(int) - 86.0 * 86.0).abs() < 1.0);
        }
        assert!(ilot_en(&ilots, [50.0, 50.0]).is_some());
        assert!(ilot_en(&ilots, [500.0, 50.0]).is_none());
    }
}
