//! Réseau routier (`reseau.json`) : un graphe de nœuds et de tronçons. Les carrefours sont les nœuds de degré ≥ 3,
//! enregistrés explicitement : le jeu ne déduit rien.

use std::collections::BTreeMap;

use mcf_geo::{ClasseRoute, P2, distance};
use serde::{Deserialize, Serialize};

use crate::ids::Id;
use crate::manifeste::Manifeste;

/// Valeur du champ `format`.
pub const FORMAT: &str = "mcfreseau";
/// Version du format.
pub const VERSION: u32 = 1;
/// Tronçons maximum.
pub const MAX_TRONCONS: usize = 50_000;
/// Nœuds maximum.
pub const MAX_NOEUDS: usize = 100_000;
/// Points intermédiaires maximum par tronçon.
pub const MAX_POINTS: usize = 500;
/// Rayon de courbure maximal (m).
pub const RAYON_MAX_M: f64 = 2000.0;
/// Rayons d'îlot de rond-point autorisés (m).
pub const ROND_POINT_M: (f64, f64) = (4.0, 60.0);
/// Distance minimale entre deux points successifs d'un tracé (m).
pub const ECART_MIN_M: f64 = 0.05;

/// Rayon de courbure posé par défaut aux points cliqués d'une classe (m).
pub const fn rayon_defaut(classe: ClasseRoute) -> f64 {
    match classe {
        ClasseRoute::Sentier => 3.0,
        ClasseRoute::CheminTerre => 8.0,
        ClasseRoute::Route => 40.0,
        ClasseRoute::Ruelle => 6.0,
        ClasseRoute::Rue => 10.0,
        ClasseRoute::Avenue => 20.0,
        ClasseRoute::VoieRapide => 150.0,
        // Courbes larges d'autoroute (le rayon reste réglable par point).
        ClasseRoute::Autoroute => 300.0,
    }
}

/// Un nœud : extrémité, raccord ou carrefour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Noeud {
    /// Position `(x, z)`.
    pub position: P2,
    /// Rond-point : rayon de l'îlot central (m).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rond_point_m: Option<f64>,
}

/// Un tronçon entre deux nœuds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Troncon {
    /// Classe (profil en travers).
    pub classe: ClasseRoute,
    /// Nœud de départ.
    pub de: Id,
    /// Nœud d'arrivée.
    pub vers: Id,
    /// Points de forme entre les deux nœuds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<P2>,
    /// Rayon de courbure à chaque point de forme (0 = angle vif).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rayons: Vec<f64>,
    /// Nom de la voie (affiché sur le plan).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nom: Option<String>,
}

/// Le réseau complet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reseau {
    /// Toujours `mcfreseau`.
    pub format: String,
    /// Version.
    pub version: u32,
    /// Nœuds.
    pub noeuds: BTreeMap<Id, Noeud>,
    /// Tronçons.
    pub troncons: BTreeMap<Id, Troncon>,
}

impl Default for Reseau {
    fn default() -> Self {
        Self {
            format: FORMAT.into(),
            version: VERSION,
            noeuds: BTreeMap::new(),
            troncons: BTreeMap::new(),
        }
    }
}

/// Ce à quoi un point cliqué s'accroche.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Ancre {
    Noeud(Id),
    Troncon,
    Libre,
}

fn projeter(p: P2, a: P2, b: P2) -> (P2, f64) {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l2 = d[0] * d[0] + d[1] * d[1];
    if l2 <= f64::EPSILON {
        return (a, 0.0);
    }
    let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
    ([a[0] + d[0] * t, a[1] + d[1] * t], t)
}

impl Reseau {
    /// Polyligne de contrôle d'un tronçon (nœuds compris) et rayons associés (0 aux extrémités).
    pub fn controle(&self, t: &Troncon) -> Option<(Vec<P2>, Vec<f64>)> {
        let a = self.noeuds.get(&t.de)?.position;
        let b = self.noeuds.get(&t.vers)?.position;
        let mut pts = Vec::with_capacity(t.points.len() + 2);
        pts.push(a);
        pts.extend_from_slice(&t.points);
        pts.push(b);
        let mut r = Vec::with_capacity(pts.len());
        r.push(0.0);
        r.extend((0..t.points.len()).map(|i| t.rayons.get(i).copied().unwrap_or(0.0)));
        r.push(0.0);
        Some((pts, r))
    }

    /// Axe discrétisé d'un tronçon (virages arrondis).
    pub fn axe(&self, id: Id) -> Option<Vec<P2>> {
        let (pts, r) = self.controle(self.troncons.get(&id)?)?;
        Some(mcf_geo::discretiser(&pts, &r, mcf_geo::ECART_CORDE_M))
    }

    /// Tronçons qui touchent un nœud.
    pub fn incidents(&self, noeud: Id) -> Vec<Id> {
        self.troncons
            .iter()
            .filter(|(_, t)| t.de == noeud || t.vers == noeud)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Nombre de bras d'un nœud (une boucle compte deux fois).
    pub fn degre(&self, noeud: Id) -> usize {
        self.troncons
            .values()
            .map(|t| usize::from(t.de == noeud) + usize::from(t.vers == noeud))
            .sum()
    }

    /// Nœud le plus proche à moins de `rayon` mètres.
    pub fn noeud_proche(&self, p: P2, rayon: f64) -> Option<Id> {
        self.noeuds
            .iter()
            .map(|(id, n)| (*id, distance(p, n.position)))
            .filter(|(_, d)| *d <= rayon)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Tronçon dont la polyligne de contrôle passe à moins de `rayon` mètres, avec le point projeté.
    pub fn troncon_proche(&self, p: P2, rayon: f64) -> Option<(Id, P2)> {
        let mut best: Option<(Id, P2, f64)> = None;
        for (id, t) in &self.troncons {
            let Some((pts, _)) = self.controle(t) else {
                continue;
            };
            for s in pts.windows(2) {
                let (q, _) = projeter(p, s[0], s[1]);
                let d = distance(p, q);
                if d <= rayon && best.is_none_or(|b| d < b.2) {
                    best = Some((*id, q, d));
                }
            }
        }
        best.map(|(id, q, _)| (id, q))
    }

    /// Coupe le tronçon qui passe par `p` (à `tolerance` près) et renvoie le nœud créé, ou le nœud existant si `p` en
    /// est un. `None` si aucun tronçon ne passe là.
    pub fn scinder_en(&mut self, p: P2, tolerance: f64) -> Option<Id> {
        if let Some(n) = self.noeud_proche(p, tolerance) {
            return Some(n);
        }
        let (tid, q) = self.troncon_proche(p, tolerance)?;
        let t = self.troncons.get(&tid)?.clone();
        let (pts, _) = self.controle(&t)?;
        // Segment porteur, ou point de forme existant.
        let mut seg = 0;
        let mut best = f64::INFINITY;
        for (i, s) in pts.windows(2).enumerate() {
            let d = distance(q, projeter(q, s[0], s[1]).0);
            if d < best {
                best = d;
                seg = i;
            }
        }
        let interieur_existant = (1..pts.len() - 1).find(|&i| distance(pts[i], q) < 1e-6);
        let noeud = Id::nouveau();
        self.noeuds.insert(
            noeud,
            Noeud {
                position: q,
                rond_point_m: None,
            },
        );
        let rayon = |i: usize| t.rayons.get(i).copied().unwrap_or(0.0);
        let (avant, apres): (Vec<usize>, Vec<usize>) = match interieur_existant {
            // pts[i] = t.points[i - 1] devient le nœud.
            Some(i) => ((0..i - 1).collect(), (i..t.points.len()).collect()),
            None => ((0..seg).collect(), (seg..t.points.len()).collect()),
        };
        let a = Troncon {
            classe: t.classe,
            de: t.de,
            vers: noeud,
            points: avant.iter().map(|&i| t.points[i]).collect(),
            rayons: avant.iter().map(|&i| rayon(i)).collect(),
            nom: t.nom.clone(),
        };
        let b = Troncon {
            classe: t.classe,
            de: noeud,
            vers: t.vers,
            points: apres.iter().map(|&i| t.points[i]).collect(),
            rayons: apres.iter().map(|&i| rayon(i)).collect(),
            nom: t.nom,
        };
        self.troncons.insert(tid, a);
        self.troncons.insert(Id::nouveau(), b);
        Some(noeud)
    }

    /// Trace une nouvelle route le long de `chemin` (points cliqués). Les extrémités et les points proches du
    /// réseau s'accrochent (à `accroche` mètres), chaque croisement crée un carrefour, et les points libres
    /// deviennent des points de forme arrondis au rayon par défaut de la classe. Renvoie les tronçons créés.
    pub fn tracer(
        &mut self,
        classe: ClasseRoute,
        chemin: &[P2],
        accroche: f64,
        manifeste: &Manifeste,
    ) -> Result<Vec<Id>, String> {
        if chemin.len() < 2 {
            return Err("une route demande au moins deux points".into());
        }
        if chemin.iter().any(|p| !manifeste.contient(*p)) {
            return Err("point hors de la carte".into());
        }
        if self.troncons.len() + chemin.len() * 4 > MAX_TRONCONS {
            return Err("réseau plein".into());
        }
        // A. Accroches.
        let mut points: Vec<(P2, Ancre)> = Vec::with_capacity(chemin.len());
        for &p in chemin {
            let ancre = if let Some(n) = self.noeud_proche(p, accroche) {
                (self.noeuds[&n].position, Ancre::Noeud(n))
            } else if let Some((_, q)) = self.troncon_proche(p, accroche) {
                (q, Ancre::Troncon)
            } else {
                (p, Ancre::Libre)
            };
            if points
                .last()
                .is_none_or(|(q, _)| distance(*q, ancre.0) > ECART_MIN_M)
            {
                points.push(ancre);
            }
        }
        if points.len() < 2 {
            return Err("tracé trop court".into());
        }
        // B. Croisements avec le réseau existant (sur les polylignes de contrôle).
        let mut croisements: Vec<(usize, f64, P2)> = Vec::new();
        let existants: Vec<Vec<P2>> = self
            .troncons
            .values()
            .filter_map(|t| self.controle(t).map(|c| c.0))
            .collect();
        for k in 0..points.len() - 1 {
            let (a, b) = (points[k].0, points[k + 1].0);
            for pts in &existants {
                for s in pts.windows(2) {
                    if let Some((x, t)) = mcf_geo::polygone::intersection_segments(a, b, s[0], s[1])
                        && distance(x, a) > accroche.max(ECART_MIN_M)
                        && distance(x, b) > accroche.max(ECART_MIN_M)
                    {
                        croisements.push((k, t, x));
                    }
                }
            }
        }
        // C. Arrêts ordonnés le long du tracé : nœud (accroche, croisement, extrémité) ou point de forme.
        let mut arrets: Vec<(usize, f64, P2, bool, Option<Id>)> = Vec::new();
        let dernier = points.len() - 1;
        for (k, (p, ancre)) in points.iter().enumerate() {
            let est_noeud = k == 0 || k == dernier || !matches!(ancre, Ancre::Libre);
            let id = if let Ancre::Noeud(n) = ancre {
                Some(*n)
            } else {
                None
            };
            arrets.push((k, 0.0, *p, est_noeud, id));
        }
        for (k, t, x) in croisements {
            arrets.push((k, t, x, true, None));
        }
        arrets.sort_by(|a, b| {
            (a.0, a.1)
                .partial_cmp(&(b.0, b.1))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        // D. Résolution des nœuds (coupe des tronçons existants si besoin), puis création des tronçons.
        let mut crees = Vec::new();
        let mut depart: Option<Id> = None;
        let mut forme: Vec<P2> = Vec::new();
        let rayon = rayon_defaut(classe);
        for (_, _, p, est_noeud, id) in arrets {
            if !est_noeud {
                forme.push(p);
                continue;
            }
            let n = match id {
                Some(n) => n,
                None => match self.scinder_en(p, 1e-3) {
                    Some(n) => n,
                    None => {
                        let n = Id::nouveau();
                        self.noeuds.insert(
                            n,
                            Noeud {
                                position: p,
                                rond_point_m: None,
                            },
                        );
                        n
                    }
                },
            };
            if let Some(d) = depart {
                let doublon = forme.is_empty()
                    && (d == n
                        || self.troncons.values().any(|t| {
                            t.points.is_empty() && ((t.de == d && t.vers == n) || (t.de == n && t.vers == d))
                        }));
                if !doublon {
                    let id = Id::nouveau();
                    self.troncons.insert(
                        id,
                        Troncon {
                            classe,
                            de: d,
                            vers: n,
                            rayons: vec![rayon; forme.len()],
                            points: std::mem::take(&mut forme),
                            nom: None,
                        },
                    );
                    crees.push(id);
                }
            }
            forme.clear();
            depart = Some(n);
        }
        Ok(crees)
    }

    /// Supprime un tronçon et les nœuds devenus isolés.
    pub fn supprimer_troncon(&mut self, id: Id) -> bool {
        let Some(t) = self.troncons.remove(&id) else {
            return false;
        };
        for n in [t.de, t.vers] {
            if self.degre(n) == 0 {
                self.noeuds.remove(&n);
            }
        }
        true
    }

    /// Déplace un nœud (refusé hors de la carte).
    pub fn deplacer_noeud(&mut self, id: Id, p: P2, manifeste: &Manifeste) -> bool {
        match self.noeuds.get_mut(&id) {
            Some(n) if manifeste.contient(p) => {
                n.position = p;
                true
            }
            _ => false,
        }
    }

    /// Déplace un point de forme.
    pub fn deplacer_point(&mut self, troncon: Id, index: usize, p: P2, manifeste: &Manifeste) -> bool {
        match self
            .troncons
            .get_mut(&troncon)
            .and_then(|t| t.points.get_mut(index))
        {
            Some(q) if manifeste.contient(p) => {
                *q = p;
                true
            }
            _ => false,
        }
    }

    /// Vérifie le réseau.
    pub fn valider(&self, manifeste: &Manifeste) -> Result<(), String> {
        if self.format != FORMAT || self.version != VERSION {
            return Err(format!("format attendu « {FORMAT} » en version {VERSION}"));
        }
        if self.noeuds.len() > MAX_NOEUDS || self.troncons.len() > MAX_TRONCONS {
            return Err("réseau trop grand".into());
        }
        for (id, n) in &self.noeuds {
            if !manifeste.contient(n.position) {
                return Err(format!("nœud {id} hors de la carte"));
            }
            if let Some(r) = n.rond_point_m
                && !(ROND_POINT_M.0..=ROND_POINT_M.1).contains(&r)
            {
                return Err(format!("nœud {id} : rayon de rond-point hors bornes"));
            }
        }
        for (id, t) in &self.troncons {
            let Some((pts, _)) = self.controle(t) else {
                return Err(format!("tronçon {id} : nœud inconnu"));
            };
            if t.points.len() > MAX_POINTS || t.rayons.len() != t.points.len() {
                return Err(format!("tronçon {id} : points ou rayons incohérents"));
            }
            if t.rayons.iter().any(|r| !(0.0..=RAYON_MAX_M).contains(r)) {
                return Err(format!("tronçon {id} : rayon hors bornes"));
            }
            if t.points.iter().any(|p| !manifeste.contient(*p)) {
                return Err(format!("tronçon {id} : point hors de la carte"));
            }
            if pts.windows(2).any(|s| distance(s[0], s[1]) < ECART_MIN_M) {
                return Err(format!("tronçon {id} : points confondus"));
            }
            if let Some(nom) = &t.nom
                && (nom.chars().count() > 64 || nom.chars().any(char::is_control))
            {
                return Err(format!("tronçon {id} : nom invalide"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m() -> Manifeste {
        Manifeste::nouveau("t", 1024.0)
    }

    #[test]
    fn croisement_cree_un_carrefour() {
        let mut r = Reseau::default();
        r.tracer(ClasseRoute::Rue, &[[-100.0, 0.0], [100.0, 0.0]], 2.0, &m())
            .expect("1");
        r.tracer(ClasseRoute::Rue, &[[0.0, -100.0], [0.0, 100.0]], 2.0, &m())
            .expect("2");
        assert_eq!(r.troncons.len(), 4);
        let centre = r.noeud_proche([0.0, 0.0], 0.01).expect("carrefour");
        assert_eq!(r.degre(centre), 4);
        assert_eq!(r.valider(&m()), Ok(()));
    }

    #[test]
    fn accroche_sur_un_noeud_et_un_troncon() {
        let mut r = Reseau::default();
        r.tracer(ClasseRoute::Rue, &[[-100.0, 0.0], [100.0, 0.0]], 2.0, &m())
            .expect("1");
        // Part de 1 m à côté de l'extrémité, arrive 1,5 m à côté du milieu.
        r.tracer(ClasseRoute::Ruelle, &[[-101.0, 50.0], [-100.0, 1.0]], 2.0, &m())
            .expect("2");
        r.tracer(ClasseRoute::Ruelle, &[[20.0, 60.0], [20.0, 1.5]], 2.0, &m())
            .expect("3");
        let gauche = r.noeud_proche([-100.0, 0.0], 0.01).expect("extrémité");
        assert_eq!(r.degre(gauche), 2);
        let t = r.noeud_proche([20.0, 0.0], 0.01).expect("T");
        assert_eq!(r.degre(t), 3);
        assert_eq!(r.valider(&m()), Ok(()));
    }

    #[test]
    fn points_de_forme_et_scission() {
        let mut r = Reseau::default();
        let ids = r
            .tracer(
                ClasseRoute::Avenue,
                &[[-200.0, 0.0], [0.0, 0.0], [0.0, 200.0]],
                2.0,
                &m(),
            )
            .expect("L");
        assert_eq!(ids.len(), 1);
        let t = &r.troncons[&ids[0]];
        assert_eq!(t.points, vec![[0.0, 0.0]]);
        assert_eq!(t.rayons, vec![20.0]);
        // Couper la branche verticale crée un nœud et garde le point de forme dans la première moitié.
        r.tracer(ClasseRoute::Rue, &[[-50.0, 100.0], [50.0, 100.0]], 2.0, &m())
            .expect("croise");
        assert_eq!(r.troncons.len(), 4);
        assert!(r.troncons.values().any(|t| t.points == vec![[0.0, 0.0]]));
        assert_eq!(r.valider(&m()), Ok(()));
    }

    #[test]
    fn suppression_et_refus() {
        let mut r = Reseau::default();
        let ids = r
            .tracer(ClasseRoute::Rue, &[[0.0, 0.0], [10.0, 0.0]], 2.0, &m())
            .expect("1");
        assert!(
            r.tracer(ClasseRoute::Rue, &[[0.0, 0.0], [9999.0, 0.0]], 2.0, &m())
                .is_err()
        );
        assert!(r.supprimer_troncon(ids[0]));
        assert!(r.noeuds.is_empty());
        let json = serde_json::to_string(&r).expect("json");
        assert!(serde_json::from_str::<Reseau>(&json.replace("\"version\"", "\"x\":0,\"version\"")).is_err());
    }
}
