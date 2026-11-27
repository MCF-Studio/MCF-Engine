//! Houle de l'océan : 4 vagues de Gerstner calées sur le pic du spectre JONSWAP (vent et fetch de la météo).
//! Porté du modèle analytique de SkyForge (utilisé au large, au-delà des cascades FFT).
//!
//! Sans rendu : le même calcul sert au shader et au gameplay (flottaison des bateaux, nage), y compris sur le
//! serveur. Les pulsations sont quantifiées pour que la houle boucle exactement sur [`PERIODE_HOULE_S`] : le temps
//! reste petit, donc précis, sur des heures de jeu.

use bevy::math::{Vec2, Vec3, Vec4, ops};
use bevy::prelude::*;

use crate::etat::EtatMeteo;

/// Accélération de la pesanteur (m/s²).
pub const G: f32 = 9.81;
/// Période de bouclage de la houle (s).
pub const PERIODE_HOULE_S: f32 = 200.0;

const ANGLES: [f32; 4] = [0.0, 0.42, -0.55, 0.9];
const LONGUEURS: [f32; 4] = [1.0, 0.61, 0.37, 0.23];
const AMPLITUDES: [f32; 4] = [1.0, 0.45, 0.25, 0.12];

/// Une vague de Gerstner.
#[derive(Reflect, Debug, Clone, Copy, PartialEq, Default)]
pub struct VagueGerstner {
    /// Direction de propagation (unitaire, plan XZ).
    pub direction: Vec2,
    /// Nombre d'onde (rad/m).
    pub k: f32,
    /// Pulsation (rad/s), quantifiée sur la période de bouclage.
    pub omega: f32,
    /// Amplitude (m).
    pub amplitude: f32,
    /// Raideur (0 = sinusoïde, 1 = crête pointue).
    pub raideur: f32,
    /// Déphasage (rad).
    pub phase: f32,
}

/// Longueur d'onde du pic (m) et hauteur significative (m) du spectre JONSWAP limité par le fetch, borné par la mer
/// pleinement développée.
pub fn pic_jonswap(vent_vitesse: f32, fetch_km: f32) -> (f32, f32) {
    let u = vent_vitesse.max(0.5);
    let f = (fetch_km * 1000.0).max(1000.0);
    let wp = (22.0 * ops::cbrt(G * G / (u * f))).max(0.855 * G / u);
    let longueur = 2.0 * std::f32::consts::PI * G / (wp * wp);
    let hs_fetch = 0.0016 * u * (f / G).sqrt();
    let hs_plein = 0.21 * u * u / G;
    (longueur, hs_fetch.min(hs_plein))
}

/// Les 4 vagues pour un état météo. `raideur` globale dans `[0, 1]`.
pub fn vagues(etat: &EtatMeteo, raideur: f32) -> [VagueGerstner; 4] {
    let (longueur_pic, hs) = pic_jonswap(etat.vent_vitesse, etat.fetch_km);
    let amplitude = 0.3 * hs;
    let vent = etat.direction_vent();
    let dv = Vec2::new(vent.x, vent.z).normalize_or(Vec2::NEG_Y);
    std::array::from_fn(|i| {
        let (s, c) = ops::sin_cos(ANGLES[i]);
        let direction = Vec2::new(dv.x * c - dv.y * s, dv.x * s + dv.y * c);
        let l = (longueur_pic * LONGUEURS[i]).max(1.0);
        let k = 2.0 * std::f32::consts::PI / l;
        let omega = quantifier_pulsation((G * k).sqrt());
        let a = amplitude * AMPLITUDES[i];
        let q = (0.6 / (k * a * 4.0).max(1e-4)).min(1.0) * raideur.clamp(0.0, 1.0);
        VagueGerstner {
            direction,
            k,
            omega,
            amplitude: a,
            raideur: q,
            phase: i as f32 * 1.7,
        }
    })
}

/// Arrondit une pulsation à un nombre entier de cycles sur [`PERIODE_HOULE_S`] (au moins un).
pub fn quantifier_pulsation(omega: f32) -> f32 {
    let tour = 2.0 * std::f32::consts::PI / PERIODE_HOULE_S;
    (omega / tour).round().max(1.0) * tour
}

/// Déplacement (x, y, z) d'un point de la surface au repos `xz`, au temps `t` (s).
pub fn deplacement(vagues: &[VagueGerstner; 4], xz: Vec2, t: f32) -> Vec3 {
    vagues.iter().fold(Vec3::ZERO, |acc, v| {
        let th = v.k * v.direction.dot(xz) - v.omega * t + v.phase;
        let (s, c) = ops::sin_cos(th);
        acc + Vec3::new(
            v.raideur * v.amplitude * v.direction.x * c,
            v.amplitude * s,
            v.raideur * v.amplitude * v.direction.y * c,
        )
    })
}

/// État de l'océan, avancé chaque image.
#[derive(Resource, Reflect, Debug, Clone, PartialEq)]
#[reflect(Resource, Default)]
pub struct Ocean {
    /// Niveau de la mer au repos (m).
    pub niveau: f32,
    /// Raideur des vagues, `[0, 1]`.
    pub raideur: f32,
    /// Temps bouclé sur [`PERIODE_HOULE_S`].
    temps: f32,
    /// Vagues actuelles (recalculées depuis la météo).
    vagues: [VagueGerstner; 4],
}

impl Default for Ocean {
    fn default() -> Self {
        Self {
            niveau: 0.0,
            raideur: 1.0,
            temps: 0.0,
            vagues: vagues(&EtatMeteo::default(), 1.0),
        }
    }
}

impl Ocean {
    /// Avance de `dt` secondes avec la météo actuelle.
    pub fn avancer(&mut self, etat: &EtatMeteo, dt: f32) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.temps = (self.temps + dt).rem_euclid(PERIODE_HOULE_S);
        self.vagues = vagues(etat, self.raideur);
    }

    /// Temps bouclé (s).
    pub fn temps(&self) -> f32 {
        self.temps
    }

    /// Vagues actuelles.
    pub fn vagues(&self) -> &[VagueGerstner; 4] {
        &self.vagues
    }

    /// Hauteur de l'eau à la verticale de (x, z) : on inverse le déplacement horizontal des vagues de Gerstner par
    /// itérations de point fixe (3 suffisent, la raideur étant bornée).
    pub fn hauteur(&self, x: f32, z: f32) -> f32 {
        let cible = Vec2::new(x, z);
        let mut repos = cible;
        for _ in 0..3 {
            let d = deplacement(&self.vagues, repos, self.temps);
            repos = cible - Vec2::new(d.x, d.z);
        }
        self.niveau + deplacement(&self.vagues, repos, self.temps).y
    }

    /// Paramètres des vagues pour le GPU : `a[i]` = (dir.x, dir.z, k, ω), `b[i]` = (amplitude, raideur, phase, 0).
    pub fn uniformes_vagues(&self) -> ([Vec4; 4], [Vec4; 4]) {
        let a = self
            .vagues
            .map(|v| Vec4::new(v.direction.x, v.direction.y, v.k, v.omega));
        let b = self
            .vagues
            .map(|v| Vec4::new(v.amplitude, v.raideur, v.phase, 0.0));
        (a, b)
    }
}

pub(crate) fn avancer_ocean(mut ocean: ResMut<Ocean>, etat: Res<EtatMeteo>, temps: Res<Time>) {
    ocean.avancer(&etat, temps.delta_secs());
}
