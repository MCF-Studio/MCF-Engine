//! État météo : tout ce qui pilote le module (nuages, vent, précipitations, température, brouillard).
//!
//! Repère MCF : Y vers le haut, main droite, avant = −Z, mètres. Les angles sont en degrés dans le plan XZ :
//! 0° = vers −Z (avant), 90° = vers +X (droite). Porté de `Weather.h/.cpp` de SkyForge.

use bevy::math::{Vec3, ops};
use bevy::prelude::*;

/// Ensemble des grandeurs météo. Chaque champ s'interpole (voir [`EtatMeteo::interpoler`]).
#[derive(Resource, Reflect, Debug, Clone, Copy, PartialEq)]
#[reflect(Resource, Default)]
pub struct EtatMeteo {
    /// Part du ciel couverte par les nuages, `[0, 1]`.
    pub couverture_nuageuse: f32,
    /// Multiplicateur d'extinction des nuages (plus grand = nuages plus opaques).
    pub densite_nuages: f32,
    /// Forme des nuages : 0 = stratus (plats), 1 = cumulonimbus (tours d'orage).
    pub type_nuages: f32,
    /// Direction vers laquelle souffle le vent, en degrés (0° = vers −Z, 90° = vers +X).
    pub vent_angle_deg: f32,
    /// Vitesse du vent à 10 m du sol (U10), en m/s.
    pub vent_vitesse: f32,
    /// Force des rafales, `[0, 1]`.
    pub rafales: f32,
    /// Intensité des précipitations, `[0, 1]`. Pluie ou neige selon [`EtatMeteo::temperature_c`].
    pub precipitations: f32,
    /// Température de l'air, en °C.
    pub temperature_c: f32,
    /// Atténuation de la lumière du soleil par la météo, `[0, 1]` (1 = ciel dégagé).
    pub intensite_soleil: f32,
    /// Distance sur laquelle le vent a soufflé sur la mer (fetch du spectre JONSWAP), en km.
    pub fetch_km: f32,
    /// Densité du brouillard de base, par mètre (s'ajoute à celui de la pluie).
    pub densite_brouillard: f32,
}

impl Default for EtatMeteo {
    fn default() -> Self {
        PresetMeteo::Clair.etat()
    }
}

impl EtatMeteo {
    /// Interpolation linéaire de `a` (t = 0) vers `b` (t = 1). L'angle du vent prend le chemin le plus court.
    pub fn interpoler(a: &Self, b: &Self, t: f32) -> Self {
        let l = |x: f32, y: f32| x + (y - x) * t;
        let d = (b.vent_angle_deg - a.vent_angle_deg + 180.0).rem_euclid(360.0) - 180.0;
        Self {
            couverture_nuageuse: l(a.couverture_nuageuse, b.couverture_nuageuse),
            densite_nuages: l(a.densite_nuages, b.densite_nuages),
            type_nuages: l(a.type_nuages, b.type_nuages),
            vent_angle_deg: (a.vent_angle_deg + d * t).rem_euclid(360.0),
            vent_vitesse: l(a.vent_vitesse, b.vent_vitesse),
            rafales: l(a.rafales, b.rafales),
            precipitations: l(a.precipitations, b.precipitations),
            temperature_c: l(a.temperature_c, b.temperature_c),
            intensite_soleil: l(a.intensite_soleil, b.intensite_soleil),
            fetch_km: l(a.fetch_km, b.fetch_km),
            densite_brouillard: l(a.densite_brouillard, b.densite_brouillard),
        }
    }

    /// Direction unitaire vers laquelle souffle le vent (horizontale).
    pub fn direction_vent(&self) -> Vec3 {
        let a = self.vent_angle_deg.to_radians();
        Vec3::new(ops::sin(a), 0.0, -ops::cos(a))
    }

    /// Vecteur vent (direction × vitesse), en m/s.
    pub fn vent(&self) -> Vec3 {
        self.direction_vent() * self.vent_vitesse
    }

    /// Exposition conseillée de la caméra (EV100) : 13 par beau temps, ouverte jusqu'à 1,8 diaphragme de plus
    /// sous un ciel couvert, comme le fait l'œil ou un appareil photo. Sans cela, à exposition fixe de plein
    /// soleil, tout ce qui est blanc sous les nuages (écume, neige) sort gris et le sable presque noir.
    pub fn exposition_ev100(&self) -> f32 {
        let couverture = if self.couverture_nuageuse.is_finite() {
            self.couverture_nuageuse
        } else {
            0.0
        };
        let c = ((couverture - 0.45) / 0.5).clamp(0.0, 1.0);
        13.0 - 1.8 * c * c * (3.0 - 2.0 * c)
    }

    /// Part des précipitations qui tombe en neige, `[0, 1]` : 0 au-dessus de 1,5 °C, 1 en dessous de −1 °C.
    /// Même formule que `SnowFactor` dans les shaders de SkyForge.
    pub fn facteur_neige(&self) -> f32 {
        ((1.5 - self.temperature_c) / 2.5).clamp(0.0, 1.0)
    }

    /// Intensité de la pluie, `[0, 1]`.
    pub fn pluie(&self) -> f32 {
        self.precipitations * (1.0 - self.facteur_neige())
    }

    /// Intensité de la neige, `[0, 1]`.
    pub fn neige(&self) -> f32 {
        self.precipitations * self.facteur_neige()
    }

    /// Ramène chaque champ dans son domaine valide (utile après une saisie dans l'inspecteur ou une
    /// réception réseau : on ne fait pas confiance aux valeurs reçues).
    pub fn borner(&mut self) {
        let fini = |v: f32, defaut: f32| if v.is_finite() { v } else { defaut };
        let d = Self::default();
        self.couverture_nuageuse = fini(self.couverture_nuageuse, d.couverture_nuageuse).clamp(0.0, 1.0);
        self.densite_nuages = fini(self.densite_nuages, d.densite_nuages).clamp(0.0, 4.0);
        self.type_nuages = fini(self.type_nuages, d.type_nuages).clamp(0.0, 1.0);
        self.vent_angle_deg = fini(self.vent_angle_deg, d.vent_angle_deg).rem_euclid(360.0);
        self.vent_vitesse = fini(self.vent_vitesse, d.vent_vitesse).clamp(0.0, 60.0);
        self.rafales = fini(self.rafales, d.rafales).clamp(0.0, 1.0);
        self.precipitations = fini(self.precipitations, d.precipitations).clamp(0.0, 1.0);
        self.temperature_c = fini(self.temperature_c, d.temperature_c).clamp(-60.0, 60.0);
        self.intensite_soleil = fini(self.intensite_soleil, d.intensite_soleil).clamp(0.0, 1.0);
        self.fetch_km = fini(self.fetch_km, d.fetch_km).clamp(1.0, 2000.0);
        self.densite_brouillard = fini(self.densite_brouillard, d.densite_brouillard).clamp(0.0, 0.1);
    }
}

/// Préréglages météo (mêmes valeurs que SkyForge).
#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresetMeteo {
    /// Ciel dégagé, petits cumulus.
    Clair,
    /// Ciel chargé, sans précipitations.
    Nuageux,
    /// Pluie modérée.
    Pluie,
    /// Tempête : vent fort, forte pluie, cumulonimbus, éclairs.
    Tempete,
    /// Neige (température négative).
    Neige,
}

impl PresetMeteo {
    /// Tous les préréglages, dans l'ordre d'affichage.
    pub const TOUS: [Self; 5] = [
        Self::Clair,
        Self::Nuageux,
        Self::Pluie,
        Self::Tempete,
        Self::Neige,
    ];

    /// Nom affiché.
    pub fn nom(self) -> &'static str {
        match self {
            Self::Clair => "Clair",
            Self::Nuageux => "Nuageux",
            Self::Pluie => "Pluie",
            Self::Tempete => "Tempête",
            Self::Neige => "Neige",
        }
    }

    /// État météo correspondant.
    pub fn etat(self) -> EtatMeteo {
        let base = EtatMeteo {
            couverture_nuageuse: 0.35,
            densite_nuages: 0.6,
            type_nuages: 0.4,
            vent_angle_deg: 30.0,
            vent_vitesse: 6.0,
            rafales: 0.3,
            precipitations: 0.0,
            temperature_c: 15.0,
            intensite_soleil: 1.0,
            fetch_km: 300.0,
            densite_brouillard: 0.00015,
        };
        match self {
            Self::Clair => EtatMeteo {
                couverture_nuageuse: 0.25,
                densite_nuages: 0.5,
                type_nuages: 0.35,
                vent_vitesse: 5.0,
                rafales: 0.15,
                densite_brouillard: 0.0001,
                ..base
            },
            Self::Nuageux => EtatMeteo {
                couverture_nuageuse: 0.6,
                densite_nuages: 0.7,
                type_nuages: 0.5,
                vent_vitesse: 9.0,
                rafales: 0.35,
                intensite_soleil: 0.85,
                densite_brouillard: 0.0002,
                ..base
            },
            Self::Pluie => EtatMeteo {
                couverture_nuageuse: 0.85,
                densite_nuages: 0.9,
                type_nuages: 0.3,
                vent_vitesse: 11.0,
                rafales: 0.5,
                precipitations: 0.6,
                temperature_c: 11.0,
                intensite_soleil: 0.6,
                densite_brouillard: 0.0004,
                ..base
            },
            Self::Tempete => EtatMeteo {
                couverture_nuageuse: 0.95,
                densite_nuages: 1.0,
                type_nuages: 0.95,
                vent_vitesse: 22.0,
                rafales: 0.9,
                precipitations: 1.0,
                temperature_c: 9.0,
                intensite_soleil: 0.4,
                fetch_km: 800.0,
                densite_brouillard: 0.0008,
                ..base
            },
            Self::Neige => EtatMeteo {
                couverture_nuageuse: 0.85,
                densite_nuages: 0.8,
                type_nuages: 0.25,
                vent_vitesse: 4.0,
                rafales: 0.25,
                precipitations: 0.7,
                temperature_c: -4.0,
                intensite_soleil: 0.65,
                densite_brouillard: 0.0009,
                ..base
            },
        }
    }
}
