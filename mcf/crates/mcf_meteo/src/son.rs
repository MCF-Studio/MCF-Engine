//! Ambiance sonore (étape 8), portée de `Audio.cpp` de SkyForge : mer au large, ressac synchronisé avec les vagues
//! de bord, vent, pluie et tonnerre, **synthétisés en temps réel** (bruits filtrés modulés par la météo), sans
//! fichier son.
//!
//! Ce module ne dépend d'aucune sortie audio : c'est le calcul des échantillons (testable, utilisable hors Bevy).
//! Le branchement sur `bevy_audio` est dans [`crate::audio`] (feature `son`).
//!
//! Synthèse (par canal, stéréo 48 kHz) :
//! * mer : bruit brun passe-bas (grondement grave), plus fort avec le vent, atténué en altitude et loin du rivage ;
//! * ressac : enveloppe calée sur l'arrivée des crêtes au rivage (même phase que le shader), fracas en bruit rose
//!   passe-bande + chuintement de l'écume qui se retire ; panoramique vers le rivage ;
//! * vent : bande de bruit dont la fréquence suit la vitesse et les rafales, sifflement au-delà de 10 m/s ;
//! * pluie : souffle passe-haut (étouffé sous un abri) + impacts de gouttes aléatoires ;
//! * tonnerre : claquement si proche, puis roulement grave, après le délai de propagation (343 m/s).
//!
//! Mélange : écrêtage doux `x / (1 + |x|)`.

use bevy::math::ops;
use bevy::prelude::*;

/// Fréquence d'échantillonnage de la synthèse (Hz).
pub const FREQUENCE: u32 = 48_000;
const TAU: f32 = std::f32::consts::TAU;
const TONNERRES_MAX: usize = 4;

/// Volumes de l'ambiance (réglables par le joueur).
#[derive(Resource, Reflect, Debug, Clone, Copy, PartialEq)]
#[reflect(Resource, Default)]
pub struct ReglagesSon {
    /// Volume général.
    pub general: f32,
    /// Mer au large.
    pub mer: f32,
    /// Ressac.
    pub ressac: f32,
    /// Vent.
    pub vent: f32,
    /// Pluie.
    pub pluie: f32,
    /// Tonnerre.
    pub tonnerre: f32,
    /// Coupe tout.
    pub muet: bool,
}

impl Default for ReglagesSon {
    fn default() -> Self {
        Self {
            general: 0.8,
            mer: 1.0,
            ressac: 1.0,
            vent: 1.0,
            pluie: 1.0,
            tonnerre: 1.0,
            muet: false,
        }
    }
}

/// État du monde vu par le son, envoyé à chaque image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntreesSon {
    /// Vitesse du vent (m/s).
    pub vent_vitesse: f32,
    /// Force des rafales (0..1).
    pub rafales: f32,
    /// Précipitations (0..1).
    pub precipitations: f32,
    /// Part de neige (0 = pluie, 1 = neige).
    pub facteur_neige: f32,
    /// Amplitude des vagues de bord (m).
    pub amplitude_bord: f32,
    /// Pulsation des vagues de bord (rad/s).
    pub pulsation_bord: f32,
    /// Phase des crêtes au rivage au droit de l'auditeur (rad), comme `rivage::ondulation_crete`.
    pub phase_rive: f32,
    /// Temps de l'océan (s), même horloge que les shaders.
    pub temps_ocean: f32,
    /// Distance au rivage (m).
    pub distance_rivage: f32,
    /// Direction du rivage : −1 à gauche, +1 à droite.
    pub pan_rivage: f32,
    /// Hauteur de l'auditeur au-dessus de la mer (m).
    pub hauteur: f32,
    /// Abrité de la pluie.
    pub abrite: bool,
    /// Jeu en pause (le temps de l'océan n'avance plus).
    pub pause: bool,
}

impl Default for EntreesSon {
    fn default() -> Self {
        Self {
            vent_vitesse: 6.0,
            rafales: 0.3,
            precipitations: 0.0,
            facteur_neige: 0.0,
            amplitude_bord: 0.4,
            pulsation_bord: 1.6,
            phase_rive: 0.0,
            temps_ocean: 0.0,
            distance_rivage: 50.0,
            pan_rivage: 0.0,
            hauteur: 2.0,
            abrite: false,
            pause: false,
        }
    }
}

/// Filtre biquad (formules RBJ, « Audio EQ Cookbook »).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }
}

#[derive(Clone, Copy)]
enum TypeFiltre {
    Bas,
    Haut,
    Bande,
}

impl Biquad {
    fn traiter(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn regler(&mut self, t: TypeFiltre, hz: f32, q: f32) {
        let w = TAU * hz.clamp(10.0, 20_000.0) / FREQUENCE as f32;
        let (s, c) = ops::sin_cos(w);
        let a = s / (2.0 * q);
        let (b0, b1, b2) = match t {
            TypeFiltre::Bas => ((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0),
            TypeFiltre::Haut => ((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0),
            TypeFiltre::Bande => (a, 0.0, -a),
        };
        let a0 = 1.0 + a;
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = -2.0 * c / a0;
        self.a2 = (1.0 - a) / a0;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Tonnerre {
    delai: f32,
    t: f32,
    proximite: f32,
    actif: bool,
}

/// Synthétiseur de l'ambiance. Un par sortie audio ; [`SynthAmbiance::rendre`] remplit un bloc stéréo entrelacé.
#[derive(Debug, Clone)]
pub struct SynthAmbiance {
    alea: u32,
    brun: [f32; 2],
    rose: [[f32; 7]; 2],
    mer_pb: [Biquad; 2],
    ressac_pbd: [Biquad; 2],
    ecume_ph: [Biquad; 2],
    vent_pbd: [Biquad; 2],
    sifflement: [Biquad; 2],
    pluie_ph: [Biquad; 2],
    pluie_pb: [Biquad; 2],
    goutte_pbd: [Biquad; 2],
    tonnerre_pb: [Biquad; 2],
    lisse: [f32; 5],
    lfo_rafales: f32,
    queue_ressac: f32,
    env_goutte: [f32; 2],
    niveau_goutte: [f32; 2],
    tonnerres: [Tonnerre; TONNERRES_MAX],
    temps: f32,
    dernier_temps_recu: f32,
}

impl Default for SynthAmbiance {
    fn default() -> Self {
        Self::new(0x1234_5678)
    }
}

impl SynthAmbiance {
    /// Nouveau synthétiseur ; `graine` ≠ 0 (générateur xorshift).
    pub fn new(graine: u32) -> Self {
        Self {
            alea: graine.max(1),
            brun: [0.0; 2],
            rose: [[0.0; 7]; 2],
            mer_pb: Default::default(),
            ressac_pbd: Default::default(),
            ecume_ph: Default::default(),
            vent_pbd: Default::default(),
            sifflement: Default::default(),
            pluie_ph: Default::default(),
            pluie_pb: Default::default(),
            goutte_pbd: Default::default(),
            tonnerre_pb: Default::default(),
            lisse: [0.0; 5],
            lfo_rafales: 0.0,
            queue_ressac: 0.0,
            env_goutte: [0.0; 2],
            niveau_goutte: [0.0; 2],
            tonnerres: [Tonnerre::default(); TONNERRES_MAX],
            temps: 0.0,
            dernier_temps_recu: -1.0,
        }
    }

    fn blanc(&mut self) -> f32 {
        self.alea ^= self.alea << 13;
        self.alea ^= self.alea >> 17;
        self.alea ^= self.alea << 5;
        self.alea as f32 / 2_147_483_648.0 - 1.0
    }

    /// Programme un coup de tonnerre dans `delai_s` secondes ; `proximite` de 0 (loin) à 1 (tout près).
    /// Au plus 4 en même temps (les suivants sont ignorés).
    pub fn tonnerre(&mut self, delai_s: f32, proximite: f32) {
        if let Some(t) = self.tonnerres.iter_mut().find(|t| !t.actif) {
            *t = Tonnerre {
                delai: if delai_s.is_finite() {
                    delai_s.max(0.0)
                } else {
                    0.0
                },
                t: 0.0,
                proximite: proximite.clamp(0.0, 1.0),
                actif: true,
            };
        }
    }

    /// Nombre de tonnerres en cours ou programmés.
    pub fn tonnerres_actifs(&self) -> usize {
        self.tonnerres.iter().filter(|t| t.actif).count()
    }

    /// Remplit `sortie` (stéréo entrelacé, gauche puis droite) avec `sortie.len() / 2` images.
    pub fn rendre(&mut self, e: &EntreesSon, r: &ReglagesSon, sortie: &mut [f32]) {
        let images = sortie.len() / 2;
        // Le temps de l'océan se recale sur celui du jeu à chaque nouvelle valeur, et avance à l'échantillon près
        // entre deux.
        if e.temps_ocean != self.dernier_temps_recu {
            self.temps = e.temps_ocean;
            self.dernier_temps_recu = e.temps_ocean;
        }
        if r.muet {
            sortie.fill(0.0);
            return;
        }
        let dt = 1.0 / FREQUENCE as f32;
        let fini = |v: f32, defaut: f32| if v.is_finite() { v } else { defaut };
        let vent_v = fini(e.vent_vitesse, 0.0).max(0.0);
        let precip = fini(e.precipitations, 0.0).clamp(0.0, 1.0);
        let neige = fini(e.facteur_neige, 0.0).clamp(0.0, 1.0);
        let distance = fini(e.distance_rivage, 1e4).max(0.0);
        let hauteur = fini(e.hauteur, 0.0).max(0.0);

        // Niveaux cibles de chaque couche (lissés par échantillon plus bas).
        let altitude = 1.0 / (1.0 + hauteur / 120.0);
        let att_rivage = 1.0 / (1.0 + ops::powf(distance / 30.0, 1.5));
        let etouffe_neige = 1.0 - 0.3 * neige * precip;
        let pluie = precip * (1.0 - neige);
        let vent_n = (vent_v / 20.0).clamp(0.0, 1.6);
        let cibles = [
            r.mer
                * (0.20 + 0.02 * vent_v)
                * altitude
                * (0.35 + 0.65 / (1.0 + distance / 300.0))
                * etouffe_neige,
            r.ressac
                * (fini(e.amplitude_bord, 0.0) / 0.5).clamp(0.0, 2.5)
                * att_rivage
                * altitude
                * etouffe_neige,
            r.vent * ops::powf(vent_n, 1.3) * (1.0 + (hauteur / 300.0).min(1.0)),
            r.pluie * pluie * if e.abrite { 0.55 } else { 1.0 },
            r.tonnerre,
        ];

        // Filtres (une fois par bloc).
        let b = self.blanc();
        self.lfo_rafales += (b * 0.5 - self.lfo_rafales * 0.02) * 0.05;
        let rafale = (1.0 + fini(e.rafales, 0.0) * self.lfo_rafales * 1.5).clamp(0.3, 2.0);
        for c in 0..2 {
            let ecart = |g: f32, d: f32| if c == 1 { d } else { g };
            self.mer_pb[c].regler(TypeFiltre::Bas, 380.0 + 10.0 * vent_v, 0.6);
            self.ecume_ph[c].regler(TypeFiltre::Haut, 2800.0, 0.7);
            self.vent_pbd[c].regler(
                TypeFiltre::Bande,
                (240.0 + 32.0 * vent_v) * rafale * ecart(0.94, 1.07),
                0.8,
            );
            self.sifflement[c].regler(
                TypeFiltre::Bande,
                (620.0 + 45.0 * vent_v) * rafale * ecart(0.98, 1.02),
                9.0,
            );
            self.pluie_ph[c].regler(TypeFiltre::Haut, 1300.0, 0.7);
            self.pluie_pb[c].regler(TypeFiltre::Bas, if e.abrite { 3500.0 } else { 9000.0 }, 0.7);
            self.goutte_pbd[c].regler(TypeFiltre::Bande, 2300.0 + 900.0 * c as f32, 1.8);
            self.tonnerre_pb[c].regler(TypeFiltre::Bas, 140.0, 0.8);
        }

        let omega = fini(e.pulsation_bord, 0.0);
        let decroissance_queue = ops::exp(-dt / 1.8);
        let decroissance_goutte = ops::exp(-dt / 0.004);
        let taux_gouttes = 450.0 * pluie;
        let pan = fini(e.pan_rivage, 0.0).clamp(-1.0, 1.0);
        let (pan_d, pan_g) = ops::sin_cos((pan * 0.7 + 1.0) * std::f32::consts::FRAC_PI_4);

        for n in 0..images {
            for (l, c) in self.lisse.iter_mut().zip(cibles) {
                *l += (c - *l) * 0.0005;
            }
            let temps = self.temps;
            if !e.pause {
                self.temps += dt;
            }

            // Ressac : enveloppe synchronisée avec l'arrivée des crêtes sur le rivage.
            let phase = fini(e.phase_rive, 0.0) - omega * temps;
            let fracas = ops::powf((0.5 + 0.5 * ops::cos(phase)).clamp(0.0, 1.0), 4.0);
            self.queue_ressac = fracas.max(self.queue_ressac * decroissance_queue);
            self.ressac_pbd[0].regler(TypeFiltre::Bande, 500.0 + 900.0 * fracas, 0.9);
            let (z1, z2) = (self.ressac_pbd[1].z1, self.ressac_pbd[1].z2);
            self.ressac_pbd[1] = Biquad {
                z1,
                z2,
                ..self.ressac_pbd[0]
            };

            let mut canaux = [0.0f32; 2];
            for (c, sortie_c) in canaux.iter_mut().enumerate() {
                let w = self.blanc();
                // Bruit brun (intégration fuyante) et rose (filtre de Paul Kellet).
                self.brun[c] = (self.brun[c] + 0.02 * w) * 0.998;
                let pk = &mut self.rose[c];
                pk[0] = 0.99886 * pk[0] + w * 0.055_517_9;
                pk[1] = 0.99332 * pk[1] + w * 0.075_075_9;
                pk[2] = 0.96900 * pk[2] + w * 0.153_852;
                pk[3] = 0.86650 * pk[3] + w * 0.310_485_6;
                pk[4] = 0.55000 * pk[4] + w * 0.532_952_2;
                pk[5] = -0.7616 * pk[5] - w * 0.016_898;
                let rose = (pk.iter().sum::<f32>() + w * 0.5362) * 0.11;
                pk[6] = w * 0.115_926;

                let mer = self.mer_pb[c].traiter(self.brun[c] * 3.0) * 2.2 * self.lisse[0];

                let corps = self.ressac_pbd[c].traiter(rose) * fracas * 2.2;
                let chuintement = self.ecume_ph[c].traiter(w) * self.queue_ressac * 0.22;
                let coup = self.brun[c] * fracas * 1.2;
                let ressac =
                    (corps + chuintement + coup) * self.lisse[1] * if c == 0 { pan_g } else { pan_d } * 1.41;

                let vent = (self.vent_pbd[c].traiter(w) * 0.9
                    + self.sifflement[c].traiter(w) * 0.25 * (vent_n - 0.5).max(0.0))
                    * self.lisse[2]
                    * rafale;

                let souffle = self.pluie_pb[c].traiter(self.pluie_ph[c].traiter(w)) * 0.35;
                if (self.alea & 0xF_FFFF) as f32 / 1_048_576.0 < taux_gouttes * dt {
                    let a = self.blanc().abs();
                    self.niveau_goutte[c] = a * a;
                    self.env_goutte[c] = 1.0;
                }
                self.env_goutte[c] *= decroissance_goutte;
                let goutte = self.goutte_pbd[c].traiter(w * self.env_goutte[c] * self.niveau_goutte[c]) * 2.5;
                let pluie_c = (souffle + goutte) * self.lisse[3];

                let mut tonnerre = 0.0;
                for t in self.tonnerres.iter().filter(|t| t.actif && t.delai <= 0.0) {
                    let claquement = if t.t < 0.25 {
                        ops::exp(-t.t * 14.0) * t.proximite * w * 0.8
                    } else {
                        0.0
                    };
                    let env =
                        (1.0 - ops::exp(-t.t * 6.0)) * ops::exp(-t.t / (2.5 + 2.5 * (1.0 - t.proximite)));
                    tonnerre += claquement + self.brun[c] * 6.0 * env * (0.5 + 0.8 * t.proximite);
                }
                let tonnerre = self.tonnerre_pb[c].traiter(tonnerre) * self.lisse[4];

                let m = (mer + ressac + vent + pluie_c + tonnerre) * r.general;
                *sortie_c = m / (1.0 + m.abs());
            }
            for t in self.tonnerres.iter_mut().filter(|t| t.actif) {
                if t.delai > 0.0 {
                    t.delai -= dt;
                } else {
                    t.t += dt;
                    if t.t > 12.0 {
                        t.actif = false;
                    }
                }
            }
            sortie[n * 2] = canaux[0];
            sortie[n * 2 + 1] = canaux[1];
        }
    }
}

/// Entrées du son pour un auditeur en `position`, dont la droite est `droite` (fonction pure).
#[allow(clippy::too_many_arguments)]
pub fn calculer_entrees(
    etat: &crate::EtatMeteo,
    ocean: &crate::Ocean,
    carte: &crate::CarteRivage,
    bord: &crate::rivage::ParamsBord,
    position: Vec3,
    droite: Vec3,
    abrite: bool,
    pause: bool,
) -> EntreesSon {
    let xz = position.xz();
    let hauteur = position.y - ocean.niveau;
    // Distance au rivage : grille du rivage si elle couvre l'auditeur, sinon « loin ».
    let (distance_plane, phase_rive, pan) = match carte.echantillonner(xz) {
        Some(e) if e.distance.abs() < crate::rivage::LOIN_DU_RIVAGE * 0.5 && e.normale != Vec2::ZERO => {
            let rive = xz - e.normale * e.distance;
            let vers = rive - xz;
            let pan = if e.distance.abs() > 0.5 {
                vers.normalize_or_zero()
                    .dot(droite.xz().normalize_or_zero())
                    .clamp(-1.0, 1.0)
            } else {
                0.0
            };
            (e.distance.abs(), crate::rivage::ondulation_crete(rive), pan)
        }
        _ => (crate::rivage::LOIN_DU_RIVAGE, 0.0, 0.0),
    };
    EntreesSon {
        vent_vitesse: etat.vent_vitesse,
        rafales: etat.rafales,
        precipitations: etat.precipitations,
        facteur_neige: etat.facteur_neige(),
        amplitude_bord: bord.amplitude,
        pulsation_bord: bord.omega,
        phase_rive,
        temps_ocean: ocean.temps(),
        distance_rivage: (distance_plane * distance_plane + hauteur * hauteur * 0.25).sqrt(),
        pan_rivage: pan,
        hauteur,
        abrite,
        pause,
    }
}
