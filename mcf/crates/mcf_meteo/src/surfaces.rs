//! État des surfaces : sol mouillé, flaques, neige au sol. Porté du modèle CPU de SkyForge (`WetnessSystem`).
//!
//! Valeurs globales (une par monde), qui évoluent avec la météo :
//! - le sol se mouille sous la pluie, puis sèche avec le vent, le soleil et la chaleur (pas sous la pluie) ;
//! - les flaques se remplissent quand la surface est saturée et sèchent plus lentement ;
//! - la neige tient quand il gèle (~1 couche complète en 2 min de forte neige), fond au-dessus de 0 °C (plus vite
//!   au soleil et sous la pluie), et l'eau de fonte mouille le sol.
//!
//! Les zones abritées restent sèches : c'est le shader qui l'applique, avec la carte des abris.
//! Sans rendu : le serveur peut simuler et diffuser ces valeurs.

use bevy::prelude::*;

use crate::etat::EtatMeteo;

/// Vitesses du modèle (par seconde).
#[derive(Reflect, Debug, Clone, Copy, PartialEq)]
#[reflect(Default)]
pub struct ParametresSurfaces {
    /// Mouillage par unité de pluie.
    pub vitesse_mouillage: f32,
    /// Remplissage des flaques par unité de pluie (une fois la surface saturée).
    pub vitesse_flaques: f32,
    /// Séchage de base.
    pub sechage_base: f32,
    /// Séchage par m/s de vent.
    pub sechage_vent: f32,
    /// Séchage au plein soleil.
    pub sechage_soleil: f32,
    /// Facteur de séchage des flaques (plus lent que la surface).
    pub sechage_flaques: f32,
    /// Accumulation de neige par unité de chute de neige.
    pub vitesse_neige: f32,
}

impl Default for ParametresSurfaces {
    fn default() -> Self {
        Self {
            vitesse_mouillage: 0.08,
            vitesse_flaques: 0.03,
            sechage_base: 0.002,
            sechage_vent: 0.0012,
            sechage_soleil: 0.01,
            sechage_flaques: 0.35,
            vitesse_neige: 0.008,
        }
    }
}

/// Humidité, flaques et neige au sol, chacune dans `[0, 1]`.
#[derive(Resource, Reflect, Debug, Clone, Copy, PartialEq, Default)]
#[reflect(Resource, Default)]
pub struct SurfacesMeteo {
    /// Sol mouillé (0 = sec, 1 = détrempé).
    pub mouillage: f32,
    /// Remplissage des flaques.
    pub flaques: f32,
    /// Couverture de neige.
    pub neige: f32,
    /// Vitesses du modèle.
    pub parametres: ParametresSurfaces,
}

impl SurfacesMeteo {
    /// Avance le modèle de `dt` secondes.
    pub fn avancer(&mut self, etat: &EtatMeteo, dt: f32) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let p = self.parametres;
        let pluie = etat.pluie().clamp(0.0, 1.0);
        let chute_neige = etat.neige().clamp(0.0, 1.0);
        let t = etat.temperature_c;

        self.mouillage = (self.mouillage + pluie * p.vitesse_mouillage * dt).clamp(0.0, 1.0);
        let sature = ((self.mouillage - 0.7) * 3.0).clamp(0.0, 1.0);
        self.flaques = (self.flaques + pluie * sature * p.vitesse_flaques * dt).clamp(0.0, 1.0);

        // Soleil : intensité météo, atténuée par la couverture nuageuse.
        let soleil =
            etat.intensite_soleil.clamp(0.0, 1.0) * (1.0 - 0.7 * etat.couverture_nuageuse.clamp(0.0, 1.0));
        let mut sechage =
            (p.sechage_base + p.sechage_vent * etat.vent_vitesse.max(0.0) + p.sechage_soleil * soleil)
                * (1.0 + 0.04 * (t - 15.0)).max(0.2);
        sechage *= 1.0 - (pluie * 4.0).clamp(0.0, 1.0);
        self.mouillage = (self.mouillage - sechage * dt).clamp(0.0, 1.0);
        self.flaques = (self.flaques - sechage * p.sechage_flaques * dt).clamp(0.0, 1.0);
        // Une flaque garde ses bords mouillés.
        self.mouillage = self.mouillage.max((self.flaques * 3.0).clamp(0.0, 1.0));

        let mut fonte = t.max(0.0) * 0.004 + if t >= 0.0 { soleil * 0.004 } else { 0.0 } + pluie * 0.02;
        if t <= -0.5 {
            fonte *= 0.1;
        }
        let avant = self.neige;
        self.neige = (self.neige + chute_neige * p.vitesse_neige * dt - fonte * dt).clamp(0.0, 1.0);
        if self.neige < avant {
            // L'eau de fonte mouille le sol.
            self.mouillage = (self.mouillage + (avant - self.neige) * 2.0).clamp(0.0, 1.0);
        }
    }
}

pub(crate) fn avancer_surfaces(mut surfaces: ResMut<SurfacesMeteo>, etat: Res<EtatMeteo>, temps: Res<Time>) {
    let dt = temps.delta_secs();
    if dt > 0.0 {
        surfaces.avancer(&etat, dt);
    }
}
