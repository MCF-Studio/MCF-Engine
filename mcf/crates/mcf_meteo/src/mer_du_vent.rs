//! Mer du vent sur le CPU : réplique des cascades FFT du GPU ([`crate::ocean_fft`]) pour la flottaison.
//!
//! Le spectre du GPU est entièrement déterministe (graines par hachage du texel, fréquences quantifiées, temps de
//! [`Ocean`]) : on le reconstruit ici à l'identique, sans aucune dépendance au rendu. Le serveur dédié obtient
//! donc la même surface que les clients, sans GPU ni relecture.
//!
//! Plutôt qu'une FFT complète (3 × 256², inutile pour quelques objets), on garde les `modes_flottaison` modes les
//! plus énergétiques et on les somme directement au point demandé. La part d'énergie gardée est donnée par
//! [`MerDuVent::energie_gardee`].
//!
//! Écarts connus avec l'image : la plus petite cascade (vagues de moins de 1,5 m) est limitée aux nombres d'onde
//! jusqu'à 22 rad/m (rides de plus de 28 cm), et le fondu des cascades avec la distance à la caméra (LOD du rendu)
//! n'est pas appliqué.

use std::f32::consts::{PI, TAU};

use bevy::math::ops;
use bevy::prelude::*;

use crate::etat::EtatMeteo;
use crate::houle::{Ocean, pic_jonswap};

/// Résolution d'une cascade (identique aux shaders).
pub const N_FFT: u32 = 256;
/// Nombre de cascades.
pub const CASCADES: u32 = 3;
const G: f32 = 9.81;
/// Plus grand indice de fréquence (en valeur absolue) parcouru pour la plus petite cascade.
const RAYON_PETITE_CASCADE: i32 = 32;

/// Réglages de la simulation FFT (partagés par le GPU et la réplique CPU).
#[derive(Resource, Reflect, Debug, Clone, PartialEq)]
#[reflect(Resource, Default)]
pub struct ReglagesFft {
    /// Taille du motif de chaque cascade (m), de la plus grande à la plus petite.
    pub tailles: [f32; 3],
    /// Déplacement horizontal (vagues pointues).
    pub choppiness: f32,
    /// Décroissance de l'écume (1/s).
    pub decroissance_ecume: f32,
    /// Les cascades FFT commencent à `debut_apres_pic × k_pic` (la houle de Gerstner porte le pic).
    pub debut_apres_pic: f32,
    /// Nombre de modes gardés pour la flottaison sur le CPU (coût par point ∝ ce nombre).
    pub modes_flottaison: u32,
}

impl Default for ReglagesFft {
    fn default() -> Self {
        Self {
            tailles: [250.0, 47.0, 9.0],
            choppiness: 1.2,
            decroissance_ecume: 0.8,
            debut_apres_pic: 1.5,
            modes_flottaison: 512,
        }
    }
}

/// Anneaux de la grille de la première cascade sautés au minimum : près de k = 0, la grille n'a que quelques
/// modes, chacun portant une large part de l'étalement directionnel ; avec un pic de tempête proche de la maille,
/// la variance de la mer du vent sortait 1,9 fois trop forte en amplitude (2,48 m RMS pour 1,81 m attendus).
/// Avec 3 anneaux : 0,68 m pour 0,62 m.
pub const ANNEAUX_MIN: f32 = 3.0;

/// Bandes de nombres d'onde des cascades : la cascade c couvre [6·2π/Lc, 6·2π/Lc+1), la première commence après
/// le pic de la houle de Gerstner (et au moins [`ANNEAUX_MIN`] mailles de sa grille).
pub fn bandes(tailles: [f32; 3], k_pic: f32, debut_apres_pic: f32) -> (Vec4, Vec4) {
    let k = |l: f32| 6.0 * TAU / l.max(0.01);
    let k_min = Vec4::new(
        (k_pic * debut_apres_pic).max(ANNEAUX_MIN * TAU / tailles[0].max(0.01)),
        k(tailles[1]),
        k(tailles[2]),
        0.0,
    );
    let k_max = Vec4::new(k(tailles[1]), k(tailles[2]), 1e9, 0.0);
    (k_min, k_max)
}

/// Paramètres du spectre (ceux que lit la passe d'initialisation du GPU).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamsSpectre {
    /// Taille du motif de chaque cascade (m).
    pub tailles: [f32; 3],
    /// Début de la bande de chaque cascade.
    pub k_min: [f32; 3],
    /// Fin de la bande de chaque cascade.
    pub k_max: [f32; 3],
    /// Direction du vent (plan XZ, normée).
    pub vent: Vec2,
    /// Vitesse du vent à 10 m (m/s), au moins 0,5.
    pub u10: f32,
    /// Fetch (m), au moins 1 km.
    pub fetch: f32,
}

impl ParamsSpectre {
    /// Paramètres pour la météo et les réglages donnés (fonction pure).
    pub fn depuis(etat: &EtatMeteo, reglages: &ReglagesFft) -> Self {
        let (longueur_pic, _) = pic_jonswap(etat.vent_vitesse, etat.fetch_km);
        let k_pic = TAU / longueur_pic.max(0.01);
        let (k_min, k_max) = bandes(reglages.tailles, k_pic, reglages.debut_apres_pic);
        let d = etat.direction_vent();
        Self {
            tailles: reglages.tailles,
            k_min: [k_min.x, k_min.y, k_min.z],
            k_max: [k_max.x, k_max.y, k_max.z],
            vent: Vec2::new(d.x, d.z).normalize_or(Vec2::NEG_Y),
            u10: etat.vent_vitesse.max(0.5),
            fetch: (etat.fetch_km * 1000.0).max(1000.0),
        }
    }
}

/// Décide quand recalculer le spectre initial (vent ou fetch qui changent notablement). Même règle pour le GPU et
/// le CPU : les deux recalculent aux mêmes images, avec les mêmes paramètres.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SuiviSpectre {
    /// Augmente à chaque recalcul (0 = jamais calculé).
    pub generation: u32,
    dernier_u10: f32,
    dernier_angle: f32,
    dernier_fetch: f32,
}

impl SuiviSpectre {
    /// Renvoie vrai (et change de génération) si le spectre doit être recalculé.
    pub fn avancer(&mut self, u10: f32, vent: Vec2, fetch: f32) -> bool {
        let angle = ops::atan2(vent.y, vent.x);
        let change = self.generation == 0
            || (u10 - self.dernier_u10).abs() > 0.25
            || (angle - self.dernier_angle).abs() > 0.035
            || (fetch - self.dernier_fetch).abs() > self.dernier_fetch * 0.01;
        if change {
            self.generation = self.generation.wrapping_add(1).max(1);
            self.dernier_u10 = u10;
            self.dernier_angle = angle;
            self.dernier_fetch = fetch;
        }
        change
    }
}

// --- Copies exactes des fonctions de `ocean_commun.wesl` et `ocean_spectre.wesl` ---------------------------------

fn hachage(x: u32) -> u32 {
    let mut x = x;
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

fn hachage3(v: UVec3) -> u32 {
    hachage(v.x ^ hachage(v.y ^ hachage(v.z)))
}

fn hachage_f(x: u32) -> f32 {
    (hachage(x) & 0x00FF_FFFF) as f32 / 16_777_216.0
}

fn gaussienne(id: UVec2, cascade: u32) -> Vec2 {
    let graine = hachage3(UVec3::new(id.x, id.y, cascade * 7919 + 17));
    let u1 = hachage_f(graine).max(1e-6);
    let u2 = hachage_f(graine ^ 0xA511_E9B3);
    let r = (-2.0 * ops::ln(u1)).sqrt();
    let (s, c) = ops::sin_cos(TAU * u2);
    Vec2::new(r * c, r * s)
}

fn jonswap(p: &ParamsSpectre, w: f32) -> f32 {
    let u = p.u10.max(0.5);
    let f = p.fetch.max(1000.0);
    let alpha = 0.076 * ops::powf(u * u / (f * G), 0.22);
    let wp = 22.0 * ops::powf(G * G / (u * f), 1.0 / 3.0);
    let gamma: f32 = 3.3;
    let sigma = if w <= wp { 0.07 } else { 0.09 };
    let r = ops::exp(-(w - wp) * (w - wp) / (2.0 * sigma * sigma * wp * wp));
    alpha * G * G / ops::powf(w, 5.0) * ops::exp(-1.25 * ops::powf(wp / w, 4.0)) * ops::powf(gamma, r)
}

fn etalement(p: &ParamsSpectre, theta: f32) -> f32 {
    let s = 4.0 + (14.0 - 4.0) * (p.u10 / 25.0).clamp(0.0, 1.0);
    let norme = (s + 0.25).sqrt() / (2.0 * PI.sqrt());
    norme * ops::powf(ops::cos(theta * 0.5).abs(), 2.0 * s)
}

fn amplitude(p: &ParamsSpectre, k: Vec2, cascade: usize) -> f32 {
    let kl = k.length();
    if kl < p.k_min[cascade].max(1e-4) || kl >= p.k_max[cascade] {
        return 0.0;
    }
    let w = (G * kl).sqrt();
    let dwdk = G / (2.0 * w);
    let theta = ops::atan2(k.y, k.x) - ops::atan2(p.vent.y, p.vent.x);
    let mut s = jonswap(p, w) * etalement(p, theta) * dwdk / kl;
    s *= ops::exp(-kl * kl * 1e-4);
    let dk = TAU / p.tailles[cascade];
    // Variance d'un mode : |h0(k)|² + |h0(−k)|² en moyenne, comptée deux fois (k et −k donnent la même partie
    // réelle). Pour que la variance de la surface soit ∫ S dk, il faut |h0|² = S dk² / 2. SkyForge prenait
    // 2 S dk² : vagues deux fois trop hautes (mesuré : 4,8 m RMS au lieu de 1,8 m en tempête).
    (0.5 * s * dk * dk).sqrt()
}

fn vecteur_onde(id: UVec2, taille: f32) -> Vec2 {
    TAU * (id.as_vec2() - (N_FFT / 2) as f32) / taille
}

/// Pulsation quantifiée : le motif boucle exactement toutes les 200 s (comme le shader).
fn pulsation(kl: f32) -> f32 {
    let w0 = TAU / 200.0;
    ((G * kl.max(1e-4)).sqrt() / w0).floor() * w0
}

fn cmul(a: Vec2, b: Vec2) -> Vec2 {
    Vec2::new(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x)
}

fn conj(a: Vec2) -> Vec2 {
    Vec2::new(a.x, -a.y)
}

/// Un mode du spectre (demi-plan : son conjugué en −k est compté par le facteur 2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mode {
    /// Vecteur d'onde (rad/m, plan XZ).
    pub k: Vec2,
    /// Pulsation quantifiée (rad/s).
    pub omega: f32,
    /// h0(k) (déjà déphasé pour la position des texels, voir [`Spectre::construire`]).
    pub h0: Vec2,
    /// conj(h0(−k)), déphasé de même.
    pub h0c: Vec2,
    /// Variance attendue de la hauteur due à ce mode et à son conjugué (m²), moyenne sur les tirages.
    pub variance_attendue: f32,
}

impl Mode {
    fn energie(&self) -> f32 {
        self.h0.length_squared() + self.h0c.length_squared()
    }

    /// Amplitude complexe à l'instant t : h0 e^{iωt} + conj(h0(−k)) e^{−iωt}.
    pub fn a_l_instant(&self, t: f32) -> Vec2 {
        let (s, c) = ops::sin_cos(self.omega * t);
        let e = Vec2::new(c, s);
        cmul(self.h0, e) + cmul(self.h0c, conj(e))
    }
}

/// Modes du spectre des 3 cascades, du plus énergétique au moins énergétique.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spectre {
    /// Modes triés par énergie décroissante.
    pub modes: Vec<Mode>,
    /// Énergie totale (somme de |h0|² + |h0c|² sur tous les modes).
    pub energie_totale: f32,
    /// Variance attendue de la hauteur (m²) : ∫ S dk discrétisé sur les grilles.
    pub variance_attendue: f32,
}

impl Spectre {
    /// Spectre complet (demi-plan) des 3 cascades.
    ///
    /// `aligner_texels` : le matériau échantillonne la texture en `xz / taille` avec un filtre linéaire, donc la
    /// valeur lue en x correspond au champ en `x − taille / (2N)` (demi-texel) ; on déphase chaque mode pour
    /// coller à l'image. Sans alignement, on obtient le champ aux centres des texels (pour comparer à la texture).
    pub fn construire(p: &ParamsSpectre, aligner_texels: bool) -> Self {
        let modes: Vec<Mode> = (0..CASCADES as usize)
            .flat_map(|c| Self::modes_cascade(p, c, aligner_texels, RAYON_PETITE_CASCADE))
            .collect();
        Self::depuis_modes(modes)
    }

    /// Spectre complet d'une seule cascade, sans alignement ni limite pour la petite cascade : le même champ que
    /// la couche `cascade` de la texture du GPU, aux centres des texels (pour la vérification).
    pub fn cascade_complete(p: &ParamsSpectre, cascade: usize) -> Self {
        Self::depuis_modes(Self::modes_cascade(p, cascade, false, N_FFT as i32 / 2 - 1))
    }

    fn depuis_modes(mut modes: Vec<Mode>) -> Self {
        modes.sort_by(|a, b| b.energie().total_cmp(&a.energie()));
        let energie_totale = modes.iter().map(Mode::energie).sum();
        let variance_attendue = modes.iter().map(|m| m.variance_attendue).sum();
        Self {
            modes,
            energie_totale,
            variance_attendue,
        }
    }

    fn modes_cascade(p: &ParamsSpectre, c: usize, aligner_texels: bool, rayon_ouvert: i32) -> Vec<Mode> {
        let mut modes = Vec::new();
        let n = N_FFT as i32;
        let Some(&taille) = p.tailles.get(c) else {
            return modes;
        };
        if !(taille.is_finite() && taille > 0.0) {
            return modes;
        }
        // Indices utiles : jusqu'à k_max (bornée pour la petite cascade, dont la bande est ouverte).
        let rayon_k = (p.k_max[c] * taille / TAU).ceil();
        let rayon = if rayon_k.is_finite() && rayon_k < (n / 2 - 1) as f32 {
            rayon_k as i32 + 1
        } else {
            rayon_ouvert
        }
        .min(n / 2 - 1);
        let decalage = Vec2::splat(-taille / (2.0 * N_FFT as f32));
        for ny in 0..=rayon {
            for nx in -rayon..=rayon {
                // Demi-plan : ny > 0, ou ny = 0 et nx > 0.
                if ny == 0 && nx <= 0 {
                    continue;
                }
                let id = UVec2::new((nx + n / 2) as u32, (ny + n / 2) as u32);
                let id_neg = UVec2::new((N_FFT - id.x) % N_FFT, (N_FFT - id.y) % N_FFT);
                let k = vecteur_onde(id, taille);
                let a = amplitude(p, k, c);
                let an = amplitude(p, -k, c);
                if a == 0.0 && an == 0.0 {
                    continue;
                }
                let mut h0 = gaussienne(id, c as u32) * a * core::f32::consts::FRAC_1_SQRT_2;
                let mut h0c = conj(gaussienne(id_neg, c as u32) * an * core::f32::consts::FRAC_1_SQRT_2);
                if aligner_texels {
                    let (s, co) = ops::sin_cos(k.dot(decalage));
                    let e = Vec2::new(co, s);
                    h0 = cmul(h0, e);
                    h0c = cmul(h0c, e);
                }
                modes.push(Mode {
                    k,
                    omega: pulsation(k.length()),
                    h0,
                    h0c,
                    variance_attendue: 2.0 * (a * a + an * an),
                });
            }
        }
        modes
    }

    /// Déplacement à l'instant `t` au point `xz` avec les `n` premiers modes (tous si `n` dépasse).
    pub fn deplacement(&self, n: usize, t: f32, xz: Vec2, choppiness: f32) -> Vec3 {
        let n = n.min(self.modes.len());
        sommer(
            self.modes[..n].iter().map(|m| (m.k, m.a_l_instant(t))),
            xz,
            choppiness,
        )
    }
}

/// Déplacement (x, y, z) dû aux modes donnés (amplitudes à l'instant voulu), au point `xz`.
pub fn sommer(modes: impl IntoIterator<Item = (Vec2, Vec2)>, xz: Vec2, choppiness: f32) -> Vec3 {
    let mut d = Vec3::ZERO;
    for (k, h) in modes {
        let (s, c) = ops::sin_cos(k.dot(xz));
        let p = cmul(h, Vec2::new(c, s));
        // Dy = Re(h e^{ik·x}) ; Dx = Re(i h kx/|k| e^{ik·x}) = −Im(...) kx/|k|.
        let kn = k / k.length().max(1e-6);
        d += Vec3::new(-p.y * kn.x * choppiness, p.x, -p.y * kn.y * choppiness);
    }
    d * 2.0 // le conjugué en −k donne la même contribution réelle
}

/// Réplique CPU de la mer du vent, mise à jour à chaque image par [`crate::McfMeteoPlugin`].
#[derive(Resource, Debug, Clone, Default)]
pub struct MerDuVent {
    suivi: SuiviSpectre,
    spectre: Spectre,
    /// Modes gardés : (k, amplitude complexe à l'instant courant).
    actifs: Vec<(Vec2, Vec2)>,
    energie_gardee: f32,
    choppiness: f32,
}

impl MerDuVent {
    /// Avance à l'instant de l'océan ; recalcule le spectre si le vent a notablement changé.
    pub fn avancer(&mut self, etat: &EtatMeteo, ocean: &Ocean, reglages: &ReglagesFft) {
        let p = ParamsSpectre::depuis(etat, reglages);
        let modes_voulus = reglages.modes_flottaison as usize;
        if self.suivi.avancer(p.u10, p.vent, p.fetch) {
            self.spectre = Spectre::construire(&p, true);
        }
        let garde = modes_voulus.min(self.spectre.modes.len());
        let t = ocean.temps();
        self.actifs.clear();
        self.actifs.extend(
            self.spectre.modes[..garde]
                .iter()
                .map(|m| (m.k, m.a_l_instant(t))),
        );
        let gardee: f32 = self.spectre.modes[..garde].iter().map(Mode::energie).sum();
        self.energie_gardee = if self.spectre.energie_totale > 0.0 {
            gardee / self.spectre.energie_totale
        } else {
            1.0
        };
        self.choppiness = reglages.choppiness;
    }

    /// Déplacement de la surface au repos `xz` (m), comme le GPU (hors LOD).
    pub fn deplacement(&self, xz: Vec2) -> Vec3 {
        sommer(self.actifs.iter().copied(), xz, self.choppiness)
    }

    /// Part de l'énergie du spectre portée par les modes gardés (`[0, 1]`).
    pub fn energie_gardee(&self) -> f32 {
        self.energie_gardee
    }

    /// Nombre de modes utilisés à chaque évaluation.
    pub fn modes_actifs(&self) -> usize {
        self.actifs.len()
    }

    /// Variance attendue de la hauteur de la mer du vent (m²), pour caler la houle de Gerstner.
    pub fn variance_attendue(&self) -> f32 {
        self.spectre.variance_attendue
    }

    /// Génération du spectre (augmente à chaque recalcul).
    pub fn generation(&self) -> u32 {
        self.suivi.generation
    }
}

pub(crate) fn avancer_mer(
    etat: Res<EtatMeteo>,
    mut ocean: ResMut<Ocean>,
    reglages: Res<ReglagesFft>,
    mut mer: ResMut<MerDuVent>,
) {
    mer.avancer(&etat, &ocean, &reglages);
    // La houle de Gerstner garde la variance que la mer du vent n'a pas (prise en compte à l'image suivante).
    ocean.variance_mer_du_vent = mer.variance_attendue();
}
