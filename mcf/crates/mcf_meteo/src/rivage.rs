//! Vagues de bord (étape 6), portées de `ShoreWaves.hlsli` de SkyForge : trains de houle qui arrivent
//! perpendiculairement au rivage, grandissent en eau peu profonde (loi de Green, A ∝ h^-1/4), se raidissent puis
//! déferlent quand leur hauteur approche 0,78 fois la profondeur (critère de McCowan), et jet de rive (run-up)
//! qui fait monter et descendre l'eau sur le sable.
//!
//! Différence avec SkyForge, où le rivage était une fonction écrite en dur : ici le jeu fournit la hauteur du sol
//! ([`SourceFond`], par exemple lue dans le terrain de la carte). Autour d'un centre (la caméra côté client, un
//! joueur côté serveur), une grille [`GrilleRivage`] échantillonne le fond et calcule la distance signée au rivage
//! (transformée de distance euclidienne exacte) et la normale du rivage. Le même calcul sert au shader de l'océan
//! et au gameplay (flottaison, nage), sans rendu.

use std::sync::Arc;

use bevy::math::{Vec2, Vec3, ops};
use bevy::prelude::*;

use crate::bruit::perlin3;
use crate::etat::EtatMeteo;
use crate::houle::{G, Ocean, deplacement, quantifier_pulsation};
use crate::mer_du_vent::MerDuVent;

/// Distance signée renvoyée quand aucun rivage n'est dans la grille (m).
pub const LOIN_DU_RIVAGE: f32 = 10_000.0;

/// Hauteur du sol (m) en (x, z), fournie par le jeu. Doit être pure et rapide : elle est appelée pour chaque case
/// de la grille quand la grille est reconstruite.
pub type FonctionFond = Arc<dyn Fn(f32, f32) -> f32 + Send + Sync>;

/// Source de la hauteur du sol. Sans cette ressource, pas de vagues de bord (océan profond partout).
#[derive(Resource, Clone)]
pub struct SourceFond(pub FonctionFond);

impl SourceFond {
    /// Source à partir d'une fonction.
    pub fn new(f: impl Fn(f32, f32) -> f32 + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }
}

/// Réglages des vagues de bord (valeurs de SkyForge) et de la grille.
#[derive(Resource, Reflect, Debug, Clone, PartialEq)]
#[reflect(Resource, Default)]
pub struct ParametresRivage {
    /// Multiplicateur des vagues de bord (0 = mer d'huile).
    pub vagues: f32,
    /// Longueur d'onde des vagues de bord (m).
    pub longueur_onde: f32,
    /// Hauteur du jet de rive (m).
    pub jet_de_rive: f32,
    /// Taille d'une case de la grille (m).
    pub cellule: f32,
    /// Cases par côté de la grille.
    pub taille: u32,
}

impl Default for ParametresRivage {
    fn default() -> Self {
        Self {
            vagues: 1.0,
            longueur_onde: 24.0,
            jet_de_rive: 0.45,
            cellule: 2.0,
            taille: 256,
        }
    }
}

/// Paramètres des vagues de bord pour l'état météo courant.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ParamsBord {
    /// Amplitude au large (m).
    pub amplitude: f32,
    /// Longueur d'onde (m).
    pub longueur: f32,
    /// Hauteur du jet de rive (m).
    pub jet: f32,
    /// Pulsation (rad/s), quantifiée pour boucler sur la période de la houle.
    pub omega: f32,
}

/// Paramètres des vagues de bord : plus de vent, plus de vagues (formules de SkyForge).
pub fn params_bord(etat: &EtatMeteo, p: &ParametresRivage) -> ParamsBord {
    let u = etat.vent_vitesse.max(0.0);
    let longueur = p.longueur_onde.max(1.0);
    ParamsBord {
        amplitude: p.vagues.max(0.0) * (0.25 + u * 0.035).clamp(0.2, 1.4),
        longueur,
        jet: p.jet_de_rive.max(0.0) * (0.4 + u * 0.05).min(1.0),
        omega: quantifier_pulsation((G * std::f32::consts::TAU / longueur).sqrt()),
    }
}

/// Marge de sable humide au-dessus du plus haut jet de rive (m) : remontée capillaire, vagues irrégulières.
pub const MARGE_SABLE_HUMIDE: f32 = 0.1;

/// Hauteur au-dessus du niveau de la mer (m) jusqu'où l'eau des vagues de bord mouille le sable : hauteur du jet
/// de rive plus une marge. Le matériau météo assombrit le sable en dessous, en fondu sur les 25 derniers
/// centimètres (`materiau_meteo.wesl`).
pub fn hauteur_mouillee(b: &ParamsBord) -> f32 {
    b.jet.max(0.0) + MARGE_SABLE_HUMIDE
}

/// Ce que la grille sait d'un point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EchantillonRivage {
    /// Hauteur du sol (m).
    pub fond: f32,
    /// Distance signée au rivage (m) : négative en mer, positive à terre.
    pub distance: f32,
    /// Normale horizontale du rivage, orientée vers la terre.
    pub normale: Vec2,
    /// Temps (s) que met une vague de bord pour aller de ce point au rivage (négatif à terre).
    pub trajet: f32,
    /// Hauteur du sol (m) un peu plus au large ([`PART_AMONT`] × la longueur d'onde, vers la mer) : la vague
    /// qui arrive ici a encore la hauteur que cette profondeur-là lui permettait.
    pub fond_amont: f32,
}

/// Grille du fond autour d'un centre.
#[derive(Debug, Clone, PartialEq)]
pub struct GrilleRivage {
    origine: Vec2,
    cellule: f32,
    n: u32,
    niveau: f32,
    fond: Vec<f32>,
    distance: Vec<f32>,
    normale: Vec<Vec2>,
    trajet: Vec<f32>,
    amont: Vec<f32>,
}

/// Distance vers le large, en part de la longueur d'onde, où l'on lit la profondeur qui borne la hauteur de la
/// vague. Une vague ne perd pas sa hauteur à l'instant où le fond remonte : sur une plage raide elle arrive
/// encore haute au bord et s'y écrase (« shore break »).
pub const PART_AMONT: f32 = 0.2;

/// Longueur d'onde des vagues de bord par défaut (m), voir [`ParametresRivage::longueur_onde`].
pub const LONGUEUR_ONDE_DEFAUT: f32 = 24.0;
/// Profondeur minimale (m) pour la célérité des vagues : en dessous, l'eau n'est plus qu'une lame sur le sable.
pub const PROFONDEUR_MIN_CELERITE: f32 = 0.15;

/// Célérité au large (m/s) d'une vague de longueur d'onde `longueur` (dispersion en eau profonde).
pub fn celerite_large(longueur: f32) -> f32 {
    let l = if longueur.is_finite() {
        longueur.max(1.0)
    } else {
        LONGUEUR_ONDE_DEFAUT
    };
    ops::sqrt(G * l / std::f32::consts::TAU)
}

/// Célérité (m/s) d'une vague de bord par `profondeur` mètres de fond : en eau peu profonde elle ne dépend que
/// de la profondeur (√(g h)) ; elle ne dépasse pas sa célérité au large. C'est ce ralentissement qui resserre
/// les vagues et les dresse avant qu'elles cassent.
pub fn celerite(profondeur: f32, longueur: f32) -> f32 {
    ops::sqrt(G * profondeur.max(PROFONDEUR_MIN_CELERITE)).min(celerite_large(longueur))
}

/// Temps de trajet (s) d'une vague de chaque case de mer jusqu'au rivage : équation eikonale |∇τ| = 1/c résolue
/// par marche rapide (Sethian) depuis les cases de mer qui touchent la terre. À terre, le temps est négatif
/// (prolongement à la célérité du large), pour que la phase reste continue au bord. `inverse_c` : 1/c par case.
fn temps_de_trajet(
    distance: &[f32],
    terre: &[bool],
    inverse_c: &[f32],
    n: usize,
    cellule: f32,
    c0: f32,
) -> Vec<f32> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    #[derive(PartialEq)]
    struct Cle(f32);
    impl Eq for Cle {}
    impl PartialOrd for Cle {
        fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(o))
        }
    }
    impl Ord for Cle {
        fn cmp(&self, o: &Self) -> std::cmp::Ordering {
            self.0.total_cmp(&o.0)
        }
    }
    let mut tau = vec![f32::INFINITY; n * n];
    let mut fige = vec![false; n * n];
    let mut tas = BinaryHeap::new();
    let voisins = |i: usize| {
        let (x, y) = (i % n, i / n);
        [
            (x > 0).then(|| i - 1),
            (x + 1 < n).then(|| i + 1),
            (y > 0).then(|| i - n),
            (y + 1 < n).then(|| i + n),
        ]
    };
    for i in 0..n * n {
        if !terre[i] && voisins(i).into_iter().flatten().any(|j| terre[j]) {
            tau[i] = (-distance[i]).max(0.0) * inverse_c[i];
            tas.push(Reverse((Cle(tau[i]), i)));
        }
    }
    while let Some(Reverse((Cle(t), i))) = tas.pop() {
        if fige[i] || t > tau[i] {
            continue;
        }
        fige[i] = true;
        for j in voisins(i).into_iter().flatten() {
            if terre[j] || fige[j] {
                continue;
            }
            let v = voisins(j);
            let pris = |k: Option<usize>| k.filter(|&k| !terre[k]).map_or(f32::INFINITY, |k| tau[k]);
            let a = pris(v[0]).min(pris(v[1]));
            let b = pris(v[2]).min(pris(v[3]));
            let f = cellule * inverse_c[j];
            let (petit, grand) = if a < b { (a, b) } else { (b, a) };
            let nouveau = if grand - petit >= f {
                petit + f
            } else {
                0.5 * (a + b + ops::sqrt(2.0 * f * f - (a - b) * (a - b)))
            };
            if nouveau < tau[j] {
                tau[j] = nouveau;
                tas.push(Reverse((Cle(nouveau), j)));
            }
        }
    }
    (0..n * n)
        .map(|i| {
            if terre[i] {
                // À terre : comme si la vague continuait à la célérité du large.
                if distance[i].abs() < LOIN_DU_RIVAGE * 0.5 {
                    -distance[i] / c0
                } else {
                    0.0
                }
            } else if tau[i].is_finite() {
                tau[i]
            } else {
                0.0
            }
        })
        .collect()
}

const TRES_LOIN: f64 = 1e9;

/// Transformée de distance 1D au carré (Felzenszwalb et Huttenlocher 2012), en place. Les cases sans cible valent
/// [`TRES_LOIN`] (fini, pour garder l'algorithme simple ; calcul en f64 pour ne pas perdre q²).
fn tdc_1d(f: &mut [f64], v: &mut [usize], z: &mut [f64], d: &mut [f64]) {
    let n = f.len();
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    for q in 1..n {
        let fq = f[q] + (q * q) as f64;
        let mut s;
        loop {
            let p = v[k];
            s = (fq - (f[p] + (p * p) as f64)) / (2.0 * (q as f64 - p as f64));
            if s <= z[k] && k > 0 {
                k -= 1;
            } else {
                break;
            }
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::INFINITY;
    }
    let mut k = 0usize;
    for (q, dq) in d.iter_mut().enumerate().take(n) {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let e = q as f64 - v[k] as f64;
        *dq = e * e + f[v[k]];
    }
    f.copy_from_slice(&d[..n]);
}

/// Distance euclidienne (en cases) de chaque case à la plus proche case `cible` (infinie s'il n'y en a pas).
fn transformee_distance(cible: &[bool], n: usize) -> Vec<f32> {
    let mut g: Vec<f64> = cible.iter().map(|&c| if c { 0.0 } else { TRES_LOIN }).collect();
    let mut f = vec![0.0; n];
    let mut v = vec![0usize; n];
    let mut z = vec![0.0; n + 1];
    let mut d = vec![0.0; n];
    for y in 0..n {
        tdc_1d(&mut g[y * n..(y + 1) * n], &mut v, &mut z, &mut d);
    }
    for x in 0..n {
        for y in 0..n {
            f[y] = g[y * n + x];
        }
        tdc_1d(&mut f, &mut v, &mut z, &mut d);
        for y in 0..n {
            g[y * n + x] = f[y];
        }
    }
    g.into_iter()
        .map(|x| {
            if x >= TRES_LOIN * 0.5 {
                f32::INFINITY
            } else {
                x.sqrt() as f32
            }
        })
        .collect()
}

impl GrilleRivage {
    /// Origine (coin −X −Z) de la grille pour un centre : alignée sur des pas d'un quart de grille, pour ne la
    /// reconstruire que quand le centre s'est vraiment déplacé.
    pub fn origine_pour(centre: Vec2, cellule: f32, n: u32) -> Vec2 {
        let taille = cellule * n as f32;
        let pas = (taille * 0.25).max(cellule);
        (centre / pas).round() * pas - Vec2::splat(taille * 0.5)
    }

    /// Construit la grille pour des vagues de longueur d'onde [`LONGUEUR_ONDE_DEFAUT`], voir
    /// [`GrilleRivage::construire_pour`].
    pub fn construire(
        fond: &dyn Fn(f32, f32) -> f32,
        centre: Vec2,
        niveau: f32,
        cellule: f32,
        n: u32,
    ) -> Self {
        Self::construire_pour(fond, centre, niveau, cellule, n, LONGUEUR_ONDE_DEFAUT)
    }

    /// Construit la grille : fond aux centres des cases, distance signée et normale du rivage au niveau `niveau`,
    /// temps de trajet jusqu'au rivage des vagues de bord de longueur d'onde `longueur` au large.
    pub fn construire_pour(
        fond: &dyn Fn(f32, f32) -> f32,
        centre: Vec2,
        niveau: f32,
        cellule: f32,
        n: u32,
        longueur: f32,
    ) -> Self {
        let cellule = if cellule.is_finite() {
            cellule.max(0.1)
        } else {
            2.0
        };
        let n = n.clamp(4, 1024);
        let nu = n as usize;
        let origine = Self::origine_pour(centre, cellule, n);
        let mut h = Vec::with_capacity(nu * nu);
        for y in 0..nu {
            for x in 0..nu {
                let p = origine + (Vec2::new(x as f32, y as f32) + 0.5) * cellule;
                let v = fond(p.x, p.y);
                h.push(if v.is_finite() { v } else { niveau - 1000.0 });
            }
        }
        let terre: Vec<bool> = h.iter().map(|&v| v >= niveau).collect();
        let mer: Vec<bool> = terre.iter().map(|&t| !t).collect();
        let vers_terre = transformee_distance(&terre, nu);
        let vers_mer = transformee_distance(&mer, nu);
        // Le rivage passe entre les centres de deux cases voisines : on retire une demi-case.
        let distance: Vec<f32> = (0..nu * nu)
            .map(|i| {
                if terre[i] {
                    if vers_mer[i].is_finite() {
                        (vers_mer[i] - 0.5) * cellule
                    } else {
                        LOIN_DU_RIVAGE
                    }
                } else if vers_terre[i].is_finite() {
                    -(vers_terre[i] - 0.5) * cellule
                } else {
                    -LOIN_DU_RIVAGE
                }
            })
            .collect();
        let at = |x: usize, y: usize| distance[y.min(nu - 1) * nu + x.min(nu - 1)];
        let mut normale = Vec::with_capacity(nu * nu);
        for y in 0..nu {
            for x in 0..nu {
                let gx = at(x + 1, y) - at(x.saturating_sub(1), y);
                let gz = at(x, y + 1) - at(x, y.saturating_sub(1));
                normale.push(Vec2::new(gx, gz).normalize_or_zero());
            }
        }
        let c0 = celerite_large(longueur);
        let inverse_c: Vec<f32> = h.iter().map(|&v| 1.0 / celerite(niveau - v, longueur)).collect();
        let trajet = temps_de_trajet(&distance, &terre, &inverse_c, nu, cellule, c0);
        // Fond lu vers le large, le long de la normale (bilinéaire, borné à la grille).
        let recul = PART_AMONT * longueur.max(1.0) / cellule;
        let max = (nu - 1) as f32;
        let mut amont = Vec::with_capacity(nu * nu);
        for y in 0..nu {
            for x in 0..nu {
                let q = (Vec2::new(x as f32, y as f32) - normale[y * nu + x] * recul)
                    .clamp(Vec2::ZERO, Vec2::splat(max));
                let i0 = q.floor();
                let t = q - i0;
                let (x0, y0) = (i0.x as usize, i0.y as usize);
                let (x1, y1) = ((x0 + 1).min(nu - 1), (y0 + 1).min(nu - 1));
                let a = h[y0 * nu + x0] + (h[y0 * nu + x1] - h[y0 * nu + x0]) * t.x;
                let b = h[y1 * nu + x0] + (h[y1 * nu + x1] - h[y1 * nu + x0]) * t.x;
                amont.push(a + (b - a) * t.y);
            }
        }
        Self {
            origine,
            cellule,
            n,
            niveau,
            fond: h,
            distance,
            normale,
            trajet,
            amont,
        }
    }

    /// Coin −X −Z de la grille (m).
    pub fn origine(&self) -> Vec2 {
        self.origine
    }

    /// Côté de la grille (m).
    pub fn taille_m(&self) -> f32 {
        self.cellule * self.n as f32
    }

    /// Cases par côté.
    pub fn n(&self) -> u32 {
        self.n
    }

    /// Niveau de la mer utilisé pour la construction.
    pub fn niveau(&self) -> f32 {
        self.niveau
    }

    /// Données pour le GPU : une image de `2n × n` texels, ligne par ligne depuis −Z. Moitié gauche : (fond,
    /// distance, normale.x, normale.z) ; moitié droite : (temps de trajet, fond en amont, 0, 0).
    pub fn donnees_gpu(&self) -> Vec<[f32; 4]> {
        let nu = self.n as usize;
        let mut donnees = Vec::with_capacity(2 * nu * nu);
        for y in 0..nu {
            for x in 0..nu {
                let i = y * nu + x;
                donnees.push([
                    self.fond[i],
                    self.distance[i],
                    self.normale[i].x,
                    self.normale[i].y,
                ]);
            }
            for x in 0..nu {
                donnees.push([self.trajet[y * nu + x], self.amont[y * nu + x], 0.0, 0.0]);
            }
        }
        donnees
    }

    /// Échantillon bilinéaire en (x, z), `None` hors de la grille.
    pub fn echantillonner(&self, p: Vec2) -> Option<EchantillonRivage> {
        let nu = self.n as usize;
        let q = (p - self.origine) / self.cellule - 0.5;
        if !q.is_finite() || q.x < -0.5 || q.y < -0.5 || q.x > nu as f32 - 0.5 || q.y > nu as f32 - 0.5 {
            return None;
        }
        let max = (nu - 1) as f32;
        let q = q.clamp(Vec2::ZERO, Vec2::splat(max));
        let i0 = q.floor();
        let t = q - i0;
        let (x0, y0) = (i0.x as usize, i0.y as usize);
        let (x1, y1) = ((x0 + 1).min(nu - 1), (y0 + 1).min(nu - 1));
        let idx = |x: usize, y: usize| y * nu + x;
        let bil = |v: &[f32]| {
            let a = v[idx(x0, y0)] + (v[idx(x1, y0)] - v[idx(x0, y0)]) * t.x;
            let b = v[idx(x0, y1)] + (v[idx(x1, y1)] - v[idx(x0, y1)]) * t.x;
            a + (b - a) * t.y
        };
        let n = {
            let a = self.normale[idx(x0, y0)].lerp(self.normale[idx(x1, y0)], t.x);
            let b = self.normale[idx(x0, y1)].lerp(self.normale[idx(x1, y1)], t.x);
            a.lerp(b, t.y).normalize_or_zero()
        };
        Some(EchantillonRivage {
            fond: bil(&self.fond),
            distance: bil(&self.distance),
            normale: n,
            trajet: bil(&self.trajet),
            fond_amont: bil(&self.amont),
        })
    }
}

/// Paramètres d'une grille à construire (voir [`CarteRivage::demande`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DemandeGrille {
    /// Coin −X −Z de la grille (m), aligné sur un quart de grille.
    pub origine: Vec2,
    /// Centre demandé (position de la caméra).
    pub centre: Vec2,
    /// Niveau de la mer (m).
    pub niveau: f32,
    /// Taille d'une case (m).
    pub cellule: f32,
    /// Cases par côté.
    pub n: u32,
    /// Longueur d'onde des vagues de bord au large (m).
    pub longueur: f32,
}

impl DemandeGrille {
    /// Construit la grille demandée (coûteux : à faire hors de l'image si possible).
    pub fn construire(&self, fond: &dyn Fn(f32, f32) -> f32) -> GrilleRivage {
        GrilleRivage::construire_pour(
            fond,
            self.centre,
            self.niveau,
            self.cellule,
            self.n,
            self.longueur,
        )
    }

    /// Vrai si la grille `g` répond à cette demande.
    pub fn correspond(&self, g: &GrilleRivage) -> bool {
        g.n == self.n
            && g.cellule == self.cellule
            && (g.niveau - self.niveau).abs() < 0.01
            && g.origine == self.origine
    }

    /// Vrai si deux demandes donnent la même grille.
    pub fn meme_grille(&self, autre: &Self) -> bool {
        self.n == autre.n
            && self.cellule == autre.cellule
            && (self.niveau - autre.niveau).abs() < 0.01
            && self.origine == autre.origine
    }
}

/// Grille courante (reconstruite par [`CarteRivage::mettre_a_jour`]).
#[derive(Resource, Debug, Clone, Default)]
pub struct CarteRivage {
    grille: Option<GrilleRivage>,
    generation: u32,
}

impl CarteRivage {
    /// Grille actuelle, s'il y a une source de fond.
    pub fn grille(&self) -> Option<&GrilleRivage> {
        self.grille.as_ref()
    }

    /// Augmente à chaque reconstruction (pour renvoyer la texture au GPU).
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Reconstruit la grille si le centre a changé de pas, si le niveau ou les réglages ont changé, ou si
    /// `forcer` (nouvelle source). Renvoie vrai en cas de reconstruction. Calcul sur place : pour ne pas
    /// bloquer l'image, le rendu passe par [`CarteRivage::demande`] et construit la grille en tâche de fond.
    pub fn mettre_a_jour(
        &mut self,
        source: Option<&SourceFond>,
        centre: Vec2,
        niveau: f32,
        p: &ParametresRivage,
        forcer: bool,
    ) -> bool {
        let Some(source) = source else {
            return self.installer(None);
        };
        match self.demande(centre, niveau, p, forcer) {
            Some(d) => self.installer(Some(d.construire(source.0.as_ref()))),
            None => false,
        }
    }

    /// Grille à construire pour ce centre, `None` si la grille actuelle convient (et pas `forcer`).
    pub fn demande(
        &self,
        centre: Vec2,
        niveau: f32,
        p: &ParametresRivage,
        forcer: bool,
    ) -> Option<DemandeGrille> {
        let n = p.taille.clamp(4, 1024);
        let cellule = if p.cellule.is_finite() {
            p.cellule.max(0.1)
        } else {
            2.0
        };
        let d = DemandeGrille {
            origine: GrilleRivage::origine_pour(centre, cellule, n),
            centre,
            niveau,
            cellule,
            n,
            longueur: p.longueur_onde,
        };
        let a_jour = self.grille.as_ref().is_some_and(|g| d.correspond(g));
        (forcer || !a_jour).then_some(d)
    }

    /// Remplace la grille (`None` : plus de rivage). Renvoie vrai si quelque chose a changé.
    pub fn installer(&mut self, grille: Option<GrilleRivage>) -> bool {
        if grille.is_none() && self.grille.is_none() {
            return false;
        }
        self.grille = grille;
        self.generation = self.generation.wrapping_add(1);
        true
    }

    /// Échantillon en (x, z), `None` sans grille ou hors grille.
    pub fn echantillonner(&self, p: Vec2) -> Option<EchantillonRivage> {
        self.grille.as_ref()?.echantillonner(p)
    }
}

/// Effet des vagues de bord en un point.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct InfoBord {
    /// Déplacement (m).
    pub deplacement: Vec3,
    /// Pente de la surface (dy/dx, dy/dz).
    pub pente: Vec2,
    /// Écume de déferlement (0..1).
    pub ecume: f32,
    /// Crête en train de déferler (0..1) : là où naissent les embruns.
    pub crete: f32,
}

/// Ondulation des crêtes le long du rivage (rad), même bruit que le shader.
pub fn ondulation_crete(p: Vec2) -> f32 {
    perlin3(Vec3::new(p.x * 0.015, 1.3, p.y * 0.015), 0) * 4.0
}

/// Phase des vagues de bord : les crêtes avancent vers la terre. `trajet` : temps (s) qu'il reste à la vague
/// pour atteindre le rivage ([`EchantillonRivage::trajet`]) ; une crête garde une phase constante.
pub fn phase_bord(p: Vec2, trajet: f32, b: &ParamsBord, t: f32) -> f32 {
    -b.omega * trajet + ondulation_crete(p) - b.omega * t
}

fn lisse(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Vagues de bord en `p` au temps `t` (s). `profondeur` = niveau − fond.
pub fn vagues_bord(p: Vec2, e: &EchantillonRivage, profondeur: f32, b: &ParamsBord, t: f32) -> InfoBord {
    let d = e.distance;
    let enveloppe = lisse(-170.0, -70.0, d) * (1.0 - lisse(15.0, 35.0, d));
    if enveloppe <= 0.0 || b.amplitude <= 0.0 || e.normale == Vec2::ZERO {
        return InfoBord::default();
    }
    let n = e.normale;
    // Nombre d'onde local : la vague ralentit et se resserre quand le fond remonte.
    let k = b.omega / celerite(profondeur, b.longueur);
    let th = phase_bord(p, e.trajet, b, t);
    let h = profondeur.max(0.05);
    // Levée de la houle (loi de Green), bornée.
    let mut a = b.amplitude * ops::powf(4.0 / h, 0.25).clamp(0.6, 1.7);
    // Limite de déferlement (McCowan : H/h = 0,78).
    let a_max = 0.39 * h + 0.03;
    let deferlement = ((a / a_max - 0.7) * 3.0).clamp(0.0, 1.0);
    // La hauteur est bornée par la profondeur un peu plus au large (la vague garde son élan), puis la vague
    // s'écrase sur les 30 derniers centimètres d'eau.
    let h_amont = h.max(profondeur + e.fond - e.fond_amont);
    // Hauteur inégale le long de la crête : la vague casse par sections, pas d'un bloc.
    let section = 0.85 + 0.15 * perlin3(Vec3::new(p.x * 0.07, 7.1, p.y * 0.07), 0);
    a = a.min(0.39 * h_amont + 0.03) * section * lisse(0.0, 0.3, profondeur) * enveloppe;
    // Rouleau : crête pointue et creux plat en eau peu profonde, face dressée vers la plage quand la vague
    // déferle (mêmes formules que `vagues_bord` dans `ocean.wesl`, sans le bouillonnement de l'écume).
    let raide = (1.0 - h / 3.0).clamp(0.0, 1.0);
    let q = 0.35 + (0.95 - 0.35) * raide;
    let (s, c) = ops::sin_cos(th);
    let n_p = 1.0 + 3.0 * raide;
    let base = 0.5 + 0.5 * c;
    let pc = ops::powf(base, n_p);
    let dpc = -0.5 * n_p * ops::powf(base, n_p - 1.0) * s;
    let moyenne2 = ops::powf(n_p, -0.44);
    let avancee = 0.55 * deferlement * raide * (a / 0.25).clamp(0.0, 1.0) / (k * 0.89);
    let horizontal = n * (avancee * pc - q * a * s);
    let mut deplacement = Vec3::new(horizontal.x, a * (2.0 * pc - moyenne2), horizontal.y);
    let etirement = (1.0 + k * (avancee * dpc - q * a * c)).max(0.12);
    let pente = n * (2.0 * a * k * dpc / etirement);
    // Jet de rive : tout le bord monte et descend au rythme des vagues qui arrivent.
    let th_rive = phase_bord(p, 0.0, b, t);
    let ressac = ops::powf((0.5 + 0.5 * ops::cos(th_rive)).clamp(0.0, 1.0), 1.5);
    deplacement.y += b.jet * ressac * lisse(-70.0, -20.0, d);
    // Écume : la crête qui déferle, la face avant du rouleau, puis une traînée qui s'efface derrière.
    let crete = ops::powf(c.clamp(0.0, 1.0), 6.0);
    let face = if s >= 0.0 {
        let ecrase = 1.0 - lisse(0.35, 1.0, profondeur);
        lisse(0.3 - 0.28 * ecrase, 0.62 - 0.42 * ecrase, pc) * deferlement
    } else {
        0.0
    };
    let derriere = (-th / std::f32::consts::TAU).rem_euclid(1.0);
    let trainee = ops::exp(-derriere * 7.0);
    InfoBord {
        deplacement,
        pente,
        ecume: deferlement * crete.max(face).max(trainee * 0.8) * enveloppe,
        // Comme la masse d'écume du shader : seulement là où la vague a encore de la hauteur.
        crete: deferlement * crete * enveloppe * lisse(0.1, 0.3, a),
    }
}

/// Atténuation de la houle du large en eau peu profonde : la rampe de SkyForge (12 % à 0 m, 100 % à 6 m), et en
/// plus, ajout par rapport à SkyForge, la houle ne dépasse pas la limite de déferlement de McCowan (0,39 h,
/// marge 0,35 h) : sans cela, les creux d'une houle de tempête découvraient le fond à 10 m de profondeur.
/// `amplitude` = amplitude crête totale de la houle (m), voir [`amplitude_houle`].
pub fn attenuation_houle(profondeur: f32, amplitude: f32) -> f32 {
    let rampe = 0.12 + 0.88 * (profondeur / 6.0).clamp(0.0, 1.0);
    let limite = (0.35 * profondeur.max(0.0) + 0.03) / amplitude.max(1e-3);
    rampe.min(limite).clamp(0.0, 1.0)
}

/// Amplitude crête totale de la houle utilisée pour l'atténuation : somme des amplitudes de Gerstner, ×1,5 pour
/// les cascades FFT (qui ne sont pas calculées sur le CPU).
pub fn amplitude_houle(ocean: &Ocean) -> f32 {
    ocean.vagues().iter().map(|v| v.amplitude).sum::<f32>() * 1.5
}

/// Déplacement total de la surface au repos `p` : houle (Gerstner + mer du vent, si `mer` est donnée) atténuée par
/// la profondeur, plus les vagues de bord. Sans grille (ou hors grille), c'est la houle seule.
pub fn deplacement_total(
    ocean: &Ocean,
    mer: Option<&MerDuVent>,
    carte: &CarteRivage,
    b: &ParamsBord,
    p: Vec2,
) -> Vec3 {
    let mut houle = deplacement(ocean.vagues(), p, ocean.temps());
    if let Some(mer) = mer {
        houle += mer.deplacement(p);
    }
    let Some(e) = carte.echantillonner(p) else {
        return houle;
    };
    let profondeur = ocean.niveau - e.fond;
    houle * attenuation_houle(profondeur, amplitude_houle(ocean))
        + vagues_bord(p, &e, profondeur, b, ocean.temps()).deplacement
}

/// Point de la surface au repos qui, une fois déplacé, arrive à la verticale de `cible` ; renvoie ce point et son
/// déplacement. Point fixe amorti (×0,7), arrêté dès que l'écart passe sous 1 cm, au plus 12 itérations : en
/// tempête, les vagues pointues de la mer du vent faisaient diverger le point fixe simple (mesuré : jusqu'à 2 m
/// d'écart horizontal après 4 itérations, 1,2 cm au pire avec l'amortissement).
pub fn repos_sous(
    ocean: &Ocean,
    mer: Option<&MerDuVent>,
    carte: &CarteRivage,
    b: &ParamsBord,
    cible: Vec2,
) -> (Vec2, Vec3) {
    let mut repos = cible;
    let mut d = deplacement_total(ocean, mer, carte, b, repos);
    for _ in 0..12 {
        let residu = repos + Vec2::new(d.x, d.z) - cible;
        if residu.length_squared() < 1e-4 {
            break;
        }
        repos -= residu * 0.7;
        d = deplacement_total(ocean, mer, carte, b, repos);
    }
    (repos, d)
}

/// Hauteur de l'eau à la verticale de (x, z), vagues de bord comprises (pour la flottaison). Le déplacement
/// horizontal est inversé par [`repos_sous`]. Avec `mer`, les cascades FFT de la mer du vent sont incluses
/// (réplique CPU du GPU, voir [`crate::mer_du_vent`]).
pub fn hauteur_eau(
    ocean: &Ocean,
    mer: Option<&MerDuVent>,
    carte: &CarteRivage,
    b: &ParamsBord,
    x: f32,
    z: f32,
) -> f32 {
    ocean.niveau + repos_sous(ocean, mer, carte, b, Vec2::new(x, z)).1.y
}
