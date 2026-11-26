//! Champ de vent 3D, porté de `wind.hlsl` de SkyForge. Évalué sur le CPU pour le gameplay (drapeaux, véhicules,
//! objets légers) et le serveur ; la version GPU (volume 3D pour la pluie et la végétation) reprendra exactement
//! les mêmes formules.
//!
//! - Profil logarithmique de couche limite : `u(z) = U10 · ln(z / z0) / ln(10 / z0)`.
//! - Rafales : bruit basse fréquence transporté dans le sens du vent.
//! - Turbulence : rotationnel d'un potentiel de bruit (« curl noise »), à divergence nulle, pour que les
//!   particules tourbillonnent sans s'agglutiner.
//!
//! Différence avec SkyForge : le transport du bruit boucle sur 2 500 m et la phase des rafales sur 1 000 s (bruit
//! périodique), pour garder la précision des `f32` sur une session de jeu de plusieurs heures.

use bevy::math::{Vec3, ops};
use bevy::prelude::*;

use crate::bruit::perlin3;
use crate::etat::EtatMeteo;

/// Échelle spatiale des rafales (1 / m).
const ECHELLE_RAFALES: f32 = 0.012;
/// Échelle spatiale de la turbulence (1 / m).
const ECHELLE_TURBULENCE: f32 = 0.04;
/// Longueur de boucle du transport (m) : `2500 × 0,012 = 30` et `2500 × 0,04 = 100` cellules entières.
const BOUCLE_TRANSPORT_M: f32 = 2500.0;
const PERIODE_RAFALES: i32 = 30;
const PERIODE_TURBULENCE: i32 = 100;
/// Vitesse de défilement vertical du bruit des rafales (cellules / s) et boucle associée (30 cellules = 1 000 s).
const VITESSE_PHASE_RAFALES: f32 = 0.03;
const BOUCLE_PHASE_RAFALES: f32 = PERIODE_RAFALES as f32;

/// Réglages du champ de vent.
#[derive(Reflect, Debug, Clone, Copy, PartialEq)]
#[reflect(Default)]
pub struct ParametresVent {
    /// Hauteur du sol de référence (niveau de la mer), en m. Le profil de couche limite part de là.
    pub niveau_reference: f32,
    /// Longueur de rugosité `z0`, en m (0,0002 mer calme, 0,02 mer agitée / terrain dégagé, 1 ville).
    pub rugosite: f32,
}

impl Default for ParametresVent {
    fn default() -> Self {
        Self {
            niveau_reference: 0.0,
            rugosite: 0.02,
        }
    }
}

/// Valeur du vent en un point.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EchantillonVent {
    /// Vitesse de l'air, en m/s.
    pub vitesse: Vec3,
    /// Intensité de la turbulence (norme de la composante tourbillonnaire), en m/s.
    pub turbulence: f32,
}

/// Champ de vent : réglages et état du transport du bruit. Avancé chaque image par le plugin.
#[derive(Resource, Reflect, Debug, Clone, PartialEq, Default)]
#[reflect(Resource, Default)]
pub struct ChampVent {
    /// Réglages.
    pub parametres: ParametresVent,
    /// Déplacement cumulé du bruit dans le sens du vent (m), bouclé sur 2 500 m.
    transport: Vec3,
    /// Phase verticale du bruit des rafales (cellules), bouclée sur 30.
    phase_rafales: f32,
}

impl ChampVent {
    /// Avance le transport du bruit de `dt` secondes avec le vent actuel.
    pub fn avancer(&mut self, etat: &EtatMeteo, dt: f32) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let deplacement = etat.direction_vent() * etat.vent_vitesse.max(1.0) * dt;
        self.transport = (self.transport + deplacement).rem_euclid(Vec3::splat(BOUCLE_TRANSPORT_M));
        self.phase_rafales =
            (self.phase_rafales + VITESSE_PHASE_RAFALES * dt).rem_euclid(BOUCLE_PHASE_RAFALES);
    }

    /// Déplacement cumulé du bruit (pour le GPU).
    pub fn transport(&self) -> Vec3 {
        self.transport
    }

    /// Phase des rafales (pour le GPU).
    pub fn phase_rafales(&self) -> f32 {
        self.phase_rafales
    }

    /// Vent au point `p` (coordonnées monde, m).
    pub fn echantillonner(&self, etat: &EtatMeteo, p: Vec3) -> EchantillonVent {
        let z0 = self.parametres.rugosite.max(1e-5);
        let hauteur = (p.y - self.parametres.niveau_reference).max(z0 * 1.5);
        let profil = (ops::ln(hauteur / z0) / ops::ln(10.0 / z0)).clamp(0.0, 1.6);

        let q = (p - self.transport) * ECHELLE_RAFALES + Vec3::new(0.0, self.phase_rafales, 0.0);
        let rafale = 1.0 + etat.rafales * perlin3(q, PERIODE_RAFALES) * 1.4;

        let turbulence = rotationnel_bruit((p - self.transport) * ECHELLE_TURBULENCE)
            * etat.vent_vitesse
            * 0.12
            * (0.3 + etat.rafales);
        let vitesse = etat.direction_vent() * etat.vent_vitesse * profil * rafale + turbulence;
        EchantillonVent {
            vitesse,
            turbulence: turbulence.length(),
        }
    }
}

fn potentiel(p: Vec3) -> Vec3 {
    Vec3::new(
        perlin3(p, PERIODE_TURBULENCE),
        perlin3(p + Vec3::new(31.4, 17.2, 5.9), PERIODE_TURBULENCE),
        perlin3(p + Vec3::new(-12.7, 44.1, 23.3), PERIODE_TURBULENCE),
    )
}

/// Rotationnel du potentiel de bruit (différences centrées), à divergence nulle.
pub fn rotationnel_bruit(p: Vec3) -> Vec3 {
    const E: f32 = 0.1;
    let dx = potentiel(p + Vec3::X * E) - potentiel(p - Vec3::X * E);
    let dy = potentiel(p + Vec3::Y * E) - potentiel(p - Vec3::Y * E);
    let dz = potentiel(p + Vec3::Z * E) - potentiel(p - Vec3::Z * E);
    Vec3::new(dy.z - dz.y, dz.x - dx.z, dx.y - dy.x) / (2.0 * E)
}

/// Fait avancer le transport du bruit avec le temps de jeu.
pub(crate) fn avancer_vent(mut champ: ResMut<ChampVent>, etat: Res<EtatMeteo>, temps: Res<Time>) {
    champ.avancer(&etat, temps.delta_secs());
}
