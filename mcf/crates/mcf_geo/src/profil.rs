//! Classes de route et profils en travers (spec §4.2). Les profils sont des **données** : la table par défaut est
//! écrite dans le manifeste de chaque carte, qui peut ensuite la corriger.

use serde::{Deserialize, Serialize};

/// Classe d'une route, de la plus modeste à la plus large.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClasseRoute {
    /// Piéton, parc.
    Sentier,
    /// Campagne, champs.
    CheminTerre,
    /// Interurbaine, deux voies.
    Route,
    /// Desserte urbaine étroite.
    Ruelle,
    /// Rue standard, deux voies.
    Rue,
    /// Axe urbain, quatre voies et stationnement.
    Avenue,
    /// Voie express : 2 × 2 voies sans séparation (identifiant historique `voie_rapide`).
    VoieRapide,
    /// Autoroute : 2 × 2 voies, bandes d'arrêt d'urgence, terre-plein central.
    Autoroute,
}

impl ClasseRoute {
    /// Toutes les classes, dans l'ordre.
    pub const TOUTES: [Self; 8] = [
        Self::Sentier,
        Self::CheminTerre,
        Self::Route,
        Self::Ruelle,
        Self::Rue,
        Self::Avenue,
        Self::VoieRapide,
        Self::Autoroute,
    ];

    /// Identifiant stable dans les fichiers.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Sentier => "sentier",
            Self::CheminTerre => "chemin_terre",
            Self::Route => "route",
            Self::Ruelle => "ruelle",
            Self::Rue => "rue",
            Self::Avenue => "avenue",
            Self::VoieRapide => "voie_rapide",
            Self::Autoroute => "autoroute",
        }
    }

    /// Classe d'après son identifiant (`None` si inconnu : aucune valeur par défaut silencieuse).
    pub fn depuis_id(id: &str) -> Option<Self> {
        Self::TOUTES.into_iter().find(|c| c.id() == id)
    }

    /// Nom affiché.
    pub const fn nom(self) -> &'static str {
        match self {
            Self::Sentier => "Sentier",
            Self::CheminTerre => "Chemin de terre",
            Self::Route => "Route",
            Self::Ruelle => "Ruelle",
            Self::Rue => "Rue",
            Self::Avenue => "Avenue",
            Self::VoieRapide => "Voie express",
            Self::Autoroute => "Autoroute",
        }
    }
}

/// Ce qui borde la chaussée de chaque côté.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cote {
    /// Rien.
    Aucun,
    /// Bande de sol stabilisé (campagne).
    Accotement,
    /// Trottoir surélevé (ville).
    Trottoir,
}

/// Nature du revêtement : décide si la route est un maillage ou une peinture du sol (spec §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Revetement {
    /// Enrobé : maillage généré, bords nets.
    Revetu,
    /// Terre : peint dans le matériau du terrain.
    Terre,
}

/// Profil en travers d'une classe.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfilRoute {
    /// Largeur de la chaussée (m).
    pub chaussee_m: f64,
    /// Largeur de chaque côté (m).
    pub cote_m: f64,
    /// Nature des côtés.
    pub cote: Cote,
    /// Revêtement.
    pub revetement: Revetement,
    /// Pente longitudinale maximale (0,08 = 8 %).
    pub pente_max: f64,
    /// Couleur sur le plan stylisé (sRGB).
    pub couleur_plan: [u8; 3],
    /// Terre-plein central (m), compris dans la chaussée : sépare les deux sens (0 = aucun).
    #[serde(default)]
    pub terre_plein_m: f64,
}

/// Couleur du terre-plein central sur le plan stylisé (sRGB) : vert gris, lisible sur le bleu de l'autoroute.
pub const COULEUR_TERRE_PLEIN: [u8; 3] = [118, 150, 104];

/// Bornes de validation d'un profil.
pub const CHAUSSEE_MIN_M: f64 = 0.5;
/// Largeur de chaussée maximale.
pub const CHAUSSEE_MAX_M: f64 = 60.0;
/// Largeur maximale d'un côté.
pub const COTE_MAX_M: f64 = 15.0;

impl ProfilRoute {
    /// Largeur totale, bord à bord (« façade à façade » en ville).
    pub fn emprise_m(&self) -> f64 {
        self.chaussee_m + 2.0 * self.cote_m
    }

    /// Vérifie les bornes (un manifeste est une donnée non fiable).
    pub fn valider(&self) -> Result<(), String> {
        if !(CHAUSSEE_MIN_M..=CHAUSSEE_MAX_M).contains(&self.chaussee_m) {
            return Err(format!(
                "chaussée hors bornes ({CHAUSSEE_MIN_M} à {CHAUSSEE_MAX_M} m)"
            ));
        }
        if !(0.0..=COTE_MAX_M).contains(&self.cote_m) {
            return Err(format!("côté hors bornes (0 à {COTE_MAX_M} m)"));
        }
        if (self.cote == Cote::Aucun) != (self.cote_m == 0.0) {
            return Err("un côté « aucun » doit avoir une largeur nulle, et inversement".into());
        }
        if !(0.0..=self.chaussee_m - 2.0 * CHAUSSEE_MIN_M).contains(&self.terre_plein_m) {
            return Err("terre-plein central hors bornes (0 à la chaussée moins deux voies)".into());
        }
        if !(0.01..=1.0).contains(&self.pente_max) {
            return Err("pente maximale hors bornes (1 % à 100 %)".into());
        }
        Ok(())
    }
}

/// Table complète des profils : une entrée par classe, toujours toutes présentes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableProfils {
    /// Profil `sentier`.
    pub sentier: ProfilRoute,
    /// Profil `chemin_terre`.
    pub chemin_terre: ProfilRoute,
    /// Profil `route`.
    pub route: ProfilRoute,
    /// Profil `ruelle`.
    pub ruelle: ProfilRoute,
    /// Profil `rue`.
    pub rue: ProfilRoute,
    /// Profil `avenue`.
    pub avenue: ProfilRoute,
    /// Profil `voie_rapide` (voie express).
    pub voie_rapide: ProfilRoute,
    /// Profil `autoroute` (absent des cartes enregistrées avant son ajout : valeur par défaut).
    #[serde(default = "autoroute_defaut")]
    pub autoroute: ProfilRoute,
}

const fn autoroute_defaut() -> ProfilRoute {
    TableProfils::DEFAUT.autoroute
}

const fn profil(
    chaussee_m: f64,
    cote_m: f64,
    cote: Cote,
    revetement: Revetement,
    pente_max: f64,
    couleur_plan: [u8; 3],
) -> ProfilRoute {
    ProfilRoute {
        chaussee_m,
        cote_m,
        cote,
        revetement,
        pente_max,
        couleur_plan,
        terre_plein_m: 0.0,
    }
}

impl TableProfils {
    /// Valeurs par défaut de la spec (§4.2) : 3,5 m par voie, 2,5 m de stationnement, rue ≈ 14 m et avenue ≈ 24 m
    /// façade à façade (doc 05 du jeu).
    pub const DEFAUT: Self = Self {
        sentier: profil(1.5, 0.0, Cote::Aucun, Revetement::Terre, 0.25, [150, 115, 75]),
        chemin_terre: profil(
            3.5,
            0.5,
            Cote::Accotement,
            Revetement::Terre,
            0.15,
            [196, 164, 120],
        ),
        route: profil(
            6.5,
            1.5,
            Cote::Accotement,
            Revetement::Revetu,
            0.08,
            [246, 214, 92],
        ),
        ruelle: profil(
            5.0,
            1.5,
            Cote::Trottoir,
            Revetement::Revetu,
            0.12,
            [255, 255, 255],
        ),
        rue: profil(
            7.0,
            3.5,
            Cote::Trottoir,
            Revetement::Revetu,
            0.10,
            [255, 255, 255],
        ),
        avenue: profil(
            19.0,
            2.5,
            Cote::Trottoir,
            Revetement::Revetu,
            0.06,
            [245, 160, 70],
        ),
        voie_rapide: profil(
            17.0,
            2.5,
            Cote::Accotement,
            Revetement::Revetu,
            0.05,
            [220, 70, 60],
        ),
        // Par sens : bande d'arrêt d'urgence 2,5 m + 2 voies de 3,5 m + bande dérasée 1 m = 10,5 m ; terre-plein 3 m.
        autoroute: ProfilRoute {
            terre_plein_m: 3.0,
            ..profil(
                24.0,
                3.0,
                Cote::Accotement,
                Revetement::Revetu,
                0.05,
                [70, 120, 210],
            )
        },
    };

    /// Profil d'une classe.
    pub const fn get(&self, classe: ClasseRoute) -> &ProfilRoute {
        match classe {
            ClasseRoute::Sentier => &self.sentier,
            ClasseRoute::CheminTerre => &self.chemin_terre,
            ClasseRoute::Route => &self.route,
            ClasseRoute::Ruelle => &self.ruelle,
            ClasseRoute::Rue => &self.rue,
            ClasseRoute::Avenue => &self.avenue,
            ClasseRoute::VoieRapide => &self.voie_rapide,
            ClasseRoute::Autoroute => &self.autoroute,
        }
    }

    /// Vérifie tous les profils.
    pub fn valider(&self) -> Result<(), String> {
        for classe in ClasseRoute::TOUTES {
            self.get(classe)
                .valider()
                .map_err(|e| format!("profil « {} » : {e}", classe.id()))?;
        }
        Ok(())
    }
}

impl Default for TableProfils {
    fn default() -> Self {
        Self::DEFAUT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emprises_de_la_spec() {
        let t = TableProfils::DEFAUT;
        assert_eq!(t.rue.emprise_m(), 14.0);
        assert_eq!(t.avenue.emprise_m(), 24.0);
        assert_eq!(t.voie_rapide.emprise_m(), 22.0);
        assert_eq!(t.autoroute.emprise_m(), 30.0);
        assert_eq!(t.autoroute.terre_plein_m, 3.0);
        assert!(t.valider().is_ok());
    }

    #[test]
    fn identifiants_aller_retour() {
        for c in ClasseRoute::TOUTES {
            assert_eq!(ClasseRoute::depuis_id(c.id()), Some(c));
        }
        assert_eq!(ClasseRoute::depuis_id("chemin"), None);
    }

    #[test]
    fn profil_hors_bornes_refuse() {
        let mut t = TableProfils::DEFAUT;
        t.rue.chaussee_m = f64::NAN;
        assert!(t.valider().is_err());
        let mut t = TableProfils::DEFAUT;
        t.sentier.cote_m = 1.0;
        assert!(t.valider().is_err());
    }

    #[test]
    fn json_strict() {
        let json = serde_json::to_string(&TableProfils::DEFAUT).expect("sérialisation");
        let relu: TableProfils = serde_json::from_str(&json).expect("relecture");
        assert_eq!(relu, TableProfils::DEFAUT);
        let piege = json.replacen("\"pente_max\"", "\"x\":1,\"pente_max\"", 1);
        assert!(serde_json::from_str::<TableProfils>(&piege).is_err());
    }

    #[test]
    fn carte_enregistree_avant_l_autoroute_reste_lisible() {
        let mut v = serde_json::to_value(TableProfils::DEFAUT).expect("sérialisation");
        let objet = v.as_object_mut().expect("objet");
        objet.remove("autoroute");
        for p in objet.values_mut() {
            p.as_object_mut().expect("profil").remove("terre_plein_m");
        }
        let relu: TableProfils = serde_json::from_value(v).expect("relecture");
        assert_eq!(relu, TableProfils::DEFAUT);
    }

    #[test]
    fn terre_plein_trop_large_refuse() {
        let mut t = TableProfils::DEFAUT;
        t.autoroute.terre_plein_m = 23.5;
        assert!(t.valider().is_err());
        t.autoroute.terre_plein_m = f64::NAN;
        assert!(t.valider().is_err());
    }
}
