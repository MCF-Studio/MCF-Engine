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
/// Lignes électriques maximum.
pub const MAX_LIGNES_ELECTRIQUES: usize = 10_000;
/// Couleur des lignes électriques (sRGB) : orange, distinct des routes et de la sélection (jaune).
pub const COULEUR_ELECTRIQUE: [u8; 3] = [235, 95, 20];

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

/// Ligne électrique : un tracé dessiné sur le plan et en 3D, rien d'autre. Aucun pylône, aucun terrassement, aucun
/// effet sur les rubans, les îlots ou le graphe routier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LigneElectrique {
    /// Points du tracé `(x, z)`, au moins deux.
    pub points: Vec<P2>,
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
    /// Réseau électrique (tracé seulement). Absent des fichiers écrits avant son ajout : vide par défaut.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lignes_electriques: BTreeMap<Id, LigneElectrique>,
}

impl Default for Reseau {
    fn default() -> Self {
        Self {
            format: FORMAT.into(),
            version: VERSION,
            noeuds: BTreeMap::new(),
            troncons: BTreeMap::new(),
            lignes_electriques: BTreeMap::new(),
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
        self.scinder_troncon(tid, q)
    }

    /// Coupe le tronçon `tid` au point `q` (sur sa polyligne de contrôle) et renvoie le nœud créé.
    fn scinder_troncon(&mut self, tid: Id, q: P2) -> Option<Id> {
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

    /// Trace une ligne électrique le long de `chemin` (points cliqués). Les points confondus sont ignorés.
    pub fn tracer_ligne_electrique(&mut self, chemin: &[P2], manifeste: &Manifeste) -> Result<Id, String> {
        if chemin.iter().any(|p| !manifeste.contient(*p)) {
            return Err("point hors de la carte".into());
        }
        if self.lignes_electriques.len() >= MAX_LIGNES_ELECTRIQUES {
            return Err("réseau électrique plein".into());
        }
        let mut points: Vec<P2> = Vec::with_capacity(chemin.len());
        for &p in chemin {
            if points.last().is_none_or(|q| distance(*q, p) > ECART_MIN_M) {
                points.push(p);
            }
        }
        if points.len() < 2 {
            return Err("une ligne électrique demande au moins deux points distincts".into());
        }
        if points.len() > MAX_POINTS + 2 {
            return Err("ligne électrique trop longue (trop de points)".into());
        }
        let id = Id::nouveau();
        self.lignes_electriques.insert(id, LigneElectrique { points });
        Ok(id)
    }

    /// Ligne électrique qui passe à moins de `marge` mètres de `p` (la plus proche).
    pub fn ligne_electrique_proche(&self, p: P2, marge: f64) -> Option<Id> {
        self.lignes_electriques
            .iter()
            .filter_map(|(id, l)| {
                l.points
                    .windows(2)
                    .map(|s| mcf_geo::distance_segment(p, s[0], s[1]))
                    .min_by(f64::total_cmp)
                    .map(|d| (*id, d))
            })
            .filter(|(_, d)| *d <= marge)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Fusionne le nœud `n` dans `cible` : ses tronçons repartent de `cible`, `n` disparaît. Les tronçons devenus
    /// vides (boucle sans point de forme, doublon direct d'un autre) sont retirés.
    pub fn fusionner_noeuds(&mut self, n: Id, cible: Id) {
        if n == cible || !self.noeuds.contains_key(&cible) {
            return;
        }
        for t in self.troncons.values_mut() {
            if t.de == n {
                t.de = cible;
            }
            if t.vers == n {
                t.vers = cible;
            }
        }
        self.noeuds.remove(&n);
        let mut vus: Vec<(Id, Id)> = Vec::new();
        let mut retirer = Vec::new();
        for (id, t) in &self.troncons {
            if !t.points.is_empty() || (t.de != cible && t.vers != cible) {
                continue;
            }
            let paire = if t.de < t.vers {
                (t.de, t.vers)
            } else {
                (t.vers, t.de)
            };
            if t.de == t.vers || vus.contains(&paire) {
                retirer.push(*id);
            } else {
                vus.push(paire);
            }
        }
        for id in retirer {
            self.supprimer_troncon(id);
        }
    }

    /// Raccorde le nœud `n`, qui vient d'être déplacé, au réseau voisin (même règle que l'accroche du tracé) : fusion
    /// avec un autre nœud à moins de `accroche` mètres, sinon coupe du tronçon le plus proche (hors des tronçons de
    /// `n`) et fusion avec le point de coupe. Renvoie le nœud qui reste.
    pub fn raccorder_noeud(&mut self, n: Id, accroche: f64) -> Id {
        let Some(p) = self.noeuds.get(&n).map(|x| x.position) else {
            return n;
        };
        if accroche <= 0.0 {
            return n;
        }
        let voisin = self
            .noeuds
            .iter()
            .filter(|(id, _)| **id != n)
            .map(|(id, x)| (*id, distance(p, x.position)))
            .filter(|(_, d)| *d <= accroche)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id);
        let cible = match voisin {
            Some(m) => m,
            None => {
                let mut best: Option<(Id, P2, f64)> = None;
                for (id, t) in &self.troncons {
                    if t.de == n || t.vers == n {
                        continue;
                    }
                    let Some((pts, _)) = self.controle(t) else {
                        continue;
                    };
                    for s in pts.windows(2) {
                        let (q, _) = projeter(p, s[0], s[1]);
                        let d = distance(p, q);
                        if d <= accroche && best.is_none_or(|b| d < b.2) {
                            best = Some((*id, q, d));
                        }
                    }
                }
                let Some((tid, q, _)) = best else {
                    return n;
                };
                match self.scinder_troncon(tid, q) {
                    Some(m) => m,
                    None => return n,
                }
            }
        };
        self.fusionner_noeuds(n, cible);
        cible
    }

    /// Crée les carrefours là où le tronçon `id` (qui vient d'être modifié) croise d'autres tronçons : il est retracé
    /// à l'identique (mêmes points, rayons et nom) et les tronçons croisés sont coupés. Renvoie les tronçons qui le
    /// remplacent (`[id]` s'il ne croise rien).
    pub fn couper_croisements(&mut self, id: Id, manifeste: &Manifeste) -> Result<Vec<Id>, String> {
        let Some(t) = self.troncons.get(&id).cloned() else {
            return Ok(Vec::new());
        };
        let Some((pts, _)) = self.controle(&t) else {
            return Ok(vec![id]);
        };
        let croise = self
            .troncons
            .iter()
            .filter(|(k, _)| **k != id)
            .filter_map(|(_, u)| self.controle(u))
            .any(|(q, _)| {
                pts.windows(2).any(|s| {
                    q.windows(2).any(|r| {
                        mcf_geo::polygone::intersection_segments(s[0], s[1], r[0], r[1]).is_some_and(
                            |(x, _)| {
                                [s[0], s[1], r[0], r[1]]
                                    .iter()
                                    .all(|e| distance(*e, x) > ECART_MIN_M)
                            },
                        )
                    })
                })
            });
        if !croise {
            return Ok(vec![id]);
        }
        self.troncons.remove(&id);
        // Accroche minimale : les extrémités retrouvent exactement leurs nœuds, les points de forme restent libres.
        let crees = self.tracer(t.classe, &pts, 1e-3, manifeste)?;
        for c in &crees {
            let Some(u) = self.troncons.get_mut(c) else {
                continue;
            };
            u.nom.clone_from(&t.nom);
            for (i, p) in u.points.iter().enumerate() {
                if let Some(k) = t.points.iter().position(|q| distance(*q, *p) < 1e-9) {
                    u.rayons[i] = t.rayons.get(k).copied().unwrap_or(0.0);
                }
            }
        }
        Ok(crees)
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
        if self.lignes_electriques.len() > MAX_LIGNES_ELECTRIQUES {
            return Err("réseau électrique trop grand".into());
        }
        for (id, l) in &self.lignes_electriques {
            if l.points.len() < 2 || l.points.len() > MAX_POINTS + 2 {
                return Err(format!("ligne électrique {id} : nombre de points hors bornes"));
            }
            if l.points.iter().any(|p| !manifeste.contient(*p)) {
                return Err(format!("ligne électrique {id} : point hors de la carte"));
            }
            if l.points.windows(2).any(|s| distance(s[0], s[1]) < ECART_MIN_M) {
                return Err(format!("ligne électrique {id} : points confondus"));
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
    fn ligne_electrique_sans_effet_sur_les_routes() {
        let mut r = Reseau::default();
        r.tracer(ClasseRoute::Rue, &[[-100.0, 0.0], [100.0, 0.0]], 2.0, &m())
            .expect("route");
        let avant = (r.noeuds.clone(), r.troncons.clone());
        // Elle croise la route : aucun carrefour, aucun nœud.
        let id = r
            .tracer_ligne_electrique(&[[0.0, -50.0], [0.0, -50.0], [0.0, 50.0], [40.0, 80.0]], &m())
            .expect("ligne");
        assert_eq!((r.noeuds.clone(), r.troncons.clone()), avant);
        assert_eq!(r.lignes_electriques[&id].points.len(), 3, "point confondu ignoré");
        assert_eq!(r.ligne_electrique_proche([1.0, 10.0], 2.0), Some(id));
        assert_eq!(r.ligne_electrique_proche([10.0, 10.0], 2.0), None);
        assert!(r.tracer_ligne_electrique(&[[0.0, 0.0]], &m()).is_err());
        assert!(
            r.tracer_ligne_electrique(&[[0.0, 0.0], [9_000.0, 0.0]], &m())
                .is_err()
        );
        assert_eq!(r.valider(&m()), Ok(()));
        // Aller-retour JSON ; un ancien fichier sans lignes reste lisible.
        let texte = serde_json::to_string(&r).expect("json");
        assert_eq!(serde_json::from_str::<Reseau>(&texte).expect("relu"), r);
        let ancien = r#"{"format":"mcfreseau","version":1,"noeuds":{},"troncons":{}}"#;
        assert!(
            serde_json::from_str::<Reseau>(ancien)
                .expect("ancien")
                .lignes_electriques
                .is_empty()
        );
        // Ligne invalide refusée à la validation (fichier non fiable).
        r.lignes_electriques
            .get_mut(&id)
            .expect("ligne")
            .points
            .truncate(1);
        assert!(r.valider(&m()).is_err());
    }

    #[test]
    fn point_de_forme_deplace_en_travers_cree_les_carrefours() {
        let mut r = Reseau::default();
        r.tracer(ClasseRoute::Rue, &[[-100.0, 0.0], [100.0, 0.0]], 2.0, &m())
            .expect("1");
        // Route en V au-dessus de la première, sans la toucher.
        let v = r
            .tracer(
                ClasseRoute::Route,
                &[[-80.0, 60.0], [0.0, 20.0], [80.0, 60.0]],
                2.0,
                &m(),
            )
            .expect("2");
        assert_eq!(v.len(), 1);
        r.troncons.get_mut(&v[0]).expect("V").nom = Some("Chemin".into());
        // On tire la pointe du V sous la première route : deux croisements.
        assert!(r.deplacer_point(v[0], 0, [0.0, -40.0], &m()));
        let crees = r.couper_croisements(v[0], &m()).expect("coupe");
        assert_eq!(crees.len(), 3, "le V est coupé en trois");
        for x in [-32.0, 32.0] {
            let n = r.noeud_proche([x, 0.0], 0.01).expect("carrefour");
            assert_eq!(r.degre(n), 4);
        }
        // Le nom et le rayon de la pointe sont conservés.
        assert!(
            crees
                .iter()
                .all(|c| r.troncons[c].nom.as_deref() == Some("Chemin"))
        );
        let milieu = crees
            .iter()
            .find(|c| !r.troncons[*c].points.is_empty())
            .expect("pointe");
        assert_eq!(r.troncons[milieu].rayons, vec![rayon_defaut(ClasseRoute::Route)]);
        assert_eq!(r.valider(&m()), Ok(()));
    }

    #[test]
    fn extremite_deplacee_sur_une_route_ou_un_noeud_fusionne() {
        let mut r = Reseau::default();
        r.tracer(ClasseRoute::Rue, &[[-100.0, 0.0], [100.0, 0.0]], 2.0, &m())
            .expect("1");
        let t = r
            .tracer(ClasseRoute::Ruelle, &[[20.0, 80.0], [20.0, 30.0]], 2.0, &m())
            .expect("2")[0];
        // Extrémité lâchée à 1 m de la première route : elle coupe la route et forme un T.
        let (haut, bout) = (r.troncons[&t].de, r.troncons[&t].vers);
        assert!(r.deplacer_noeud(bout, [20.0, 1.0], &m()));
        let n = r.raccorder_noeud(bout, 2.0);
        assert_ne!(n, bout);
        assert_eq!(r.noeuds[&n].position, [20.0, 0.0]);
        assert_eq!(r.degre(n), 3);
        // Autre extrémité lâchée près du bout d'une troisième route : fusion des deux nœuds.
        r.tracer(ClasseRoute::Ruelle, &[[-100.0, 50.0], [-60.0, 50.0]], 2.0, &m())
            .expect("3");
        let gauche = r.noeud_proche([-100.0, 50.0], 0.01).expect("gauche");
        assert!(r.deplacer_noeud(haut, [-99.0, 49.5], &m()));
        assert_eq!(r.raccorder_noeud(haut, 2.0), gauche);
        assert_eq!(r.degre(gauche), 2);
        assert!(!r.noeuds.contains_key(&haut));
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
