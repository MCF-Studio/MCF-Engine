//! Conception de carte 2D ↔ 3D (spec `docs/SPEC_CONCEPTION_CARTE_2D_3D.md`).
//!
//! Un seul document (`mcf_carte::Carte`), une seule scène Bevy, deux caméras : la vue **Plan** (orthographique
//! verticale) et la vue **3D**. Toute la géométrie vient de `mcf_carte` / `mcf_geo` ; les deux vues affichent les
//! mêmes maillages, seul le style change (couche de rendu « plan » à aplats, couche « 3D » éclairée). egui ne dessine
//! aucune géométrie de carte : seulement les panneaux.

mod camera;
mod outils;
mod scene;
mod ui;

use std::path::PathBuf;

use bevy::{
    camera::{CameraOutputMode, visibility::RenderLayers},
    prelude::*,
    render::render_resource::BlendState,
};
use bevy_egui::{EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext};
use mcf_carte::{Carte, Densite, Famille, Id, relief::Pinceau};
use mcf_geo::{ClasseRoute, P2};

/// Couche de rendu vue seulement par la caméra Plan.
pub const COUCHE_PLAN: usize = 1;
/// Couche de rendu vue seulement par la caméra 3D.
pub const COUCHE_3D: usize = 2;

/// Le projet de carte ouvert : l'unique source de vérité.
#[derive(Resource)]
pub struct Projet {
    /// Document (données sources, historique, dérivés).
    pub carte: Carte,
    /// Dernier message pour la barre d'état.
    pub message: String,
    /// Le message est une erreur.
    pub erreur: bool,
}

impl Projet {
    fn info(&mut self, m: impl Into<String>) {
        self.message = m.into();
        self.erreur = false;
    }

    fn echec(&mut self, m: impl Into<String>) {
        self.message = m.into();
        self.erreur = true;
    }

    /// Note le résultat d'une commande.
    fn resultat<T>(&mut self, r: Result<T, String>, ok: impl FnOnce(T) -> String) {
        match r {
            Ok(v) => {
                let m = ok(v);
                self.info(m);
            }
            Err(e) => self.echec(e),
        }
    }
}

/// Étapes du déroulé de conception (spec §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Etape {
    /// Créer / ouvrir la carte.
    Cadre,
    /// Sculpter le relief.
    Relief,
    /// Tracer le réseau routier.
    Reseau,
    /// Affecter les îlots.
    Quartiers,
    /// Faire pousser et ajuster le bâti.
    Bati,
    /// Exporter.
    Export,
}

impl Etape {
    /// Toutes les étapes, dans l'ordre.
    pub const TOUTES: [Self; 6] = [
        Self::Cadre,
        Self::Relief,
        Self::Reseau,
        Self::Quartiers,
        Self::Bati,
        Self::Export,
    ];

    /// Sous-titre (ce qu'on y fait).
    pub const fn resume(self) -> &'static str {
        match self {
            Self::Cadre => "Créer, ouvrir, enregistrer la carte",
            Self::Relief => "Collines, vallées, plateaux",
            Self::Reseau => "Tracer et modifier les routes",
            Self::Quartiers => "Donner un usage aux îlots",
            Self::Bati => "Faire pousser et ajuster le bâti",
            Self::Export => "Plan, minimap, graphe GPS",
        }
    }

    /// Couleur de l'étape (sRGB).
    pub const fn couleur(self) -> [u8; 3] {
        match self {
            Self::Cadre => [96, 125, 160],
            Self::Relief => [126, 150, 74],
            Self::Reseau => [214, 132, 52],
            Self::Quartiers => [150, 100, 190],
            Self::Bati => [62, 140, 200],
            Self::Export => [40, 160, 150],
        }
    }

    /// Nom affiché.
    pub const fn nom(self) -> &'static str {
        match self {
            Self::Cadre => "1. Cadre",
            Self::Relief => "2. Relief",
            Self::Reseau => "3. Réseau",
            Self::Quartiers => "4. Quartiers",
            Self::Bati => "5. Bâti",
            Self::Export => "6. Exporter",
        }
    }

    /// Outil proposé en entrant dans l'étape.
    const fn outil_par_defaut(self) -> Outil {
        match self {
            Self::Cadre | Self::Export => Outil::Selection,
            Self::Relief => Outil::Pinceau(Pinceau::Lever),
            Self::Reseau => Outil::Tracer,
            Self::Quartiers => Outil::Affecter,
            Self::Bati => Outil::Selection,
        }
    }
}

/// Outil actif.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outil {
    /// Sélectionner, déplacer (bâtiments, points de tracé).
    Selection,
    /// Tracer une route (clics, Entrée pour finir).
    Tracer,
    /// Sculpter le relief.
    Pinceau(Pinceau),
    /// Affecter l'îlot cliqué à une famille et une densité.
    Affecter,
    /// Dessiner une zone libre (clics, Entrée pour finir).
    ZoneLibre,
    /// Faire pousser le bâti du quartier cliqué.
    Lotir,
}

/// Ce qui est sélectionné.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Selection {
    /// Rien.
    #[default]
    Rien,
    /// Un ou plusieurs bâtiments.
    Batiments(Vec<Id>),
    /// Un tronçon de route.
    Troncon(Id),
    /// Une affectation d'îlot ou une zone libre.
    Quartier(Id),
}

/// Ce qu'on fait glisser.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CibleGlisse {
    /// Les bâtiments sélectionnés.
    Batiments,
    /// Un point de contrôle d'un tronçon (`index` dans la polyligne de contrôle, extrémités comprises).
    Poignee {
        /// Tronçon.
        troncon: Id,
        /// Indice du point.
        index: usize,
        /// Nombre de points de contrôle.
        nombre: usize,
    },
}

/// Un geste de glisser en cours : rien n'est écrit dans la carte avant le relâchement (une seule annulation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glisse {
    /// Point de départ (m).
    pub depart: P2,
    /// Point courant (m).
    pub actuel: P2,
    /// Cible.
    pub cible: CibleGlisse,
}

/// Disposition des vues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Plan seul.
    Plan,
    /// 3D avec la mini-carte (Plan en incrustation).
    Perspective,
    /// Plan à gauche, 3D à droite.
    Partagee,
}

/// État d'édition (outils, sélection, saisies de l'interface).
#[derive(Resource)]
pub struct Edition {
    /// Étape courante.
    pub etape: Etape,
    /// Outil courant.
    pub outil: Outil,
    /// Classe des routes tracées.
    pub classe: ClasseRoute,
    /// Famille affectée.
    pub famille: Famille,
    /// Densité affectée.
    pub densite: Densite,
    /// Rayon du pinceau (m).
    pub rayon: f64,
    /// Force du pinceau (m/s pour lever / abaisser / naturel, fraction par seconde sinon).
    pub force: f64,
    /// Altitude cible de « Aplanir » (m).
    pub cible: f64,
    /// Points cliqués du tracé ou de la zone en cours.
    pub points: Vec<P2>,
    /// Sélection.
    pub selection: Selection,
    /// Glisser en cours.
    pub glisse: Option<Glisse>,
    /// Disposition des vues.
    pub disposition: Disposition,
    /// Point de la carte sous la souris (m) et son altitude.
    pub curseur: Option<(P2, f64)>,
    /// Accroche des tracés aux routes existantes (désactivable avec Alt).
    pub accroche: bool,
    /// Chemin saisi (ouvrir / enregistrer).
    pub chemin: String,
    /// Nom saisi d'une nouvelle carte.
    pub nouveau_nom: String,
    /// Côté saisi d'une nouvelle carte (m).
    pub nouvelle_taille: f64,
    /// Autoriser le remplacement d'un export existant.
    pub ecraser_export: bool,
    /// Bâtiments en conflit avec une emprise (recalculé avec la scène).
    pub conflits: Vec<Id>,
    /// Durée du dernier recalcul de la scène (ms), pour les budgets du §12.
    pub duree_scene_ms: f64,
    /// Altitude figée du plan visé pendant un coup de pinceau en 3D (sinon le pinceau « grimpe » sur la colline
    /// qu'il est en train de lever).
    pub plan_pinceau: Option<f64>,
    /// Nom de voie en cours de saisie (tronçon, texte).
    pub nom_saisi: Option<(Id, String)>,
}

impl Default for Edition {
    fn default() -> Self {
        Self {
            etape: Etape::Reseau,
            outil: Outil::Tracer,
            classe: ClasseRoute::Rue,
            famille: Famille::Residentiel,
            densite: Densite::Moyenne,
            rayon: 20.0,
            force: 3.0,
            cible: 0.0,
            points: Vec::new(),
            selection: Selection::Rien,
            glisse: None,
            disposition: Disposition::Perspective,
            curseur: None,
            accroche: true,
            chemin: String::new(),
            nouveau_nom: "ma_carte".into(),
            nouvelle_taille: 1536.0,
            ecraser_export: false,
            conflits: Vec::new(),
            duree_scene_ms: 0.0,
            plan_pinceau: None,
            nom_saisi: None,
        }
    }
}

impl Edition {
    /// Change d'étape (abandonne le tracé en cours).
    pub fn aller_a(&mut self, etape: Etape) {
        self.etape = etape;
        self.outil = etape.outil_par_defaut();
        self.points.clear();
        self.glisse = None;
    }
}

/// Zone de l'écran laissée aux vues par les panneaux, et état de la souris / du clavier côté egui (image
/// précédente).
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct ZoneVues {
    /// Rectangle logique (pixels de fenêtre) des vues.
    pub rect: Rect,
    /// La souris est au-dessus d'un panneau egui.
    pub souris_sur_ui: bool,
    /// Un champ egui a le clavier.
    pub clavier_ui: bool,
}

/// Lance l'éditeur en mode Conception. `dossier` : projet de carte à ouvrir (sinon la carte de test).
pub fn lancer(dossier: Option<PathBuf>) -> AppExit {
    let (carte, message, erreur) = match dossier.as_deref().map(Carte::ouvrir) {
        Some(Ok(c)) => {
            let m = format!("Carte « {} » ouverte.", c.manifeste.nom);
            (c, m, false)
        }
        Some(Err(e)) => (
            carte_de_test(),
            format!("Ouverture refusée : {e}. Carte de test affichée."),
            true,
        ),
        None => (
            carte_de_test(),
            "Carte de test chargée. Choisissez une étape à gauche ; Tab bascule Plan / 3D.".into(),
            false,
        ),
    };
    let mut edition = Edition::default();
    if let Some(d) = &dossier {
        edition.chemin = d.display().to_string();
    }
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "MCF Editor — Conception de carte 2D ↔ 3D".into(),
                resolution: (1600, 900).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .insert_resource(Projet {
            carte,
            message,
            erreur,
        })
        .insert_resource(edition)
        .init_resource::<ZoneVues>()
        .add_plugins((camera::CamerasPlugin, scene::ScenePlugin, outils::OutilsPlugin))
        .add_systems(Startup, preparer)
        .add_systems(EguiPrimaryContextPass, ui::interface)
        .run()
}

/// Carte de test de référence (`mcf_carte::carte_de_test`). Elle est construite par du code testé : un échec ici
/// serait un bogue, on repart alors d'une carte vide plutôt que de planter.
fn carte_de_test() -> Carte {
    mcf_carte::carte_de_test::construire()
        .or_else(|_| Carte::nouvelle(mcf_carte::Manifeste::nouveau("carte_vide", 512.0)))
        .expect("une carte vide de 512 m est toujours valide")
}

fn preparer(mut commands: Commands, mut egui: ResMut<EguiGlobalSettings>) {
    // Caméra dédiée à l'interface : les caméras de carte gardent leur viewport sans déplacer egui.
    egui.auto_create_primary_context = false;
    commands.spawn((
        PrimaryEguiContext,
        Camera2d,
        RenderLayers::none(),
        Camera {
            order: 10,
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            clear_color: ClearColorConfig::None,
            ..default()
        },
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 350.0,
        ..default()
    });
    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-0.5, 1.0, -0.35).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
