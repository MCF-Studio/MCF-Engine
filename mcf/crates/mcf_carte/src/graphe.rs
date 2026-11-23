//! Graphe routier lisible par le jeu et le serveur (GPS, IA) : une arête par tronçon, un nœud par carrefour,
//! raccord ou extrémité. Dérivé du réseau ; ne dépend ni du rendu ni de Bevy.
//!
//! Tout est ordonné par identifiant : deux calculs sur le même réseau donnent le même graphe et les mêmes
//! itinéraires, à l'octet près.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use mcf_geo::{ClasseRoute, P2, TableProfils, distance};
use serde::Serialize;

use crate::ids::Id;
use crate::reseau::Reseau;

/// Un nœud du graphe.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NoeudGraphe {
    /// Identifiant du nœud du réseau.
    pub id: Id,
    /// Position `(x, z)`.
    pub position: P2,
    /// Nombre de bras (1 = extrémité ou impasse, 2 = raccord, ≥ 3 = carrefour).
    pub degre: usize,
}

/// Une arête : un tronçon du réseau, parcourable dans les deux sens.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AreteGraphe {
    /// Identifiant du tronçon (UID de route).
    pub troncon: Id,
    /// Nœud de départ.
    pub de: Id,
    /// Nœud d'arrivée.
    pub vers: Id,
    /// Classe, lue dans les données du réseau.
    pub classe: ClasseRoute,
    /// Emprise façade à façade (m), lue dans les profils du manifeste.
    pub largeur_m: f64,
    /// Longueur de l'axe discrétisé, virages arrondis compris (m).
    pub longueur_m: f64,
    /// Nom de la voie, s'il y en a un.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nom: Option<String>,
}

/// Un itinéraire.
#[derive(Debug, Clone, PartialEq)]
pub struct Itineraire {
    /// Nœuds traversés, départ et arrivée compris.
    pub noeuds: Vec<Id>,
    /// Tronçons empruntés, dans l'ordre.
    pub troncons: Vec<Id>,
    /// Longueur totale (m).
    pub longueur_m: f64,
}

/// Le graphe complet.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct GrapheRoutier {
    /// Nœuds, triés par identifiant.
    pub noeuds: Vec<NoeudGraphe>,
    /// Arêtes, triées par identifiant de tronçon.
    pub aretes: Vec<AreteGraphe>,
}

impl GrapheRoutier {
    /// Construit le graphe d'un réseau (supposé validé).
    pub fn construire(reseau: &Reseau, profils: &TableProfils) -> Self {
        let aretes: Vec<AreteGraphe> = reseau
            .troncons
            .iter()
            .filter_map(|(id, t)| {
                let axe = reseau.axe(*id)?;
                Some(AreteGraphe {
                    troncon: *id,
                    de: t.de,
                    vers: t.vers,
                    classe: t.classe,
                    largeur_m: profils.get(t.classe).emprise_m(),
                    longueur_m: mcf_geo::longueur(&axe),
                    nom: t.nom.clone(),
                })
            })
            .collect();
        let mut degres: BTreeMap<Id, usize> = BTreeMap::new();
        for a in &aretes {
            *degres.entry(a.de).or_default() += 1;
            *degres.entry(a.vers).or_default() += 1;
        }
        let noeuds = reseau
            .noeuds
            .iter()
            .map(|(id, n)| NoeudGraphe {
                id: *id,
                position: n.position,
                degre: degres.get(id).copied().unwrap_or(0),
            })
            .collect();
        Self { noeuds, aretes }
    }

    /// Voisins de chaque nœud : `(voisin, arête)`, dans l'ordre des arêtes.
    fn adjacence(&self) -> BTreeMap<Id, Vec<(Id, usize)>> {
        let mut adj: BTreeMap<Id, Vec<(Id, usize)>> =
            self.noeuds.iter().map(|n| (n.id, Vec::new())).collect();
        for (k, a) in self.aretes.iter().enumerate() {
            adj.entry(a.de).or_default().push((a.vers, k));
            adj.entry(a.vers).or_default().push((a.de, k));
        }
        adj
    }

    /// Nombre de composantes connexes (0 pour un graphe vide).
    pub fn composantes(&self) -> usize {
        let adj = self.adjacence();
        let mut vu: BTreeMap<Id, bool> = adj.keys().map(|k| (*k, false)).collect();
        let mut n = 0;
        for depart in adj.keys() {
            if vu[depart] {
                continue;
            }
            n += 1;
            let mut pile = vec![*depart];
            while let Some(x) = pile.pop() {
                if std::mem::replace(vu.get_mut(&x).expect("nœud connu"), true) {
                    continue;
                }
                pile.extend(adj[&x].iter().map(|(v, _)| *v).filter(|v| !vu[v]));
            }
        }
        n
    }

    /// Le réseau est-il d'un seul tenant ?
    pub fn connexe(&self) -> bool {
        self.composantes() == 1
    }

    /// Nœud le plus proche d'un point, à moins de `rayon` mètres (à égalité, le plus petit identifiant).
    pub fn noeud_proche(&self, p: P2, rayon: f64) -> Option<Id> {
        self.noeuds
            .iter()
            .map(|n| (distance(p, n.position), n.id))
            .filter(|(d, _)| *d <= rayon)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, id)| id)
    }

    /// Plus court chemin (Dijkstra sur la longueur des axes). Déterministe : à longueur égale, le nœud de plus
    /// petit identifiant est traité d'abord, et un prédécesseur n'est remplacé que par un chemin strictement plus
    /// court. `None` si les nœuds ne sont pas reliés.
    pub fn plus_court_chemin(&self, de: Id, vers: Id) -> Option<Itineraire> {
        let adj = self.adjacence();
        if !adj.contains_key(&de) || !adj.contains_key(&vers) {
            return None;
        }
        let mut dist: BTreeMap<Id, f64> = BTreeMap::new();
        let mut pred: BTreeMap<Id, (Id, usize)> = BTreeMap::new();
        // Longueurs positives : l'ordre des bits d'un f64 positif est celui des valeurs.
        let mut file = BinaryHeap::new();
        dist.insert(de, 0.0);
        file.push(Reverse((0.0f64.to_bits(), de)));
        while let Some(Reverse((bits, x))) = file.pop() {
            let d = f64::from_bits(bits);
            if d > dist[&x] {
                continue;
            }
            if x == vers {
                break;
            }
            for (v, k) in &adj[&x] {
                let nd = d + self.aretes[*k].longueur_m;
                if dist.get(v).is_none_or(|ancien| nd < *ancien) {
                    dist.insert(*v, nd);
                    pred.insert(*v, (x, *k));
                    file.push(Reverse((nd.to_bits(), *v)));
                }
            }
        }
        let longueur_m = *dist.get(&vers)?;
        let mut noeuds = vec![vers];
        let mut troncons = Vec::new();
        let mut x = vers;
        while x != de {
            let (p, k) = pred[&x];
            troncons.push(self.aretes[k].troncon);
            noeuds.push(p);
            x = p;
        }
        noeuds.reverse();
        troncons.reverse();
        Some(Itineraire {
            noeuds,
            troncons,
            longueur_m,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifeste::Manifeste;

    #[test]
    fn carre_et_raccourci() {
        let m = Manifeste::nouveau("t", 1024.0);
        let mut r = Reseau::default();
        r.tracer(ClasseRoute::Rue, &[[0.0, 0.0], [100.0, 0.0]], 2.0, &m)
            .expect("1");
        r.tracer(ClasseRoute::Rue, &[[100.0, 0.0], [100.0, 100.0]], 2.0, &m)
            .expect("2");
        r.tracer(ClasseRoute::Avenue, &[[0.0, 0.0], [100.0, 100.0]], 2.0, &m)
            .expect("3");
        let g = GrapheRoutier::construire(&r, &m.profils);
        assert_eq!((g.noeuds.len(), g.aretes.len()), (3, 3));
        assert!(g.connexe());
        let a = g.noeud_proche([0.0, 0.0], 0.1).expect("a");
        let c = g.noeud_proche([100.0, 100.0], 0.1).expect("c");
        let it = g.plus_court_chemin(a, c).expect("chemin");
        assert_eq!(it.troncons.len(), 1, "la diagonale est plus courte");
        assert!((it.longueur_m - 100.0 * 2f64.sqrt()).abs() < 1e-9);
        // Une route isolée fait deux composantes.
        r.tracer(ClasseRoute::Rue, &[[300.0, 300.0], [400.0, 300.0]], 2.0, &m)
            .expect("4");
        let g = GrapheRoutier::construire(&r, &m.profils);
        assert_eq!(g.composantes(), 2);
        let isole = g.noeud_proche([300.0, 300.0], 0.1).expect("isolé");
        assert!(g.plus_court_chemin(a, isole).is_none());
    }
}
