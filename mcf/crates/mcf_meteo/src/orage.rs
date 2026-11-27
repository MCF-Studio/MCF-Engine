//! Éclairs pendant les orages (étape 8), comme SkyForge : en temps d'orage (forte pluie, cumulonimbus, ciel
//! couvert, pas de neige), un éclair toutes les 7 à 29 s, à une distance aléatoire. Chaque éclair est publié en
//! message [`Eclair`] : le son programme le tonnerre (délai = distance / 343 m/s), le rendu peut faire un flash
//! avec [`Orage::eclat`]. Sans rendu : le serveur peut tirer les éclairs et les diffuser.

use bevy::math::ops;
use bevy::prelude::*;

use crate::bruit::hash;
use crate::etat::EtatMeteo;

/// Vitesse du son (m/s).
pub const VITESSE_SON: f32 = 343.0;

/// Un éclair vient de frapper.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct Eclair {
    /// 0 = loin (3,5 km), 1 = tout près (250 m).
    pub proximite: f32,
    /// Distance (m).
    pub distance_m: f32,
    /// Délai avant le tonnerre (s).
    pub delai_tonnerre_s: f32,
}

impl Eclair {
    /// Éclair à une proximité donnée.
    pub fn new(proximite: f32) -> Self {
        let p = proximite.clamp(0.0, 1.0);
        let distance_m = 3500.0 + (250.0 - 3500.0) * p;
        Self {
            proximite: p,
            distance_m,
            delai_tonnerre_s: distance_m / VITESSE_SON,
        }
    }
}

/// Tirage des éclairs.
#[derive(Resource, Reflect, Debug, Clone, PartialEq)]
#[reflect(Resource, Default)]
pub struct Orage {
    /// Autorise les éclairs.
    pub eclairs: bool,
    alea: u32,
    prochain: f32,
    depuis_eclair: f32,
}

impl Default for Orage {
    fn default() -> Self {
        Self {
            eclairs: true,
            alea: 0x2545_f491,
            prochain: 6.0,
            depuis_eclair: 100.0,
        }
    }
}

impl Orage {
    /// Vrai si la météo est orageuse (mêmes seuils que SkyForge).
    pub fn orageux(etat: &EtatMeteo) -> bool {
        etat.precipitations > 0.55
            && etat.type_nuages > 0.6
            && etat.couverture_nuageuse > 0.7
            && etat.facteur_neige() < 0.5
    }

    fn aleatoire(&mut self) -> f32 {
        self.alea = hash(self.alea.wrapping_add(0x9e37_79b9));
        (self.alea & 0x00ff_ffff) as f32 / 16_777_216.0
    }

    /// Avance de `dt` secondes ; renvoie l'éclair s'il en tombe un.
    pub fn avancer(&mut self, etat: &EtatMeteo, dt: f32) -> Option<Eclair> {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.depuis_eclair = (self.depuis_eclair + dt).min(1000.0);
        if !self.eclairs || !Self::orageux(etat) || dt <= 0.0 {
            return None;
        }
        self.prochain -= dt;
        if self.prochain > 0.0 {
            return None;
        }
        let eclair = Eclair::new(self.aleatoire());
        self.prochain = 7.0 + self.aleatoire() * 22.0;
        self.depuis_eclair = 0.0;
        Some(eclair)
    }

    /// Intensité du flash (0..1) : 2 ou 3 scintillements rapides dans les 0,6 s qui suivent l'éclair.
    pub fn eclat(&self) -> f32 {
        let s = self.depuis_eclair;
        if !(0.0..0.6).contains(&s) {
            return 0.0;
        }
        let a = ops::exp(-s * 25.0);
        let b = if s > 0.12 {
            ops::exp(-(s - 0.12) * 30.0) * 0.7
        } else {
            0.0
        };
        let c = if s > 0.3 {
            ops::exp(-(s - 0.3) * 18.0) * 0.5
        } else {
            0.0
        };
        a.max(b).max(c)
    }
}

pub(crate) fn avancer_orage(
    mut orage: ResMut<Orage>,
    etat: Res<EtatMeteo>,
    temps: Res<Time>,
    mut eclairs: MessageWriter<Eclair>,
) {
    if let Some(e) = orage.avancer(&etat, temps.delta_secs()) {
        eclairs.write(e);
    }
}
