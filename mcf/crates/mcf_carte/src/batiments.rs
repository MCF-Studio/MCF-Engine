//! Bâti (`batiments/c_<x>_<z>.json`) : des blocs (empreinte × niveaux), générés ou posés à la main, et des bâtiments
//! modélisés (fiche du catalogue + cap, spec §6.5). Rangés par cellule pour limiter les conflits Git quand deux
//! personnes travaillent sur des quartiers différents.
//!
//! Version 2 : champ facultatif `modele`. La version 1 (sans modèles) se lit toujours, telle quelle.

use std::collections::BTreeMap;

use mcf_geo::P2;
use serde::{Deserialize, Serialize};

use crate::gabarits::{Famille, MAX_NIVEAUX};
use crate::ids::Id;
use crate::manifeste::Manifeste;

/// Valeur du champ `format`.
pub const FORMAT: &str = "mcfbatiments";
/// Version écrite.
pub const VERSION: u32 = 2;
/// Versions lues (la 1 n'a pas de bâtiments modélisés : même structure, champ absent).
pub const VERSIONS_LUES: [u32; 2] = [1, 2];
/// Bâtiments maximum sur une carte.
pub const MAX_BATIMENTS: usize = 200_000;
/// Sommets maximum d'une empreinte.
pub const MAX_SOMMETS: usize = 64;
/// Décalage vertical maximal d'un bâtiment par rapport au sol (m), vers le haut comme vers le bas.
pub const DECALAGE_Y_MAX_M: f64 = 50.0;
/// Inclinaison maximale d'un bâtiment modélisé autour de ses axes X et Z (degrés).
pub const INCLINAISON_MAX_DEG: f64 = 45.0;

fn est_zero(v: &f64) -> bool {
    *v == 0.0
}

/// D'où vient un bâtiment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origine {
    /// Produit par le lotissement : peut être régénéré.
    Genere,
    /// Posé ou retouché à la main : jamais touché par une régénération.
    Manuel,
}

/// Un bâtiment en bloc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batiment {
    /// Empreinte au sol, sens trigonométrique dans `(x, z)`.
    pub empreinte: Vec<P2>,
    /// Niveaux, rez-de-chaussée compris.
    pub niveaux: u32,
    /// Hauteur d'un niveau (m).
    pub hauteur_niveau_m: f64,
    /// Usage.
    pub famille: Famille,
    /// Origine.
    pub origine: Origine,
    /// Verrouillé : ni déplacé, ni supprimé, ni régénéré.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub verrou: bool,
    /// Affectation de quartier qui l'a généré.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affectation: Option<Id>,
    /// Bâtiment modélisé : l'empreinte est alors le rectangle de la fiche, tourné du cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modele: Option<ModelePose>,
    /// Décalage vertical par rapport au sol sous son centre (m, + vers le haut) : socle, bâtiment enfoncé dans
    /// une pente. Borné à ± [`DECALAGE_Y_MAX_M`].
    #[serde(default, skip_serializing_if = "est_zero")]
    pub decalage_y_m: f64,
}

/// Fiche de catalogue posée et son orientation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelePose {
    /// Identifiant de la fiche (`crate::catalogue`).
    pub fiche: String,
    /// Cap (degrés, [0 ; 360[) : rotation autour de +Y, main droite (vue de dessus, +X tourne vers −Z).
    pub cap_deg: f64,
    /// Tangage (degrés) : inclinaison autour de l'axe X du modèle, appliquée après le cap. Borné à
    /// ± [`INCLINAISON_MAX_DEG`]. L'empreinte reste le rectangle non incliné.
    #[serde(default, skip_serializing_if = "est_zero")]
    pub tangage_deg: f64,
    /// Roulis (degrés) : inclinaison autour de l'axe Z du modèle, appliquée après le tangage. Mêmes bornes.
    #[serde(default, skip_serializing_if = "est_zero")]
    pub roulis_deg: f64,
}

/// Transformation d'un geste sur des bâtiments (gizmo, inspecteur, touche R).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Transformation {
    /// Décalage horizontal `(x, z)` (m), appliqué après la rotation.
    pub delta: P2,
    /// Décalage vertical (m).
    pub dy_m: f64,
    /// Rotation autour de +Y (degrés), autour de `pivot`.
    pub cap_deg: f64,
    /// Centre de la rotation.
    pub pivot: P2,
    /// Tangage ajouté (degrés, bâtiments modélisés seulement).
    pub tangage_deg: f64,
    /// Roulis ajouté (degrés, bâtiments modélisés seulement).
    pub roulis_deg: f64,
}

impl Transformation {
    /// Déplacement horizontal seul.
    pub fn deplacement(delta: P2) -> Self {
        Self {
            delta,
            ..Self::default()
        }
    }

    /// Rotation seule autour de +Y.
    pub fn rotation(cap_deg: f64, pivot: P2) -> Self {
        Self {
            cap_deg,
            pivot,
            ..Self::default()
        }
    }

    /// Rien ne bouge (à 5 cm et un centième de degré près).
    pub fn est_nulle(&self) -> bool {
        self.delta[0].hypot(self.delta[1]) < 0.05
            && self.dy_m.abs() < 0.005
            && [self.cap_deg, self.tangage_deg, self.roulis_deg]
                .iter()
                .all(|a| a.abs() < 0.01)
    }

    /// Ne touche que l'inclinaison (sans effet sur un bloc).
    pub fn inclinaison_seule(&self) -> bool {
        (self.tangage_deg != 0.0 || self.roulis_deg != 0.0)
            && self.delta == [0.0, 0.0]
            && self.dy_m == 0.0
            && self.cap_deg == 0.0
    }
}

/// Ramène un angle en degrés dans [0 ; 360[.
pub fn normaliser_cap(deg: f64) -> f64 {
    let c = deg.rem_euclid(360.0);
    // `rem_euclid` peut rendre 360 pour un très petit négatif.
    if c >= 360.0 { 0.0 } else { c }
}

/// Tourne `p` de `deg` autour de `pivot`, comme `Quat::from_rotation_y` (x' = x cos + z sin, z' = −x sin + z cos).
pub fn tourner(p: P2, pivot: P2, deg: f64) -> P2 {
    let (s, c) = deg.to_radians().sin_cos();
    let (x, z) = (p[0] - pivot[0], p[1] - pivot[1]);
    [pivot[0] + x * c + z * s, pivot[1] - x * s + z * c]
}

/// Empreinte d'un bâtiment modélisé : rectangle `largeur` (X du modèle) × `profondeur` (Z du modèle) centré sur
/// `centre`, tourné de `cap_deg`. Sens trigonométrique dans `(x, z)`, comme toute empreinte.
pub fn empreinte_rectangle(centre: P2, largeur: f64, profondeur: f64, cap_deg: f64) -> Vec<P2> {
    let (a, b) = (largeur * 0.5, profondeur * 0.5);
    let coins = [[-a, -b], [a, -b], [a, b], [-a, b]]
        .map(|q| tourner([centre[0] + q[0], centre[1] + q[1]], centre, cap_deg));
    mcf_geo::polygone::oriente_positif(&coins)
}

impl Batiment {
    /// Hauteur totale (m).
    pub fn hauteur(&self) -> f64 {
        f64::from(self.niveaux) * self.hauteur_niveau_m
    }

    /// Applique `t` : rotation autour de `t.pivot` puis décalage (empreinte et cap), décalage vertical et
    /// inclinaison (modélisés), ramenés dans leurs bornes plutôt que refusés.
    pub fn transformer(&mut self, t: &Transformation) {
        for p in &mut self.empreinte {
            let q = tourner(*p, t.pivot, t.cap_deg);
            *p = [q[0] + t.delta[0], q[1] + t.delta[1]];
        }
        self.decalage_y_m = (self.decalage_y_m + t.dy_m).clamp(-DECALAGE_Y_MAX_M, DECALAGE_Y_MAX_M);
        if let Some(m) = &mut self.modele {
            let borne = |v: f64| v.clamp(-INCLINAISON_MAX_DEG, INCLINAISON_MAX_DEG);
            m.cap_deg = normaliser_cap(m.cap_deg + t.cap_deg);
            m.tangage_deg = borne(m.tangage_deg + t.tangage_deg);
            m.roulis_deg = borne(m.roulis_deg + t.roulis_deg);
        }
    }

    /// Centre de l'empreinte.
    pub fn centre(&self) -> P2 {
        mcf_geo::polygone::centroide(&self.empreinte)
    }

    /// Vérifie le bâtiment.
    pub fn valider(&self, m: &Manifeste) -> Result<(), String> {
        if self.empreinte.len() < 3 || self.empreinte.len() > MAX_SOMMETS {
            return Err("empreinte : 3 à 64 sommets".into());
        }
        if self.empreinte.iter().any(|p| !m.contient(*p)) || !mcf_geo::polygone::est_simple(&self.empreinte) {
            return Err("empreinte hors carte ou croisée".into());
        }
        if !(1..=MAX_NIVEAUX).contains(&self.niveaux) || !(2.0..=12.0).contains(&self.hauteur_niveau_m) {
            return Err("niveaux ou hauteur hors bornes".into());
        }
        if !(self.decalage_y_m.is_finite() && self.decalage_y_m.abs() <= DECALAGE_Y_MAX_M) {
            return Err(format!("décalage vertical hors de ± {DECALAGE_Y_MAX_M} m"));
        }
        if let Some(m) = &self.modele {
            if ![m.tangage_deg, m.roulis_deg]
                .iter()
                .all(|a| a.is_finite() && a.abs() <= INCLINAISON_MAX_DEG)
            {
                return Err(format!("inclinaison hors de ± {INCLINAISON_MAX_DEG}°"));
            }
            if !crate::manifeste::nom_sur(&m.fiche) {
                return Err("fiche de modèle : identifiant non sûr".into());
            }
            if !(m.cap_deg.is_finite() && (0.0..360.0).contains(&m.cap_deg)) {
                return Err("cap hors de [0 ; 360[".into());
            }
            if self.empreinte.len() != 4 {
                return Err("bâtiment modélisé : l'empreinte doit être un rectangle".into());
            }
        }
        Ok(())
    }
}

/// Contenu d'un fichier de cellule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FichierBatiments {
    /// Toujours `mcfbatiments`.
    pub format: String,
    /// Version.
    pub version: u32,
    /// Bâtiments de la cellule.
    pub batiments: BTreeMap<Id, Batiment>,
}

/// Cellule d'un point (tuiles du manifeste).
pub fn cellule(m: &Manifeste, p: P2) -> (usize, usize) {
    let (a, _) = m.bornes();
    let t = f64::from(m.tuile_m);
    let (nx, nz) = m.tuiles();
    let c = |v: f64, o: f64, n: usize| (((v - o) / t).floor().max(0.0) as usize).min(n.saturating_sub(1));
    (c(p[0], a[0], nx), c(p[1], a[1], nz))
}

/// Range les bâtiments par cellule.
pub fn par_cellule(
    m: &Manifeste,
    tous: &BTreeMap<Id, Batiment>,
) -> BTreeMap<(usize, usize), BTreeMap<Id, Batiment>> {
    let mut v: BTreeMap<(usize, usize), BTreeMap<Id, Batiment>> = BTreeMap::new();
    for (id, b) in tous {
        v.entry(cellule(m, b.centre()))
            .or_default()
            .insert(*id, b.clone());
    }
    v
}
