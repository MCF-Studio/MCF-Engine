//! Transition douce d'un état météo vers un autre (courbe smoothstep), comme `WeatherTimeline` de SkyForge.

use bevy::prelude::*;

use crate::etat::EtatMeteo;

/// Transition en cours entre deux états. La ressource [`EtatMeteo`] est recalculée à chaque image tant que la
/// transition n'est pas finie.
#[derive(Resource, Reflect, Debug, Clone, PartialEq)]
#[reflect(Resource, Default)]
pub struct TransitionMeteo {
    depart: EtatMeteo,
    cible: EtatMeteo,
    /// Avancement linéaire, `[0, 1]` (1 = terminée).
    avancement: f32,
    duree_s: f32,
}

impl Default for TransitionMeteo {
    fn default() -> Self {
        let e = EtatMeteo::default();
        Self {
            depart: e,
            cible: e,
            avancement: 1.0,
            duree_s: 0.0,
        }
    }
}

impl TransitionMeteo {
    /// Démarre une transition depuis `actuel` vers `cible` en `duree_s` secondes (0 ou moins = immédiat).
    pub fn demarrer(&mut self, actuel: EtatMeteo, cible: EtatMeteo, duree_s: f32) {
        self.depart = actuel;
        self.cible = cible;
        self.duree_s = if duree_s.is_finite() {
            duree_s.max(0.0)
        } else {
            0.0
        };
        self.avancement = if self.duree_s > 0.0 { 0.0 } else { 1.0 };
    }

    /// Avance de `dt` secondes et renvoie l'état interpolé.
    pub fn avancer(&mut self, dt: f32) -> EtatMeteo {
        if self.avancement < 1.0 {
            self.avancement += dt.max(0.0) / self.duree_s.max(1e-3);
            // Les petits pas accumulés en f32 peuvent s'arrêter juste sous 1 : on arrondit à la fin.
            if self.avancement > 1.0 - 1e-5 {
                self.avancement = 1.0;
            }
        }
        let t = self.avancement;
        let s = t * t * (3.0 - 2.0 * t);
        EtatMeteo::interpoler(&self.depart, &self.cible, s)
    }

    /// Vrai tant que la transition n'est pas terminée.
    pub fn en_cours(&self) -> bool {
        self.avancement < 1.0
    }

    /// Avancement linéaire, `[0, 1]`.
    pub fn avancement(&self) -> f32 {
        self.avancement
    }

    /// État visé.
    pub fn cible(&self) -> &EtatMeteo {
        &self.cible
    }
}

/// Demande de changement de météo. Envoyée par le gameplay, l'éditeur ou le réseau.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct ChangerMeteo {
    /// État visé (il est borné avant d'être appliqué).
    pub cible: EtatMeteo,
    /// Durée de la transition, en secondes (0 = immédiat).
    pub duree_s: f32,
}

/// Lit les demandes [`ChangerMeteo`] (la dernière de l'image l'emporte) puis fait avancer la transition.
pub(crate) fn mettre_a_jour_meteo(
    mut demandes: MessageReader<ChangerMeteo>,
    mut transition: ResMut<TransitionMeteo>,
    mut etat: ResMut<EtatMeteo>,
    temps: Res<Time>,
) {
    if let Some(demande) = demandes.read().last() {
        let mut cible = demande.cible;
        cible.borner();
        transition.demarrer(*etat, cible, demande.duree_s);
        if !transition.en_cours() {
            etat.set_if_neq(cible);
        }
    }
    // Hors transition, on ne touche pas à l'état : l'inspecteur de l'éditeur peut le régler à la main.
    if transition.en_cours() {
        let nouvel = transition.avancer(temps.delta_secs());
        etat.set_if_neq(nouvel);
    }
}
