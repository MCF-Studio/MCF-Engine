//! Relevé assisté sur un calque de référence calé (todo « Import de cartes 2D », étape 2).
//!
//! L'auteur clique une couleur du calque (la mer, une tache urbaine, une route) ; l'analyse **propose** des éléments
//! aux conventions de l'éditeur, que l'auteur valide ou rejette un par un. Rien n'est écrit dans la carte ici :
//! ce module ne fait que calculer des propositions (le calque n'est jamais une source de vérité, spec §8).
//!
//! Déroulé :
//! 1. [`Grille`] : la partie de l'image posée **sur la carte** (le reste, légende comprise, est ignoré), réduite si
//!    besoin à [`MAX_PIXELS_ANALYSE`] pixels, couleurs converties en CIE Lab (écart perçu ≈ distance).
//! 2. Masque des pixels proches de la couleur cliquée (ΔE 1976), fermeture morphologique (comble les textes et
//!    traits fins qui coupent une tache), composantes connexes.
//! 3. Selon la sorte :
//!    - **eau** : masque gardé tel quel (les îles restent, les petits trous sont comblés), bords pour l'affichage ;
//!      la carte creuse ensuite le relief sous le niveau d'eau (`Carte::creuser_eau`) ;
//!    - **zone** : contour extérieur simplifié en polygone simple (zone libre à affecter) ;
//!    - **routes** : squelette du masque, découpé en branches entre carrefours, chaque branche simplifiée en tracé.
//!
//! Coûts bornés : grille plafonnée, nombre de propositions plafonné, sommets plafonnés aux limites des fichiers.

use std::collections::{HashMap, VecDeque};

use mcf_geo::{P2, polygone};

use crate::manifeste::Manifeste;
use crate::references::{ImageRgba, Reference};

/// Pixels analysés au plus (l'image est moyennée par blocs au-delà).
pub const MAX_PIXELS_ANALYSE: u64 = 4 * 1024 * 1024;
/// Propositions au plus par analyse (les plus grandes sont gardées).
pub const MAX_PROPOSITIONS: usize = 400;
/// Sommets au plus d'une zone proposée (limite de `quartiers.json`).
pub const MAX_SOMMETS_ZONE: usize = crate::quartiers::MAX_SOMMETS;
/// Points au plus d'un tracé proposé (sous la limite de points par tronçon du réseau).
pub const MAX_POINTS_ROUTE: usize = 200;
/// Côté maximal de la fermeture morphologique (pixels de grille).
pub const MAX_COMBLER_PX: u32 = 8;

/// Ce qu'on relève.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sorte {
    /// Mer, lac, rivière large : creuse le relief sous le niveau d'eau.
    Eau,
    /// Tache (urbaine, industrielle…) : zone libre à affecter.
    Zone,
    /// Réseau d'une couleur de route : tracés à valider.
    Routes,
}

impl Sorte {
    /// Toutes les sortes.
    pub const TOUTES: [Self; 3] = [Self::Eau, Self::Zone, Self::Routes];

    /// Nom affiché.
    pub const fn nom(self) -> &'static str {
        match self {
            Self::Eau => "Eau / côte",
            Self::Zone => "Taches (quartiers)",
            Self::Routes => "Routes",
        }
    }

    /// Réglages conseillés.
    pub const fn reglages(self) -> Reglages {
        match self {
            Self::Eau => Reglages {
                tolerance: 14.0,
                combler_px: 2,
                toutes: false,
                aire_min_m2: 20_000.0,
                longueur_min_m: 0.0,
            },
            Self::Zone => Reglages {
                tolerance: 12.0,
                combler_px: 3,
                toutes: true,
                aire_min_m2: 20_000.0,
                longueur_min_m: 0.0,
            },
            Self::Routes => Reglages {
                tolerance: 18.0,
                combler_px: 1,
                toutes: false,
                aire_min_m2: 0.0,
                longueur_min_m: 60.0,
            },
        }
    }
}

/// Réglages d'une analyse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reglages {
    /// Écart de couleur accepté (ΔE CIE 1976 : ~2 à peine visible, ~10 net, ~40 très différent).
    pub tolerance: f32,
    /// Rayon de la fermeture (pixels de grille) : comble textes et traits fins qui coupent une tache.
    pub combler_px: u32,
    /// Toutes les taches de cette couleur (sinon seulement celle sous le clic).
    pub toutes: bool,
    /// Eau et zones : taches plus petites ignorées ; eau : trous (îles) plus petits comblés.
    pub aire_min_m2: f64,
    /// Routes : branches plus courtes ignorées (et ergots supprimés).
    pub longueur_min_m: f64,
}

/// Une proposition, à valider ou rejeter par l'auteur.
#[derive(Debug, Clone, PartialEq)]
pub enum Proposition {
    /// Plan d'eau.
    Eau(Eau),
    /// Zone libre (polygone simple, sens trigonométrique).
    Zone {
        /// Contour.
        polygone: Vec<P2>,
        /// Aire (m²).
        aire_m2: f64,
    },
    /// Tracé de route (points de passage).
    Route {
        /// Points.
        trace: Vec<P2>,
        /// Longueur (m).
        longueur_m: f64,
    },
}

impl Proposition {
    /// Description courte.
    pub fn resume(&self) -> String {
        match self {
            Self::Eau(e) => format!("Eau, {}", surface(e.aire_m2)),
            Self::Zone { aire_m2, .. } => format!("Zone, {}", surface(*aire_m2)),
            Self::Route { longueur_m, .. } => format!("Route, {longueur_m:.0} m"),
        }
    }

    /// Lignes à dessiner (fermées pour l'eau et les zones).
    pub fn lignes(&self) -> Vec<(Vec<P2>, bool)> {
        match self {
            Self::Eau(e) => e.bords.iter().map(|b| (b.clone(), true)).collect(),
            Self::Zone { polygone, .. } => vec![(polygone.clone(), true)],
            Self::Route { trace, .. } => vec![(trace.clone(), false)],
        }
    }

    /// Point représentatif (pour centrer la vue).
    pub fn centre(&self) -> P2 {
        let pts: Vec<P2> = self.lignes().into_iter().flat_map(|(l, _)| l).collect();
        let (a, b) = polygone::englobant(&pts);
        [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
    }
}

fn surface(m2: f64) -> String {
    if m2 >= 1_000_000.0 {
        format!("{:.2} km²", m2 / 1_000_000.0)
    } else {
        format!("{:.1} ha", m2 / 10_000.0)
    }
}

/// Plan d'eau proposé : masque sur la grille d'analyse (îles comprises), et ses bords pour l'affichage.
#[derive(Debug, Clone, PartialEq)]
pub struct Eau {
    /// Masque recadré.
    masque: Masque,
    /// Position du masque dans la grille (pixels).
    decalage: (usize, usize),
    /// Calque tel qu'au moment de l'analyse.
    reference: Reference,
    /// Facteur de réduction de la grille.
    facteur: u32,
    /// Bords (rivage et îles), simplifiés.
    pub bords: Vec<Vec<P2>>,
    /// Aire (m²).
    pub aire_m2: f64,
}

impl Eau {
    /// Le point de la carte est-il dans l'eau ?
    pub fn contient(&self, p: P2) -> bool {
        let (x, y) = self.reference.vers_image(p);
        let f = f64::from(self.facteur);
        let (gx, gy) = ((x / f).floor(), (y / f).floor());
        if gx < 0.0 || gy < 0.0 {
            return false;
        }
        let (gx, gy) = (gx as usize, gy as usize);
        gx >= self.decalage.0
            && gy >= self.decalage.1
            && self
                .masque
                .get((gx - self.decalage.0) as isize, (gy - self.decalage.1) as isize)
    }

    /// Rectangle englobant sur la carte (m).
    pub fn englobant(&self) -> (P2, P2) {
        let f = f64::from(self.facteur);
        let (x0, y0) = (self.decalage.0 as f64 * f, self.decalage.1 as f64 * f);
        let (x1, y1) = (
            (self.decalage.0 + self.masque.l) as f64 * f,
            (self.decalage.1 + self.masque.h) as f64 * f,
        );
        polygone::englobant(&[
            self.reference.vers_carte(x0, y0),
            self.reference.vers_carte(x1, y0),
            self.reference.vers_carte(x1, y1),
            self.reference.vers_carte(x0, y1),
        ])
    }
}

// ------------------------------------------------------------------------------------------------ masques

/// Masque binaire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Masque {
    /// Largeur.
    pub l: usize,
    /// Hauteur.
    pub h: usize,
    /// Valeurs, ligne par ligne.
    pub v: Vec<bool>,
}

impl Masque {
    /// Masque vide.
    pub fn vide(l: usize, h: usize) -> Self {
        Self {
            l,
            h,
            v: vec![false; l * h],
        }
    }

    /// Valeur (faux hors du masque).
    pub fn get(&self, x: isize, y: isize) -> bool {
        x >= 0
            && y >= 0
            && (x as usize) < self.l
            && (y as usize) < self.h
            && self.v[y as usize * self.l + x as usize]
    }

    fn set(&mut self, x: usize, y: usize, b: bool) {
        self.v[y * self.l + x] = b;
    }

    /// Pixels vrais.
    pub fn compte(&self) -> usize {
        self.v.iter().filter(|b| **b).count()
    }

    /// Recadre sur les pixels vrais, avec une marge d'un pixel. Renvoie le masque et son décalage.
    fn recadrer(&self) -> Option<(Masque, (usize, usize))> {
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        for y in 0..self.h {
            for x in 0..self.l {
                if self.v[y * self.l + x] {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        if x0 == usize::MAX {
            return None;
        }
        let (x0, y0) = (x0.saturating_sub(1), y0.saturating_sub(1));
        let (x1, y1) = ((x1 + 1).min(self.l - 1), (y1 + 1).min(self.h - 1));
        let mut m = Masque::vide(x1 - x0 + 1, y1 - y0 + 1);
        for y in y0..=y1 {
            for x in x0..=x1 {
                m.set(x - x0, y - y0, self.v[y * self.l + x]);
            }
        }
        Some((m, (x0, y0)))
    }
}

pub(crate) const PAS_DROIT: u32 = 3;
const PAS_DIAG: u32 = 4;
const LOIN: u32 = u32::MAX / 2;

/// Distance (chanfrein 3-4, en tiers de pixel) de chaque pixel au pixel le plus proche valant `cible`. Hors du
/// masque, rien ne compte comme `cible`.
pub(crate) fn distance_a(m: &Masque, cible: bool) -> Vec<u32> {
    let (l, h) = (m.l, m.h);
    let mut d: Vec<u32> = m.v.iter().map(|&b| if b == cible { 0 } else { LOIN }).collect();
    for y in 0..h {
        for x in 0..l {
            let i = y * l + x;
            let mut v = d[i];
            if x > 0 {
                v = v.min(d[i - 1] + PAS_DROIT);
            }
            if y > 0 {
                v = v.min(d[i - l] + PAS_DROIT);
                if x > 0 {
                    v = v.min(d[i - l - 1] + PAS_DIAG);
                }
                if x + 1 < l {
                    v = v.min(d[i - l + 1] + PAS_DIAG);
                }
            }
            d[i] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..l).rev() {
            let i = y * l + x;
            let mut v = d[i];
            if x + 1 < l {
                v = v.min(d[i + 1] + PAS_DROIT);
            }
            if y + 1 < h {
                v = v.min(d[i + l] + PAS_DROIT);
                if x + 1 < l {
                    v = v.min(d[i + l + 1] + PAS_DIAG);
                }
                if x > 0 {
                    v = v.min(d[i + l - 1] + PAS_DIAG);
                }
            }
            d[i] = v;
        }
    }
    d
}

/// Fermeture morphologique (dilatation puis érosion d'un disque de rayon `r`) : comble les trous et coupures plus
/// fins que ~2r sans grossir la tache.
pub fn fermer(m: &Masque, r: u32) -> Masque {
    if r == 0 {
        return m.clone();
    }
    // « + 1 » : au rayon 1, les voisins en diagonale (distance 4) comptent aussi.
    let seuil = r * PAS_DROIT + 1;
    let dilate = Masque {
        l: m.l,
        h: m.h,
        v: distance_a(m, true).iter().map(|d| *d <= seuil).collect(),
    };
    // Le bord de la grille ne ronge pas la tache (la mer qui touche le bord reste entière).
    Masque {
        l: m.l,
        h: m.h,
        v: distance_a(&dilate, false).iter().map(|d| *d > seuil).collect(),
    }
}

/// Étiquette les composantes connexes des pixels valant `valeur` (0 = autre valeur), en 4- ou 8-connexité. Renvoie
/// les étiquettes et la taille de chaque composante (indice = étiquette − 1).
fn etiqueter(m: &Masque, valeur: bool, huit: bool) -> (Vec<u32>, Vec<usize>) {
    let mut etiquettes = vec![0u32; m.l * m.h];
    let mut tailles = Vec::new();
    let mut file = VecDeque::new();
    for depart in 0..m.v.len() {
        if m.v[depart] != valeur || etiquettes[depart] != 0 {
            continue;
        }
        tailles.push(0usize);
        let e = tailles.len() as u32;
        etiquettes[depart] = e;
        file.push_back(depart);
        while let Some(i) = file.pop_front() {
            tailles[e as usize - 1] += 1;
            let (x, y) = (i % m.l, i / m.l);
            let mut voir = |j: usize| {
                if m.v[j] == valeur && etiquettes[j] == 0 {
                    etiquettes[j] = e;
                    file.push_back(j);
                }
            };
            if x > 0 {
                voir(i - 1);
            }
            if x + 1 < m.l {
                voir(i + 1);
            }
            if y > 0 {
                voir(i - m.l);
            }
            if y + 1 < m.h {
                voir(i + m.l);
            }
            if huit {
                if y > 0 && x > 0 {
                    voir(i - m.l - 1);
                }
                if y > 0 && x + 1 < m.l {
                    voir(i - m.l + 1);
                }
                if y + 1 < m.h && x > 0 {
                    voir(i + m.l - 1);
                }
                if y + 1 < m.h && x + 1 < m.l {
                    voir(i + m.l + 1);
                }
            }
        }
    }
    (etiquettes, tailles)
}

/// Comble les trous (zones fausses entourées, sans contact avec le bord) d'au plus `aire_max` pixels.
pub fn combler_trous(m: &Masque, aire_max: usize) -> Masque {
    let (etiquettes, tailles) = etiqueter(m, false, false);
    let mut touche_bord = vec![false; tailles.len()];
    for y in 0..m.h {
        for x in 0..m.l {
            if x == 0 || y == 0 || x + 1 == m.l || y + 1 == m.h {
                let e = etiquettes[y * m.l + x];
                if e > 0 {
                    touche_bord[e as usize - 1] = true;
                }
            }
        }
    }
    let mut r = m.clone();
    for (i, e) in etiquettes.iter().enumerate() {
        if *e > 0 && !touche_bord[*e as usize - 1] && tailles[*e as usize - 1] <= aire_max {
            r.v[i] = true;
        }
    }
    r
}

/// Contours d'un masque, sur les coins des pixels (`(x, y)` de 0 à `l`, `h`). Les pixels en diagonale ne sont pas
/// reliés (4-connexité). Aire signée **négative** pour un bord extérieur, positive pour un trou (repère image, y
/// vers le bas). Seuls les coins sont gardés.
pub fn contours(m: &Masque) -> Vec<Vec<(i32, i32)>> {
    // Arêtes orientées autour de chaque pixel vrai, là où le voisin est faux : haut vers la gauche, gauche vers le
    // bas, bas vers la droite, droite vers le haut.
    let mut sortantes: HashMap<(i32, i32), Vec<(i32, i32)>> = HashMap::new();
    for y in 0..m.h as i32 {
        for x in 0..m.l as i32 {
            if !m.get(x as isize, y as isize) {
                continue;
            }
            let mut arete = |a: (i32, i32), b: (i32, i32)| sortantes.entry(a).or_default().push(b);
            if !m.get(x as isize, y as isize - 1) {
                arete((x + 1, y), (x, y));
            }
            if !m.get(x as isize - 1, y as isize) {
                arete((x, y), (x, y + 1));
            }
            if !m.get(x as isize, y as isize + 1) {
                arete((x, y + 1), (x + 1, y + 1));
            }
            if !m.get(x as isize + 1, y as isize) {
                arete((x + 1, y + 1), (x + 1, y));
            }
        }
    }
    let mut departs: Vec<(i32, i32)> = sortantes.keys().copied().collect();
    departs.sort_unstable();
    let mut boucles = Vec::new();
    for depart in departs {
        while let Some(premier) = sortantes.get_mut(&depart).and_then(Vec::pop) {
            let mut boucle = vec![depart];
            let (mut prec, mut ici) = (depart, premier);
            while ici != depart {
                boucle.push(ici);
                let entree = (ici.0 - prec.0, ici.1 - prec.1);
                let Some(choix) = sortantes.get_mut(&ici) else {
                    break;
                };
                if choix.is_empty() {
                    break;
                }
                // Au coin partagé par deux pixels en diagonale : tourner comme autour d'un pixel seul (produit
                // vectoriel −1), ce qui sépare les deux pixels.
                let rang = |s: &(i32, i32)| {
                    let d = (s.0 - ici.0, s.1 - ici.1);
                    match entree.0 * d.1 - entree.1 * d.0 {
                        -1 => 0,
                        0 => 1,
                        _ => 2,
                    }
                };
                let k = (0..choix.len()).min_by_key(|&k| rang(&choix[k])).unwrap_or(0);
                let suivant = choix.swap_remove(k);
                prec = ici;
                ici = suivant;
            }
            boucles.push(coins(&boucle));
        }
    }
    boucles
}

/// Garde les points où la direction change.
fn coins(boucle: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let n = boucle.len();
    (0..n)
        .filter(|&i| {
            let (a, b, c) = (boucle[(i + n - 1) % n], boucle[i], boucle[(i + 1) % n]);
            (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0) != 0
        })
        .map(|i| boucle[i])
        .collect()
}

fn aire_signee_px(b: &[(i32, i32)]) -> f64 {
    let n = b.len();
    (0..n)
        .map(|i| {
            let (p, q) = (b[i], b[(i + 1) % n]);
            f64::from(p.0) * f64::from(q.1) - f64::from(q.0) * f64::from(p.1)
        })
        .sum::<f64>()
        * 0.5
}

// ------------------------------------------------------------------------------------------------ simplification

/// Douglas–Peucker sur une polyligne ouverte.
pub fn simplifier(pts: &[P2], tolerance: f64) -> Vec<P2> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut garder = vec![false; pts.len()];
    garder[0] = true;
    garder[pts.len() - 1] = true;
    let mut pile = vec![(0, pts.len() - 1)];
    while let Some((a, b)) = pile.pop() {
        let (mut loin, mut k) = (0.0, 0);
        for (i, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let d = mcf_geo::distance_segment(*p, pts[a], pts[b]);
            if d > loin {
                loin = d;
                k = i;
            }
        }
        if loin > tolerance {
            garder[k] = true;
            pile.push((a, k));
            pile.push((k, b));
        }
    }
    pts.iter()
        .zip(garder)
        .filter_map(|(p, g)| g.then_some(*p))
        .collect()
}

/// Douglas–Peucker sur un polygone fermé (coupé au point le plus éloigné du premier).
pub fn simplifier_ferme(pts: &[P2], tolerance: f64) -> Vec<P2> {
    if pts.len() < 4 {
        return pts.to_vec();
    }
    let k = (1..pts.len())
        .max_by(|&i, &j| mcf_geo::distance(pts[0], pts[i]).total_cmp(&mcf_geo::distance(pts[0], pts[j])))
        .unwrap_or(pts.len() / 2);
    let mut a = simplifier(&pts[..=k], tolerance);
    let mut retour: Vec<P2> = pts[k..].to_vec();
    retour.push(pts[0]);
    let b = simplifier(&retour, tolerance);
    a.pop();
    a.extend_from_slice(&b[..b.len() - 1]);
    a
}

// ------------------------------------------------------------------------------------------------ squelette

/// Amincissement de Zhang–Suen, puis retrait des pixels « en escalier » : squelette d'un pixel d'épaisseur,
/// 8-connexe.
pub fn squelette(m: &Masque) -> Masque {
    let mut s = m.clone();
    let voisins = |s: &Masque, x: usize, y: usize| -> [bool; 8] {
        let (x, y) = (x as isize, y as isize);
        // P2 (nord) puis sens horaire.
        [
            s.get(x, y - 1),
            s.get(x + 1, y - 1),
            s.get(x + 1, y),
            s.get(x + 1, y + 1),
            s.get(x, y + 1),
            s.get(x - 1, y + 1),
            s.get(x - 1, y),
            s.get(x - 1, y - 1),
        ]
    };
    loop {
        let mut change = false;
        for passe in 0..2 {
            let mut retirer = Vec::new();
            for y in 0..s.h {
                for x in 0..s.l {
                    if !s.v[y * s.l + x] {
                        continue;
                    }
                    let p = voisins(&s, x, y);
                    let b = p.iter().filter(|v| **v).count();
                    let a = (0..8).filter(|&i| !p[i] && p[(i + 1) % 8]).count();
                    let (n, e, su, o) = (p[0], p[2], p[4], p[6]);
                    let ok = if passe == 0 {
                        !(n && e && su) && !(e && su && o)
                    } else {
                        !(n && e && o) && !(n && su && o)
                    };
                    if (2..=6).contains(&b) && a == 1 && ok {
                        retirer.push(y * s.l + x);
                    }
                }
            }
            change |= !retirer.is_empty();
            for i in retirer {
                s.v[i] = false;
            }
        }
        if !change {
            break;
        }
    }
    // Escaliers : un pixel dont les voisins forment un seul groupe 8-connexe (et qui en a au moins deux) est
    // superflu.
    for y in 0..s.h {
        for x in 0..s.l {
            if !s.v[y * s.l + x] {
                continue;
            }
            let p = voisins(&s, x, y);
            if p.iter().filter(|v| **v).count() >= 2 && groupes_anneau(&p) == 1 {
                s.v[y * s.l + x] = false;
            }
        }
    }
    s
}

/// Groupes 8-connexes parmi les voisins d'un pixel (ordre : nord puis sens horaire).
fn groupes_anneau(p: &[bool; 8]) -> usize {
    let mut vu = [false; 8];
    let mut groupes = 0;
    for i in 0..8 {
        if !p[i] || vu[i] {
            continue;
        }
        groupes += 1;
        let mut pile = vec![i];
        vu[i] = true;
        while let Some(k) = pile.pop() {
            // Voisins dans l'anneau ; deux voisins droits séparés d'un coin se touchent aussi.
            let mut proches = vec![(k + 1) % 8, (k + 7) % 8];
            if k % 2 == 0 {
                proches.extend([(k + 2) % 8, (k + 6) % 8]);
            }
            for j in proches {
                if p[j] && !vu[j] {
                    vu[j] = true;
                    pile.push(j);
                }
            }
        }
    }
    groupes
}

/// Branches d'un squelette entre extrémités et carrefours (pixels, centres), ergots plus courts que `ergot_px`
/// retirés. Un carrefour de plusieurs pixels compte pour un seul point (leur moyenne).
pub fn branches(s: &Masque, ergot_px: f64) -> Vec<Vec<(f64, f64)>> {
    let (l, h) = (s.l, s.h);
    let idx = |x: usize, y: usize| y * l + x;
    let voisins = |i: usize| -> Vec<usize> {
        let (x, y) = ((i % l) as isize, (i / l) as isize);
        let mut v = Vec::with_capacity(8);
        for dy in -1..=1 {
            for dx in -1..=1 {
                if (dx, dy) != (0, 0) && s.get(x + dx, y + dy) {
                    v.push(idx((x + dx) as usize, (y + dy) as usize));
                }
            }
        }
        v
    };
    let degre: Vec<usize> = (0..l * h)
        .map(|i| if s.v[i] { voisins(i).len() } else { 0 })
        .collect();
    let noeud = |i: usize| s.v[i] && degre[i] != 2;
    // Grappes de pixels-nœuds voisins.
    let mut grappe = vec![usize::MAX; l * h];
    let mut centres: Vec<(f64, f64)> = Vec::new();
    let mut extremite: Vec<bool> = Vec::new();
    for i in 0..l * h {
        if !noeud(i) || grappe[i] != usize::MAX {
            continue;
        }
        let g = centres.len();
        let mut pile = vec![i];
        grappe[i] = g;
        let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
        let mut seul_bout = true;
        while let Some(k) = pile.pop() {
            sx += (k % l) as f64 + 0.5;
            sy += (k / l) as f64 + 0.5;
            n += 1.0;
            seul_bout &= degre[k] <= 1;
            for j in voisins(k) {
                if noeud(j) && grappe[j] == usize::MAX {
                    grappe[j] = g;
                    pile.push(j);
                }
            }
        }
        centres.push((sx / n, sy / n));
        extremite.push(seul_bout && n == 1.0);
    }
    let centre_px = |i: usize| ((i % l) as f64 + 0.5, (i / l) as f64 + 0.5);
    // Parcours des branches.
    let mut vu = vec![false; l * h];
    // (grappe de départ, grappe d'arrivée ou None pour une boucle, points)
    let mut liste: Vec<(usize, Option<usize>, Vec<(f64, f64)>)> = Vec::new();
    for i in 0..l * h {
        if !noeud(i) {
            continue;
        }
        for premier in voisins(i) {
            if noeud(premier) || vu[premier] {
                continue;
            }
            let mut pts = vec![centres[grappe[i]], centre_px(premier)];
            vu[premier] = true;
            let (mut prec, mut ici) = (i, premier);
            let fin = loop {
                let suivant = voisins(ici)
                    .into_iter()
                    .find(|&j| j != prec && !(vu[j] && !noeud(j)));
                match suivant {
                    Some(j) if noeud(j) => break Some(grappe[j]),
                    Some(j) => {
                        vu[j] = true;
                        pts.push(centre_px(j));
                        prec = ici;
                        ici = j;
                    }
                    None => break None,
                }
            };
            match fin {
                // Petit retour sur le carrefour de départ : artefact du squelette.
                Some(g) if g == grappe[i] && pts.len() <= 4 => continue,
                Some(g) => pts.push(centres[g]),
                None => continue,
            }
            liste.push((grappe[i], fin, pts));
        }
    }
    // Boucles sans carrefour (rocade isolée).
    for i in 0..l * h {
        if !s.v[i] || noeud(i) || vu[i] {
            continue;
        }
        let mut pts = vec![centre_px(i)];
        vu[i] = true;
        let (mut prec, mut ici) = (usize::MAX, i);
        while let Some(j) = voisins(ici).into_iter().find(|&j| j != prec && !vu[j]) {
            vu[j] = true;
            pts.push(centre_px(j));
            prec = ici;
            ici = j;
        }
        pts.push(centre_px(i));
        liste.push((usize::MAX, None, pts));
    }
    // Ergots : branche courte qui finit en cul-de-sac sur un côté.
    let longueur = |p: &[(f64, f64)]| -> f64 {
        p.windows(2)
            .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
            .sum()
    };
    let est_bout = |g: usize| g != usize::MAX && extremite[g];
    liste.retain(|(a, b, pts)| {
        let bout = est_bout(*a) || b.is_some_and(est_bout);
        let carrefour = !est_bout(*a) || b.is_some_and(|g| !est_bout(g));
        !(bout && carrefour && longueur(pts) < ergot_px)
    });
    // Recoller les branches qui se suivent à un nœud devenu simple (ergot retiré).
    loop {
        let mut compte: HashMap<usize, usize> = HashMap::new();
        for (a, b, _) in &liste {
            if *a != usize::MAX {
                *compte.entry(*a).or_default() += 1;
            }
            if let Some(b) = b {
                *compte.entry(*b).or_default() += 1;
            }
        }
        let Some(g) = compte
            .iter()
            .filter(|(_, n)| **n == 2)
            .map(|(g, _)| *g)
            .find(|g| {
                // Pas une branche qui revient sur elle-même.
                !liste.iter().any(|(a, b, _)| a == g && *b == Some(*g))
            })
        else {
            break;
        };
        let mut idx: Vec<usize> = (0..liste.len())
            .filter(|&k| liste[k].0 == g || liste[k].1 == Some(g))
            .collect();
        idx.sort_unstable();
        let (k2, k1) = (idx[1], idx[0]);
        let b2 = liste.swap_remove(k2);
        let b1 = liste.swap_remove(k1);
        // Orienter b1 pour finir en g, b2 pour partir de g.
        let (a1, mut p1) = if b1.1 == Some(g) {
            (b1.0, b1.2)
        } else {
            let mut p = b1.2;
            p.reverse();
            (b1.1.unwrap_or(g), p)
        };
        let (fin2, p2) = if b2.0 == g {
            (b2.1, b2.2)
        } else {
            let mut p = b2.2;
            p.reverse();
            (Some(b2.0), p)
        };
        p1.extend_from_slice(&p2[1..]);
        liste.push((a1, fin2, p1));
    }
    liste.into_iter().map(|(_, _, p)| p).collect()
}

// ------------------------------------------------------------------------------------------------ couleurs

fn lineaire() -> [f64; 256] {
    let mut t = [0.0f64; 256];
    for (i, v) in t.iter_mut().enumerate() {
        let c = i as f64 / 255.0;
        *v = if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        };
    }
    t
}

/// sRGB 8 bits → CIE Lab (D65). Calcul en `f64` (les fonctions `f32` de la bibliothèque standard ne sont pas
/// déterministes d'une plateforme à l'autre, règle clippy de Bevy).
fn lab(t: &[f64; 256], r: u8, g: u8, b: u8) -> [f32; 3] {
    let (r, g, b) = (t[r as usize], t[g as usize], t[b as usize]);
    let x = (0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.950_47;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b;
    let z = (0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b) / 1.088_83;
    let f = |v: f64| {
        if v > 0.008_856 {
            v.cbrt()
        } else {
            7.787 * v + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [
        (116.0 * fy - 16.0) as f32,
        (500.0 * (fx - fy)) as f32,
        (200.0 * (fy - fz)) as f32,
    ]
}

fn ecart2(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

// ------------------------------------------------------------------------------------------------ grille

/// Image d'un calque préparée pour l'analyse.
#[derive(Debug, Clone)]
pub struct Grille {
    /// Facteur de réduction (1 = pixels de l'image).
    pub facteur: u32,
    /// Largeur (pixels de grille).
    pub l: usize,
    /// Hauteur (pixels de grille).
    pub h: usize,
    lab: Vec<[f32; 3]>,
    /// Pixels posés sur la carte (et opaques) : seuls analysés.
    sur_carte: Masque,
    /// Calque au moment de l'analyse.
    pub reference: Reference,
}

impl Grille {
    /// Prépare l'image du calque `r` posé sur la carte `m`.
    pub fn preparer(image: &ImageRgba, r: &Reference, m: &Manifeste) -> Result<Self, String> {
        if image.largeur != r.largeur_px || image.hauteur != r.hauteur_px {
            return Err("image du calque absente ou de mauvaise taille".into());
        }
        let pixels = u64::from(image.largeur) * u64::from(image.hauteur);
        let mut facteur = 1u32;
        while pixels / u64::from(facteur * facteur) > MAX_PIXELS_ANALYSE {
            facteur += 1;
        }
        let (l, h) = (
            image.largeur.div_ceil(facteur) as usize,
            image.hauteur.div_ceil(facteur) as usize,
        );
        let t = lineaire();
        let mut lab_ = Vec::with_capacity(l * h);
        let mut sur_carte = Masque::vide(l, h);
        let (iw, ih) = (image.largeur as usize, image.hauteur as usize);
        let f = facteur as usize;
        for gy in 0..h {
            for gx in 0..l {
                let mut somme = [0u32; 4];
                let mut n = 0u32;
                for y in gy * f..((gy + 1) * f).min(ih) {
                    for x in gx * f..((gx + 1) * f).min(iw) {
                        let k = (y * iw + x) * 4;
                        for (c, s) in somme.iter_mut().enumerate() {
                            *s += u32::from(image.rgba[k + c]);
                        }
                        n += 1;
                    }
                }
                let moy = somme.map(|s| (s / n.max(1)) as u8);
                lab_.push(lab(&t, moy[0], moy[1], moy[2]));
                let centre = r.vers_carte((gx as f64 + 0.5) * f as f64, (gy as f64 + 0.5) * f as f64);
                sur_carte.set(gx, gy, moy[3] >= 128 && m.contient(centre));
            }
        }
        if sur_carte.compte() == 0 {
            return Err("le calque ne recouvre pas la carte : calez-le d'abord".into());
        }
        Ok(Self {
            facteur,
            l,
            h,
            lab: lab_,
            sur_carte,
            reference: r.clone(),
        })
    }

    /// Mètres par pixel de grille.
    pub fn m_par_pixel(&self) -> f64 {
        self.reference.m_par_px * f64::from(self.facteur)
    }

    /// Point de la carte au point `(x, y)` de la grille (pixels de grille, réels).
    pub fn vers_carte(&self, x: f64, y: f64) -> P2 {
        let f = f64::from(self.facteur);
        self.reference.vers_carte(x * f, y * f)
    }

    /// Pixel de grille sous un point de la carte, s'il est analysé.
    pub fn pixel(&self, p: P2) -> Option<(usize, usize)> {
        let (x, y) = self.reference.vers_image(p);
        let f = f64::from(self.facteur);
        let (gx, gy) = ((x / f).floor(), (y / f).floor());
        if gx < 0.0 || gy < 0.0 || gx >= self.l as f64 || gy >= self.h as f64 {
            return None;
        }
        let (gx, gy) = (gx as usize, gy as usize);
        self.sur_carte.v[gy * self.l + gx].then_some((gx, gy))
    }

    /// Couleur (Lab) moyenne autour d'un pixel (3 × 3, pixels analysés seulement).
    fn couleur_autour(&self, gx: usize, gy: usize) -> [f32; 3] {
        let mut s = [0.0f32; 3];
        let mut n = 0.0;
        for y in gy.saturating_sub(1)..=(gy + 1).min(self.h - 1) {
            for x in gx.saturating_sub(1)..=(gx + 1).min(self.l - 1) {
                let i = y * self.l + x;
                if self.sur_carte.v[i] {
                    for (somme, v) in s.iter_mut().zip(self.lab[i]) {
                        *somme += v;
                    }
                    n += 1.0;
                }
            }
        }
        s.map(|v| v / f32::max(n, 1.0))
    }

    /// Pixels analysés proches de la couleur `c`.
    fn masque_couleur(&self, c: [f32; 3], tolerance: f32) -> Masque {
        let t2 = tolerance * tolerance;
        Masque {
            l: self.l,
            h: self.h,
            v: self
                .lab
                .iter()
                .zip(&self.sur_carte.v)
                .map(|(p, sur)| *sur && ecart2(*p, c) <= t2)
                .collect(),
        }
    }

    /// Analyse : propositions pour la couleur sous le point `graine` de la carte.
    pub fn analyser(
        &self,
        sorte: Sorte,
        graine: P2,
        reglages: &Reglages,
    ) -> Result<Vec<Proposition>, String> {
        let (gx, gy) = self
            .pixel(graine)
            .ok_or("cliquez sur le calque, à l'intérieur de la carte")?;
        // Une route fait souvent un ou deux pixels de large : sa couleur est celle du pixel cliqué, pas une moyenne
        // avec les bords. Pour une tache, la moyenne 3 × 3 lisse le grain (satmap).
        let couleur = match sorte {
            Sorte::Routes => self.lab[gy * self.l + gx],
            Sorte::Eau | Sorte::Zone => self.couleur_autour(gx, gy),
        };
        let tolerance = reglages.tolerance.clamp(1.0, 100.0);
        let brut = self.masque_couleur(couleur, tolerance);
        let combler = reglages.combler_px.min(MAX_COMBLER_PX);
        let mut masque = fermer(&brut, combler);
        for (v, sur) in masque.v.iter_mut().zip(&self.sur_carte.v) {
            *v &= *sur;
        }
        let px2 = self.m_par_pixel() * self.m_par_pixel();
        let aire_min_px = (reglages.aire_min_m2.max(0.0) / px2).ceil() as usize;
        // Un trait de route d'un pixel est souvent relié seulement en diagonale.
        let (etiquettes, tailles) = etiqueter(&masque, true, sorte == Sorte::Routes);
        let sous_clic = etiquettes[gy * self.l + gx];
        if sous_clic == 0 {
            return Err("couleur introuvable sous le clic : augmentez la tolérance".into());
        }
        // Composantes retenues, les plus grandes d'abord.
        let mut retenues: Vec<u32> = if reglages.toutes {
            (1..=tailles.len() as u32)
                .filter(|e| tailles[*e as usize - 1] >= aire_min_px.max(1))
                .collect()
        } else {
            vec![sous_clic]
        };
        retenues.sort_by_key(|e| std::cmp::Reverse(tailles[*e as usize - 1]));
        let mut propositions = Vec::new();
        for e in retenues {
            if propositions.len() >= MAX_PROPOSITIONS {
                break;
            }
            let composante = Masque {
                l: self.l,
                h: self.h,
                v: etiquettes.iter().map(|x| *x == e).collect(),
            };
            let Some((m, decalage)) = composante.recadrer() else {
                continue;
            };
            match sorte {
                Sorte::Eau => propositions.push(self.eau(m, decalage, aire_min_px)),
                Sorte::Zone => propositions.extend(self.zone(&m, decalage, reglages.aire_min_m2)),
                Sorte::Routes => {
                    let reste = MAX_PROPOSITIONS - propositions.len();
                    propositions.extend(self.routes(&m, decalage, reglages.longueur_min_m, reste));
                }
            }
        }
        if propositions.is_empty() {
            return Err("rien à proposer : élargissez la tolérance ou baissez les minimums".into());
        }
        Ok(propositions)
    }

    fn boucle_vers_carte(&self, b: &[(i32, i32)], decalage: (usize, usize)) -> Vec<P2> {
        b.iter()
            .map(|&(x, y)| {
                self.vers_carte(f64::from(x) + decalage.0 as f64, f64::from(y) + decalage.1 as f64)
            })
            .collect()
    }

    fn eau(&self, m: Masque, decalage: (usize, usize), aire_min_px: usize) -> Proposition {
        let m = combler_trous(&m, aire_min_px);
        let aire_m2 = m.compte() as f64 * self.m_par_pixel() * self.m_par_pixel();
        let tol = self.m_par_pixel();
        let bords = contours(&m)
            .iter()
            .map(|b| simplifier_ferme(&self.boucle_vers_carte(b, decalage), tol))
            .filter(|b| b.len() >= 3)
            .collect();
        Proposition::Eau(Eau {
            masque: m,
            decalage,
            reference: self.reference.clone(),
            facteur: self.facteur,
            bords,
            aire_m2,
        })
    }

    fn zone(&self, m: &Masque, decalage: (usize, usize), aire_min_m2: f64) -> Option<Proposition> {
        let plein = combler_trous(m, usize::MAX);
        let exterieur = contours(&plein)
            .into_iter()
            .filter(|b| aire_signee_px(b) < 0.0)
            .max_by(|a, b| aire_signee_px(b).total_cmp(&aire_signee_px(a)))?;
        let brut = self.boucle_vers_carte(&exterieur, decalage);
        let poly = polygone_valide(&brut, self.m_par_pixel(), MAX_SOMMETS_ZONE)?;
        let aire_m2 = polygone::aire(&poly);
        (aire_m2 >= aire_min_m2.max(1.0)).then_some(Proposition::Zone {
            polygone: poly,
            aire_m2,
        })
    }

    fn routes(
        &self,
        m: &Masque,
        decalage: (usize, usize),
        longueur_min_m: f64,
        reste: usize,
    ) -> Vec<Proposition> {
        let mpp = self.m_par_pixel();
        let ergot_px = (longueur_min_m / mpp).max(2.0);
        let mut traces: Vec<(Vec<P2>, f64)> = branches(&squelette(m), ergot_px)
            .into_iter()
            .filter_map(|b| {
                let pts: Vec<P2> = b
                    .iter()
                    .map(|&(x, y)| self.vers_carte(x + decalage.0 as f64, y + decalage.1 as f64))
                    .collect();
                let mut tol = mpp;
                let mut trace = simplifier(&pts, tol);
                while trace.len() > MAX_POINTS_ROUTE {
                    tol *= 1.5;
                    trace = simplifier(&pts, tol);
                }
                let longueur = mcf_geo::longueur(&trace);
                (trace.len() >= 2 && longueur >= longueur_min_m.max(mpp)).then_some((trace, longueur))
            })
            .collect();
        traces.sort_by(|a, b| b.1.total_cmp(&a.1));
        traces.truncate(reste);
        traces
            .into_iter()
            .map(|(trace, longueur_m)| Proposition::Route { trace, longueur_m })
            .collect()
    }
}

/// Polygone simple aux bornes de la carte et sous `max` sommets : la tolérance grandit jusqu'à y parvenir.
fn polygone_valide(brut: &[P2], tol: f64, max: usize) -> Option<Vec<P2>> {
    let mut t = tol;
    for _ in 0..12 {
        let mut p = simplifier_ferme(brut, t);
        p.dedup_by(|a, b| mcf_geo::distance(*a, *b) < 1e-6);
        if p.len() >= 3 && p.len() <= max && polygone::est_simple(&p) {
            return Some(polygone::oriente_positif(&p));
        }
        t *= 1.5;
    }
    None
}

/// Applique les bornes de la carte à un tracé ou un polygone (les bords des pixels peuvent déborder d'un
/// demi-pixel).
pub fn borner(pts: &[P2], m: &Manifeste) -> Vec<P2> {
    let (a, b) = m.bornes();
    let mut r: Vec<P2> = pts
        .iter()
        .map(|p| [p[0].clamp(a[0], b[0]), p[1].clamp(a[1], b[1])])
        .collect();
    r.dedup_by(|x, y| mcf_geo::distance(*x, *y) < 1e-6);
    r
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn masque(lignes: &[&str]) -> Masque {
        let h = lignes.len();
        let l = lignes[0].len();
        Masque {
            l,
            h,
            v: lignes.iter().flat_map(|s| s.bytes().map(|b| b == b'#')).collect(),
        }
    }

    #[test]
    fn contour_d_un_carre_avec_trou() {
        let m = masque(&["#####", "#...#", "#...#", "#####"]);
        let mut c = contours(&m);
        c.sort_by(|a, b| aire_signee_px(a).total_cmp(&aire_signee_px(b)));
        assert_eq!(c.len(), 2);
        assert_eq!(aire_signee_px(&c[0]), -20.0, "bord extérieur 5 × 4");
        assert_eq!(c[0].len(), 4);
        assert_eq!(aire_signee_px(&c[1]), 6.0, "trou 3 × 2");
    }

    #[test]
    fn pixels_en_diagonale_separes() {
        let m = masque(&["#.", ".#"]);
        let c = contours(&m);
        assert_eq!(c.len(), 2);
        assert!(c.iter().all(|b| aire_signee_px(b) == -1.0));
    }

    #[test]
    fn fermeture_comble_un_texte_et_trous_combles() {
        // Une tache coupée par une ligne d'un pixel : refermée par un rayon 1.
        let m = masque(&["#######", "#######", "...#...", "#######", "#######"]);
        let f = fermer(&m, 1);
        assert!(f.get(1, 2) && f.get(5, 2), "{f:?}");
        let (_, tailles) = etiqueter(&f, true, false);
        assert_eq!(tailles.len(), 1);
        // Trous : le petit est comblé, le grand gardé.
        let m = masque(&["########", "#.####..", "######..", "########"]);
        let c = combler_trous(&m, 1);
        assert!(c.get(1, 1));
        assert!(!c.get(6, 1), "zone touchant le bord : pas un trou");
    }

    #[test]
    fn douglas_peucker() {
        let pts: Vec<P2> = (0..=10)
            .map(|i| [f64::from(i), 0.01 * f64::from(i % 2)])
            .collect();
        assert_eq!(simplifier(&pts, 0.1), vec![[0.0, 0.0], [10.0, 0.0]]);
        let carre = vec![[0.0, 0.0], [5.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        assert_eq!(simplifier_ferme(&carre, 0.1).len(), 4);
    }

    #[test]
    fn squelette_d_une_croix_donne_quatre_branches() {
        let mut lignes = vec![String::new(); 21];
        for (y, l) in lignes.iter_mut().enumerate() {
            *l = (0..21)
                .map(|x| {
                    if (9..=11).contains(&y) || (9..=11).contains(&x) {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect();
        }
        let refs: Vec<&str> = lignes.iter().map(String::as_str).collect();
        let s = squelette(&masque(&refs));
        let b = branches(&s, 2.0);
        assert_eq!(b.len(), 4, "{b:?}");
        for br in &b {
            let long: f64 = br
                .windows(2)
                .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
                .sum();
            assert!(long > 6.0, "{br:?}");
        }
    }

    #[test]
    fn ligne_epaisse_une_seule_branche() {
        let m = masque(&[
            "..............",
            ".############.",
            ".############.",
            ".############.",
            "..............",
        ]);
        let b = branches(&squelette(&m), 3.0);
        assert_eq!(b.len(), 1, "{b:?}");
    }

    /// Image synthétique : mer bleue à gauche, terre verte, tache rose (ville) avec une rue blanche, route jaune.
    fn image_test() -> (ImageRgba, Reference, Manifeste) {
        let (l, h) = (200u32, 200u32);
        let mut rgba = Vec::with_capacity((l * h * 4) as usize);
        for y in 0..h {
            for x in 0..l {
                let c: [u8; 3] = if x < 50 {
                    [70, 140, 210]
                } else if (120..170).contains(&x) && (40..90).contains(&y) {
                    if y == 65 { [250, 250, 250] } else { [235, 150, 140] }
                } else if (98..=101).contains(&y) && x >= 50 {
                    [240, 200, 40]
                } else {
                    [150, 200, 110]
                };
                rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        // Lac avec une île au milieu.
        for y in 130..180 {
            for x in 70..120 {
                let (dx, dy) = (x as i32 - 95, y as i32 - 155);
                if dx * dx + dy * dy < 400 && dx * dx + dy * dy > 36 {
                    let k = ((y * l + x) * 4) as usize;
                    rgba[k..k + 3].copy_from_slice(&[70, 140, 210]);
                }
            }
        }
        let image = ImageRgba {
            largeur: l,
            hauteur: h,
            rgba: Arc::new(rgba),
        };
        let m = Manifeste::nouveau("t", 512.0);
        let r = Reference {
            nom: "plan".into(),
            largeur_px: l,
            hauteur_px: h,
            coin: m.bornes().0,
            m_par_px: 2.0,
            rotation_deg: 0.0,
            opacite: 0.5,
            visible: true,
        };
        (image, r, m)
    }

    #[test]
    fn eau_cliquee_garde_l_ile_et_situe_la_cote() {
        let (image, r, m) = image_test();
        let g = Grille::preparer(&image, &r, &m).expect("grille");
        let mer = r.vers_carte(10.0, 10.0);
        let mut reglages = Sorte::Eau.reglages();
        reglages.aire_min_m2 = 50.0;
        let p = g.analyser(Sorte::Eau, mer, &reglages).expect("analyse");
        assert_eq!(p.len(), 1, "seulement la mer cliquée");
        let Proposition::Eau(e) = &p[0] else { panic!() };
        assert!(e.contient(r.vers_carte(25.0, 100.0)));
        assert!(!e.contient(r.vers_carte(60.0, 100.0)));
        // 50 × 200 pixels de 2 m.
        assert!((e.aire_m2 - 40_000.0).abs() < 1.0, "{}", e.aire_m2);
        // Le lac, avec « toutes » : deux plans d'eau ; l'île (≈ 113 px × 4 m²) est gardée.
        reglages.toutes = true;
        let p = g.analyser(Sorte::Eau, mer, &reglages).expect("analyse");
        assert_eq!(p.len(), 2);
        let Proposition::Eau(lac) = &p[1] else { panic!() };
        assert!(lac.contient(r.vers_carte(95.5 + 12.0, 155.5)));
        assert!(!lac.contient(r.vers_carte(95.5, 155.5)), "île gardée");
        assert_eq!(lac.bords.len(), 2, "rivage + île");
    }

    #[test]
    fn tache_urbaine_coupee_par_une_rue_donne_une_zone() {
        let (image, r, m) = image_test();
        let g = Grille::preparer(&image, &r, &m).expect("grille");
        let mut reglages = Sorte::Zone.reglages();
        reglages.aire_min_m2 = 1_000.0;
        let p = g
            .analyser(Sorte::Zone, r.vers_carte(130.0, 50.0), &reglages)
            .expect("analyse");
        assert_eq!(p.len(), 1, "{p:?}");
        let Proposition::Zone { polygone, aire_m2 } = &p[0] else {
            panic!()
        };
        assert!(polygone::est_simple(polygone));
        assert!(polygone::aire_signee(polygone) > 0.0);
        // 50 × 50 pixels de 2 m = 10 000 m².
        assert!((aire_m2 - 10_000.0).abs() < 400.0, "{aire_m2}");
        assert!(
            polygone::contient(polygone, r.vers_carte(145.0, 65.5)),
            "la rue est comblée"
        );
    }

    #[test]
    fn route_jaune_donne_un_trace_droit() {
        let (image, r, m) = image_test();
        let g = Grille::preparer(&image, &r, &m).expect("grille");
        let p = g
            .analyser(
                Sorte::Routes,
                r.vers_carte(100.0, 99.5),
                &Sorte::Routes.reglages(),
            )
            .expect("analyse");
        assert_eq!(p.len(), 1, "{p:?}");
        let Proposition::Route { trace, longueur_m } = &p[0] else {
            panic!()
        };
        assert!(trace.len() <= 4, "{trace:?}");
        // De x = 50 à 200 pixels : ~300 m à 2 m/pixel.
        assert!((longueur_m - 300.0).abs() < 12.0, "{longueur_m}");
        let z = r.vers_carte(0.0, 100.0)[1];
        assert!(trace.iter().all(|q| (q[1] - z).abs() < 3.0), "{trace:?}");
    }

    #[test]
    fn hors_carte_et_grande_image() {
        let (image, mut r, m) = image_test();
        let g = Grille::preparer(&image, &r, &m).expect("grille");
        assert!(
            g.analyser(Sorte::Eau, [10_000.0, 0.0], &Sorte::Eau.reglages())
                .is_err()
        );
        r.coin = [5_000.0, 5_000.0];
        assert!(
            Grille::preparer(&image, &r, &m).is_err(),
            "calque hors de la carte"
        );
        // Image plus grande que le plafond : réduite.
        let grande = ImageRgba {
            largeur: 4096,
            hauteur: 2048,
            rgba: Arc::new(vec![200; 4096 * 2048 * 4]),
        };
        let mut r2 = r.clone();
        r2.largeur_px = 4096;
        r2.hauteur_px = 2048;
        r2.coin = m.bornes().0;
        r2.m_par_px = 0.1;
        let g = Grille::preparer(&grande, &r2, &m).expect("grille");
        assert_eq!(g.facteur, 2);
        assert!((g.l * g.h) as u64 <= MAX_PIXELS_ANALYSE);
    }
}
