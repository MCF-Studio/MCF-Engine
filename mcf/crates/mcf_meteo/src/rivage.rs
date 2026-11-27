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

    /// Construit la grille : fond aux centres des cases, distance signée et normale du rivage au niveau `niveau`.
    pub fn construire(
        fond: &dyn Fn(f32, f32) -> f32,
        centre: Vec2,
        niveau: f32,
        cellule: f32,
        n: u32,
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
        Self {
            origine,
            cellule,
            n,
            niveau,
            fond: h,
            distance,
            normale,
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

    /// Données par case pour le GPU : (fond, distance, normale.x, normale.z), ligne par ligne depuis −Z.
    pub fn donnees_gpu(&self) -> Vec<[f32; 4]> {
        (0..self.fond.len())
            .map(|i| {
                [
                    self.fond[i],
                    self.distance[i],
                    self.normale[i].x,
                    self.normale[i].y,
                ]
            })
            .collect()
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
        })
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
    /// `forcer` (nouvelle source). Renvoie vrai en cas de reconstruction.
    pub fn mettre_a_jour(
        &mut self,
        source: Option<&SourceFond>,
        centre: Vec2,
        niveau: f32,
        p: &ParametresRivage,
        forcer: bool,
    ) -> bool {
        let Some(source) = source else {
            if self.grille.take().is_some() {
                self.generation = self.generation.wrapping_add(1);
                return true;
            }
            return false;
        };
        let n = p.taille.clamp(4, 1024);
        let cellule = if p.cellule.is_finite() {
            p.cellule.max(0.1)
        } else {
            2.0
        };
        let a_jour = self.grille.as_ref().is_some_and(|g| {
            g.n == n
                && g.cellule == cellule
                && (g.niveau - niveau).abs() < 0.01
                && g.origine == GrilleRivage::origine_pour(centre, cellule, n)
        });
        if a_jour && !forcer {
            return false;
        }
        self.grille = Some(GrilleRivage::construire(
            source.0.as_ref(),
            centre,
            niveau,
            cellule,
            n,
        ));
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
}

/// Ondulation des crêtes le long du rivage (rad), même bruit que le shader.
pub fn ondulation_crete(p: Vec2) -> f32 {
    perlin3(Vec3::new(p.x * 0.015, 1.3, p.y * 0.015), 0) * 4.0
}

/// Phase des vagues de bord : les crêtes avancent vers la terre (distance signée croissante).
pub fn phase_bord(p: Vec2, distance: f32, b: &ParamsBord, t: f32) -> f32 {
    let k = std::f32::consts::TAU / b.longueur.max(1.0);
    k * distance + ondulation_crete(p) - b.omega * t
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
    let k = std::f32::consts::TAU / b.longueur.max(1.0);
    let th = phase_bord(p, d, b, t);
    let h = profondeur.max(0.05);
    // Levée de la houle (loi de Green), bornée.
    let mut a = b.amplitude * ops::powf(4.0 / h, 0.25).clamp(0.6, 1.7);
    // Limite de déferlement (McCowan : H/h = 0,78).
    let a_max = 0.39 * h + 0.03;
    let deferlement = ((a / a_max - 0.7) * 3.0).clamp(0.0, 1.0);
    a = a.min(a_max) * enveloppe;
    // La crête se penche vers la plage en eau peu profonde.
    let q = 0.35 + (0.95 - 0.35) * (1.0 - h / 3.0).clamp(0.0, 1.0);
    let (s, c) = ops::sin_cos(th);
    let horizontal = -n * (q * a * s);
    let mut deplacement = Vec3::new(horizontal.x, a * c, horizontal.y);
    let pente = n * (-a * k * s);
    // Jet de rive : tout le bord monte et descend au rythme des vagues qui arrivent.
    let th_rive = phase_bord(p, 0.0, b, t);
    let ressac = ops::powf((0.5 + 0.5 * ops::cos(th_rive)).clamp(0.0, 1.0), 1.5);
    deplacement.y += b.jet * ressac * lisse(-70.0, -20.0, d);
    // Écume : sur la crête qui déferle, puis une traînée qui s'efface derrière elle.
    let crete = ops::powf(c.clamp(0.0, 1.0), 6.0);
    let derriere = (-th / std::f32::consts::TAU).rem_euclid(1.0);
    let trainee = ops::exp(-derriere * 5.0);
    InfoBord {
        deplacement,
        pente,
        ecume: deferlement * crete.max(trainee * 0.85) * enveloppe,
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
